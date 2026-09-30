//! Root service reached through a protected Unix socket and systemd/polkit authorization.
mod credentials;
use super::{
    HelperStatus, RegistrationState,
    framing::{read_body, write_frame},
};
use crate::{
    HelperError, HelperServer, Result,
    protocol::{OperationResult, SignedFrame, WireError},
    server::PeerIdentity,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const CANCEL_RESULT_GRACE: Duration = Duration::from_secs(10);
const CONNECTION_CLEANUP_TIMEOUT: Duration = Duration::from_secs(12);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(86_400);
const SOCKET_ROOT: &str = "/run/network-lantern";
static STARTUP: Mutex<Option<ServiceStartup>> = Mutex::new(None);

struct ServiceStartup {
    uid: u32,
    _instance_lock: File,
}

struct CredentialLease {
    uid: u32,
    active: bool,
}

impl CredentialLease {
    fn new(uid: u32) -> Self {
        Self { uid, active: true }
    }

    fn revoke(&mut self) -> Result<()> {
        credentials::revoke(self.uid)?;
        self.active = false;
        Ok(())
    }
}

impl Drop for CredentialLease {
    fn drop(&mut self) {
        if self.active {
            let _ = credentials::revoke(self.uid);
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Message {
    Status,
    PrepareRemoval,
    AbortRemoval,
    CancelRun { run_id: uuid::Uuid },
    Operation { frame: SignedFrame },
}
fn owner_uid() -> Result<u32> {
    std::env::var("NETWORK_LANTERN_OWNER_UID")
        .map_err(|_| {
            HelperError::Authorization("The system service must supply its owner UID".into())
        })?
        .parse()
        .map_err(|_| HelperError::Validation("Service owner UID is invalid".into()))
}
fn socket_path(uid: u32) -> PathBuf {
    PathBuf::from(SOCKET_ROOT).join(format!("helper-{uid}.sock"))
}
fn protected_runtime(create: bool) -> Result<()> {
    // Every existing ancestor must be root-owned and immune to unprivileged replacement.
    for path in [Path::new("/run"), Path::new(SOCKET_ROOT)] {
        if create && path == Path::new(SOCKET_ROOT) {
            match std::fs::create_dir(path) {
                Ok(()) => std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| HelperError::transport("protect runtime directory", e))?,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(HelperError::transport("create runtime directory", e)),
            }
        }
        let meta = std::fs::symlink_metadata(path)
            .map_err(|e| HelperError::transport("inspect runtime directory", e))?;
        if !meta.is_dir()
            || meta.file_type().is_symlink()
            || meta.uid() != 0
            || meta.mode() & 0o022 != 0
        {
            return Err(HelperError::Authorization(
                "Runtime directories must be root-owned and not writable by other users".into(),
            ));
        }
    }
    Ok(())
}
pub(super) async fn status() -> Result<HelperStatus> {
    let uid = unsafe { libc::geteuid() };
    let socket = socket_path(uid);
    let metadata = match std::fs::symlink_metadata(&socket) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HelperStatus {
                registration: RegistrationState::NotRegistered,
                service_reachable: false,
                authorized: false,
                detail: Some(
                    "The per-user system helper is stopped or its deployment assets are absent"
                        .into(),
                ),
            });
        }
        Err(e) => return Err(HelperError::transport("inspect helper socket", e)),
    };
    protected_runtime(false)?;
    if !metadata.file_type().is_socket() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        return Err(HelperError::Authorization(
            "Helper socket has an unsafe owner or mode".into(),
        ));
    }
    let reachable = exchange(Message::Status, &CancellationToken::new(), FRAME_TIMEOUT)
        .await
        .is_ok();
    let credential = load_secret().is_ok();
    Ok(HelperStatus {
        registration: RegistrationState::Registered,
        service_reachable: reachable,
        authorized: reachable && credential,
        detail: Some(
            if !reachable {
                "Service did not complete an authenticated root-peer status exchange"
            } else if !credential {
                "Per-user helper credential is absent or unsafe"
            } else {
                "Root service and private per-user credential verified"
            }
            .into(),
        ),
    })
}
pub(super) async fn register() -> Result<HelperStatus> {
    systemd_unit("StartUnit").await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let result = status().await?;
        if result.authorized {
            return Ok(result);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(HelperError::Unavailable("Service start did not produce an authenticated helper; inspect the installed unit and policy".into()));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
pub(super) async fn remove() -> Result<HelperStatus> {
    let prepared = status().await?.service_reachable;
    if prepared {
        exchange(
            Message::PrepareRemoval,
            &CancellationToken::new(),
            FRAME_TIMEOUT,
        )
        .await?;
    }
    let result = async {
        systemd_unit("StopUnit").await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let uid = unsafe { libc::geteuid() };
        loop {
            let result = status().await?;
            if result.registration == RegistrationState::NotRegistered {
                match credentials::present(uid) {
                    Ok(false) => return Ok(result),
                    Ok(true) => {}
                    Err(error) => {
                        return Err(HelperError::Unavailable(format!(
                            "Service stopped, but credential cleanup could not be verified: {error}"
                        )));
                    }
                }
            }
            if tokio::time::Instant::now() >= deadline {
                if result.registration == RegistrationState::NotRegistered {
                    return Err(HelperError::Unavailable(
                        "Service stopped, but credential cleanup did not finish within ten seconds"
                            .into(),
                    ));
                }
                return Err(HelperError::Unavailable(
                    "Service stop did not finish within ten seconds".into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    .await;
    if result.is_err() && prepared {
        let _ = exchange(
            Message::AbortRemoval,
            &CancellationToken::new(),
            FRAME_TIMEOUT,
        )
        .await;
    }
    result
}
pub(super) fn load_secret() -> Result<[u8; 32]> {
    credentials::read(unsafe { libc::geteuid() })
}

pub(super) fn cleanup_credentials() -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(HelperError::Authorization(
            "Linux credential cleanup must run as root".into(),
        ));
    }
    let uid = owner_uid()?;
    let _instance_lock = acquire_instance(uid)?;
    credentials::revoke(uid)
}
pub(super) async fn request(
    frame: SignedFrame,
    cancel: &CancellationToken,
) -> Result<OperationResult> {
    serde_json::from_value(exchange(Message::Operation { frame }, cancel, OPERATION_TIMEOUT).await?)
        .map_err(|e| HelperError::Protocol(e.to_string()))
}
async fn exchange(
    message: Message,
    cancel: &CancellationToken,
    deadline: Duration,
) -> Result<Value> {
    if cancel.is_cancelled() {
        return Err(HelperError::Cancelled);
    }
    let run_id = match &message {
        Message::Operation { frame } => Some(
            serde_json::from_slice::<crate::AuthorizedRequest>(&frame.body)
                .map_err(|e| HelperError::Protocol(e.to_string()))?
                .run_id,
        ),
        _ => None,
    };
    let response = roundtrip(message, deadline);
    tokio::pin!(response);
    tokio::select! {
        result=&mut response=>result,
        _=cancel.cancelled()=>{
            if let Some(run_id)=run_id {
                let _acknowledged=roundtrip(Message::CancelRun{run_id},FRAME_TIMEOUT).await?;
                return tokio::time::timeout(CANCEL_RESULT_GRACE,&mut response).await
                    .unwrap_or(Err(HelperError::Cancelled));
            }
            Err(HelperError::Cancelled)
        }
    }
}
async fn roundtrip(message: Message, deadline: Duration) -> Result<Value> {
    protected_runtime(false)?;
    let work = async {
        let mut stream = tokio::time::timeout(
            FRAME_TIMEOUT,
            UnixStream::connect(socket_path(unsafe { libc::geteuid() })),
        )
        .await
        .map_err(|_| HelperError::Unavailable("Helper connection timed out".into()))?
        .map_err(|e| HelperError::transport("connect helper", e))?;
        let peer = stream
            .peer_cred()
            .map_err(|e| HelperError::transport("authenticate helper", e))?;
        if peer.uid() != 0 {
            return Err(HelperError::Authentication);
        }
        write_frame(&mut stream, &message, FRAME_TIMEOUT).await?;
        // Long operation results may take time; once their header arrives, body reads are bounded.
        let length = stream
            .read_u32()
            .await
            .map_err(|e| HelperError::transport("read response length", e))?
            as usize;
        let bytes = read_body(&mut stream, length, FRAME_TIMEOUT).await?;
        let reply: std::result::Result<Value, WireError> =
            serde_json::from_slice(&bytes).map_err(|e| HelperError::Protocol(e.to_string()))?;
        reply.map_err(Into::into)
    };
    tokio::time::timeout(deadline, work)
        .await
        .map_err(|_| HelperError::Unavailable("Helper operation deadline exceeded".into()))?
}
async fn systemd_unit(method: &str) -> Result<()> {
    let unit = format!("network-lantern-helper@{}.service", unsafe {
        libc::geteuid()
    });
    let work = async {
        let connection = zbus::Connection::system()
            .await
            .map_err(|e| HelperError::Unavailable(format!("System bus: {e}")))?;
        let proxy = zbus::Proxy::new(
            &connection,
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
        )
        .await
        .map_err(|e| HelperError::Unavailable(format!("System service manager: {e}")))?;
        let reply: Option<zbus::zvariant::OwnedObjectPath> = proxy
            .call_with_flags(
                method,
                zbus::proxy::MethodFlags::AllowInteractiveAuth.into(),
                &(unit, "replace"),
            )
            .await
            .map_err(|e| {
                HelperError::Authorization(format!("System helper authorization denied: {e}"))
            })?;
        if reply.is_none() {
            return Err(HelperError::Protocol(
                "System service manager returned no job".into(),
            ));
        }
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(120), work)
        .await
        .map_err(|_| HelperError::Authorization("System authorization timed out".into()))?
}
pub(super) fn server_default() -> Result<HelperServer> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(HelperError::Authorization(
            "Linux helper must run as a root system service".into(),
        ));
    }
    let uid = owner_uid()?;
    let mut startup = STARTUP
        .lock()
        .map_err(|_| HelperError::Protocol("Helper startup lock poisoned".into()))?;
    if startup.is_some() {
        return Err(HelperError::Unavailable(
            "Helper service startup is already prepared".into(),
        ));
    }
    let instance_lock = acquire_instance(uid)?;
    let secret = credentials::provision(uid)?;
    *startup = Some(ServiceStartup {
        uid,
        _instance_lock: instance_lock,
    });
    Ok(HelperServer::new(secret, u64::from(uid)))
}
pub(super) fn run(server: HelperServer) -> Result<()> {
    let startup = STARTUP
        .lock()
        .map_err(|_| HelperError::Protocol("Helper startup lock poisoned".into()))?
        .take()
        .ok_or_else(|| {
            HelperError::Protocol("Helper service was not prepared for this process".into())
        })?;
    let mut credential = CredentialLease::new(startup.uid);
    let service_result = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| HelperError::transport("create helper runtime", e))?
        .block_on(serve(server, startup.uid));
    let revoke_result = credential.revoke();
    drop(startup);
    match (service_result, revoke_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(service), Err(revoke)) => Err(HelperError::Protocol(format!(
            "{service}; credential revocation also failed: {revoke}"
        ))),
    }
}

fn acquire_instance(uid: u32) -> Result<File> {
    protected_runtime(true)?;
    acquire_instance_at(Path::new(SOCKET_ROOT), uid, 0)
}

fn acquire_instance_at(directory: &Path, uid: u32, expected_owner: u32) -> Result<File> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(directory.join(format!("helper-{uid}.lock")))
        .map_err(|e| HelperError::transport("open instance lock", e))?;
    let meta = lock
        .metadata()
        .map_err(|e| HelperError::transport("inspect instance lock", e))?;
    if !meta.is_file()
        || meta.uid() != expected_owner
        || meta.mode() & 0o077 != 0
        || meta.nlink() != 1
    {
        return Err(HelperError::Authentication);
    }
    lock.try_lock()
        .map_err(|_| HelperError::Unavailable("Another helper instance owns this UID".into()))?;
    Ok(lock)
}
struct Service {
    server: HelperServer,
    admission: Arc<Semaphore>,
    removal: Mutex<Option<OwnedSemaphorePermit>>,
}
impl Service {
    async fn handle(
        self: Arc<Self>,
        message: Message,
        peer: PeerIdentity,
        cancel: CancellationToken,
    ) -> Result<Value> {
        match message {
            Message::Status => {
                Ok(json!({"protocol":1,"active":self.admission.available_permits()==0}))
            }
            Message::CancelRun { run_id } => {
                Ok(json!({"cancelled":self.server.cancel_run(run_id)}))
            }
            Message::PrepareRemoval => {
                let mut removal = self
                    .removal
                    .lock()
                    .map_err(|_| HelperError::Protocol("Removal lock poisoned".into()))?;
                if removal.is_none() {
                    let permit = self.admission.clone().try_acquire_owned().map_err(|_| {
                        HelperError::Scope(
                            "An operation is active; helper removal is refused".into(),
                        )
                    })?;
                    *removal = Some(permit);
                }
                Ok(json!({"removal_prepared":true}))
            }
            Message::AbortRemoval => {
                let released = self
                    .removal
                    .lock()
                    .map_err(|_| HelperError::Protocol("Removal lock poisoned".into()))?
                    .take()
                    .is_some();
                Ok(json!({"removal_aborted":released}))
            }
            Message::Operation { frame } => {
                let _permit = self.admission.clone().try_acquire_owned().map_err(|_| {
                    HelperError::Scope("Another helper operation or removal is active".into())
                })?;
                serde_json::to_value(self.server.handle(frame, peer, cancel).await?)
                    .map_err(|e| HelperError::Protocol(e.to_string()))
            }
        }
    }
}
async fn serve(server: HelperServer, uid: u32) -> Result<()> {
    let socket = socket_path(uid);
    if let Ok(meta) = std::fs::symlink_metadata(&socket) {
        if !meta.file_type().is_socket() || meta.uid() != uid {
            return Err(HelperError::Authentication);
        }
        std::fs::remove_file(&socket)
            .map_err(|e| HelperError::transport("remove stale socket", e))?;
    }
    let listener =
        UnixListener::bind(&socket).map_err(|e| HelperError::transport("bind helper socket", e))?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| HelperError::transport("protect socket", e))?;
    let name = std::ffi::CString::new(socket.as_os_str().as_encoded_bytes())
        .map_err(|_| HelperError::Authentication)?;
    if unsafe { libc::chown(name.as_ptr(), uid, u32::MAX) } != 0 {
        return Err(HelperError::transport(
            "assign socket owner",
            std::io::Error::last_os_error(),
        ));
    }
    let service = Arc::new(Service {
        server,
        admission: Arc::new(Semaphore::new(1)),
        removal: Mutex::new(None),
    });
    let slots = Arc::new(Semaphore::new(64));
    let shutdown = CancellationToken::new();
    let mut tasks = JoinSet::new();
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| HelperError::transport("install shutdown signal", e))?;
    loop {
        tokio::select! {
            _=term.recv()=>break,
            _=tokio::signal::ctrl_c()=>break,
            Some(_)=tasks.join_next(),if !tasks.is_empty()=>(),
            accepted=listener.accept()=>{
                let (stream,_)=accepted.map_err(|e|HelperError::transport("accept helper client",e))?;
                if let Ok(permit)=slots.clone().try_acquire_owned(){
                    let service=service.clone();let stop=shutdown.child_token();
                    tasks.spawn(async move {let _permit=permit;let _=serve_connection(service,stream,uid,stop).await;});
                }
            }
        }
    }
    shutdown.cancel();
    let _ = tokio::time::timeout(CONNECTION_CLEANUP_TIMEOUT, async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    tasks.abort_all();
    drop(listener);
    std::fs::remove_file(&socket)
        .map_err(|e| HelperError::transport("remove stopped helper socket", e))?;
    Ok(())
}
async fn serve_connection(
    service: Arc<Service>,
    stream: UnixStream,
    uid: u32,
    stop: CancellationToken,
) -> Result<()> {
    let credentials = stream
        .peer_cred()
        .map_err(|e| HelperError::transport("read client credentials", e))?;
    if credentials.uid() != uid {
        return Err(HelperError::Authentication);
    }
    let peer = PeerIdentity {
        user_id: u64::from(uid),
        process_id: credentials.pid().and_then(|p| u32::try_from(p).ok()),
        trusted_signature: true,
    };
    let (mut reader, mut writer) = stream.into_split();
    let read = async {
        let length = reader
            .read_u32()
            .await
            .map_err(|e| HelperError::transport("read request length", e))?
            as usize;
        let bytes = read_body(&mut reader, length, FRAME_TIMEOUT).await?;
        serde_json::from_slice::<Message>(&bytes).map_err(|e| HelperError::Protocol(e.to_string()))
    };
    let message = tokio::select! {_=stop.cancelled()=>return Err(HelperError::Cancelled),r=tokio::time::timeout(FRAME_TIMEOUT,read)=>r.map_err(|_|HelperError::Protocol("Request timed out".into()))??};
    let cancelled = stop.child_token();
    let guard = cancelled.clone().drop_guard();
    let token = cancelled.clone();
    let operation = service.handle(message, peer, token);
    tokio::pin!(operation);
    let mut unexpected = [0; 1];
    let (result, can_reply) = tokio::select! {
        _=stop.cancelled()=>{
            cancelled.cancel();
            (tokio::time::timeout(CONNECTION_CLEANUP_TIMEOUT,&mut operation).await
                .unwrap_or_else(|_|Err(HelperError::Scope("Connection cleanup timed out".into()))), false)
        },
        _=reader.read(&mut unexpected)=>{
            cancelled.cancel();
            (tokio::time::timeout(CONNECTION_CLEANUP_TIMEOUT,&mut operation).await
                .unwrap_or_else(|_|Err(HelperError::Scope("Connection cleanup timed out".into()))), false)
        },
        _=tokio::time::sleep(OPERATION_TIMEOUT)=>{
            cancelled.cancel();
            (tokio::time::timeout(CONNECTION_CLEANUP_TIMEOUT,&mut operation).await
                .unwrap_or_else(|_|Err(HelperError::Scope("Operation cleanup timed out".into()))), true)
        },
        result=&mut operation=>(result, true),
    };
    drop(guard);
    if !can_reply {
        return result.map(|_| ());
    }
    let response = result.map_err(WireError::from);
    write_frame(&mut writer, &response, FRAME_TIMEOUT).await?;
    writer
        .shutdown()
        .await
        .map_err(|e| HelperError::transport("finish response", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_lock_prevents_rotation_by_a_second_incarnation() {
        let temp = tempfile::tempdir().unwrap();
        let uid = unsafe { libc::geteuid() };
        let first = acquire_instance_at(temp.path(), uid, uid).unwrap();

        assert!(matches!(
            acquire_instance_at(temp.path(), uid, uid),
            Err(HelperError::Unavailable(message))
                if message.contains("Another helper instance")
        ));

        drop(first);
        acquire_instance_at(temp.path(), uid, uid).unwrap();
    }

    #[test]
    fn unit_uses_lock_aware_native_credential_cleanup() {
        let unit = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../deployment/helper/linux/network-lantern-helper@.service"
        ));

        assert!(
            unit.contains("ExecStopPost=/usr/libexec/network-lantern-helper --cleanup-credentials")
        );
        assert!(unit.contains("CapabilityBoundingSet=CAP_CHOWN CAP_DAC_READ_SEARCH CAP_NET_RAW"));
        assert!(!unit.contains("CAP_NET_ADMIN"));
    }
}
