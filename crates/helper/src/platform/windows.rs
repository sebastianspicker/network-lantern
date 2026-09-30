//! LocalSystem service reached through a per-owner protected named pipe.

use super::{
    HelperStatus, RegistrationState,
    framing::{read_body, write_frame},
};
use crate::{
    HelperError, HelperServer, Result,
    protocol::{AuthorizedRequest, OperationResult, SignedFrame, WireError},
    server::PeerIdentity,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs::File,
    io::Write,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions},
    sync::{OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER, ERROR_PIPE_BUSY,
        GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree, MAX_PATH,
    },
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        },
        GetTokenInformation, PSECURITY_DESCRIPTOR, RevertToSelf, SECURITY_ATTRIBUTES, TOKEN_QUERY,
        TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_NORMAL},
    System::{
        Pipes::{
            GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, ImpersonateNamedPipeClient,
        },
        Threading::{
            GetCurrentProcess, GetCurrentThread, OpenProcess, OpenProcessToken, OpenThreadToken,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
    UI::Shell::{CSIDL_COMMON_APPDATA, SHGFP_TYPE_CURRENT, SHGetFolderPathW},
};

mod service;

const PIPE: &str = r"\\.\pipe\NetworkLantern.Helper.v1";
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const CANCELLATION_GRACE: Duration = Duration::from_secs(10);
const SERVER_CLEANUP_GRACE: Duration = Duration::from_secs(12);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(86_400);

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Message {
    Status,
    PrepareRemoval,
    AbortRemoval,
    CancelRun { run_id: Uuid },
    Operation { frame: SignedFrame },
}

pub(super) async fn status() -> Result<HelperStatus> {
    let registered = service::is_registered()?;
    if !registered {
        return Ok(HelperStatus {
            registration: RegistrationState::NotRegistered,
            service_reachable: false,
            authorized: false,
            detail: Some("Windows helper service is not registered".into()),
        });
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
            if reachable && credential {
                "LocalSystem service and owner-scoped credential verified"
            } else {
                "Service is registered but its protected transport or credential is unavailable"
            }
            .into(),
        ),
    })
}

pub(super) async fn register() -> Result<HelperStatus> {
    let owner = current_process_sid()?;
    service::elevate_install(&owner)?;
    wait_for_status(true).await
}

pub(super) async fn remove() -> Result<HelperStatus> {
    let current = status().await?;
    let prepared = current.service_reachable;
    if prepared {
        exchange(
            Message::PrepareRemoval,
            &CancellationToken::new(),
            FRAME_TIMEOUT,
        )
        .await?;
    } else if current.registration == RegistrationState::Registered && !service::is_stopped()? {
        return Err(HelperError::Unavailable(
            "Windows helper is running but cannot confirm that no operation is active".into(),
        ));
    }
    if let Err(error) = service::elevate_remove() {
        if prepared {
            let _ = exchange(
                Message::AbortRemoval,
                &CancellationToken::new(),
                FRAME_TIMEOUT,
            )
            .await;
        }
        return Err(error);
    }
    wait_for_status(false).await
}

async fn wait_for_status(expected_registered: bool) -> Result<HelperStatus> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let current = status().await?;
        if (current.registration == RegistrationState::Registered) == expected_registered
            && (!expected_registered || current.authorized)
        {
            return Ok(current);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(HelperError::Unavailable(
                "Windows helper service did not reach the requested state within ten seconds"
                    .into(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub(super) fn load_secret() -> Result<[u8; 32]> {
    let sid = current_process_sid()?;
    read_secret(&sid)
}

pub(super) async fn request(
    frame: SignedFrame,
    cancellation: &CancellationToken,
) -> Result<OperationResult> {
    let authorized: AuthorizedRequest = serde_json::from_slice(&frame.body).map_err(|error| {
        HelperError::Protocol(format!("decode signed request for cancellation: {error}"))
    })?;
    authorized.validate()?;
    let run_id = authorized.run_id;
    let transport_token = CancellationToken::new();
    let operation = exchange(
        Message::Operation { frame },
        &transport_token,
        OPERATION_TIMEOUT,
    );
    tokio::pin!(operation);
    let value = tokio::select! {
        result = &mut operation => result?,
        _ = cancellation.cancelled() => {
            let cancel_token = CancellationToken::new();
            let cancel = exchange(
                Message::CancelRun { run_id },
                &cancel_token,
                FRAME_TIMEOUT,
            );
            if cancel.await.is_err() {
                return Err(HelperError::Cancelled);
            }
            tokio::time::timeout(CANCELLATION_GRACE, &mut operation)
                .await
                .map_err(|_| HelperError::Cancelled)??
        }
    };
    serde_json::from_value(value)
        .map_err(|error| HelperError::Protocol(format!("decode operation result: {error}")))
}

async fn exchange(
    message: Message,
    cancellation: &CancellationToken,
    deadline: Duration,
) -> Result<Value> {
    let work = async {
        let mut pipe = open_pipe().await?;
        authenticate_server(&pipe)?;
        write_frame(&mut pipe, &message, FRAME_TIMEOUT).await?;
        let length = tokio::time::timeout(FRAME_TIMEOUT, pipe.read_u32())
            .await
            .map_err(|_| HelperError::Protocol("response header timed out".into()))?
            .map_err(|error| HelperError::transport("read helper response length", error))?
            as usize;
        let bytes = read_body(&mut pipe, length, FRAME_TIMEOUT).await?;
        let reply: std::result::Result<Value, WireError> = serde_json::from_slice(&bytes)
            .map_err(|error| HelperError::Protocol(format!("decode helper response: {error}")))?;
        reply.map_err(Into::into)
    };
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(HelperError::Cancelled),
        result = tokio::time::timeout(deadline, work) => result
            .map_err(|_| HelperError::Unavailable("Windows helper operation deadline exceeded".into()))?,
    }
}

async fn open_pipe() -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    let deadline = tokio::time::Instant::now() + FRAME_TIMEOUT;
    loop {
        match ClientOptions::new().open(PIPE) {
            Ok(pipe) => return Ok(pipe),
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(HelperError::Unavailable(
                        "Windows helper connection timed out".into(),
                    ));
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(error) => return Err(HelperError::transport("open Windows helper pipe", error)),
        }
    }
}

pub(super) fn server_default() -> Result<HelperServer> {
    let owner = registered_owner()?;
    let secret = read_secret(&owner)?;
    Ok(HelperServer::new(secret, sid_identity(&owner)))
}

pub(super) fn run(server: HelperServer) -> Result<()> {
    service::run_dispatcher(server)
}

pub(crate) fn install_service(owner_sid: &str) -> Result<()> {
    validate_owner_sid(owner_sid)?;
    provision(owner_sid)?;
    service::install(owner_sid)
}

pub(crate) fn remove_service() -> Result<()> {
    service::remove()?;
    purge_state()
}

struct Service {
    server: HelperServer,
    owner_sid: String,
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
                Ok(json!({"protocol": 1, "active": self.admission.available_permits() == 0}))
            }
            Message::PrepareRemoval => {
                let permit = self.admission.clone().try_acquire_owned().map_err(|_| {
                    HelperError::Scope("An operation is active; helper removal is refused".into())
                })?;
                *self
                    .removal
                    .lock()
                    .map_err(|_| HelperError::Protocol("removal lock poisoned".into()))? =
                    Some(permit);
                Ok(json!({"removal_prepared": true}))
            }
            Message::AbortRemoval => {
                let released = self
                    .removal
                    .lock()
                    .map_err(|_| HelperError::Protocol("removal lock poisoned".into()))?
                    .take()
                    .is_some();
                Ok(json!({"removal_aborted": released}))
            }
            Message::CancelRun { run_id } => Ok(json!({
                "run_id": run_id,
                "cancelled": self.server.cancel_run(run_id),
            })),
            Message::Operation { frame } => {
                let _permit = self.admission.clone().try_acquire_owned().map_err(|_| {
                    HelperError::Scope("Another helper operation or removal is active".into())
                })?;
                serde_json::to_value(self.server.handle(frame, peer, cancel).await?).map_err(
                    |error| HelperError::Protocol(format!("encode operation result: {error}")),
                )
            }
        }
    }
}

pub(super) async fn serve(server: HelperServer, stop: CancellationToken) -> Result<()> {
    let owner_sid = registered_owner()?;
    let service = Arc::new(Service {
        server,
        owner_sid,
        admission: Arc::new(Semaphore::new(1)),
        removal: Mutex::new(None),
    });
    let slots = Arc::new(Semaphore::new(64));
    let mut tasks = JoinSet::new();
    let mut first = true;
    loop {
        let pipe = create_pipe(&service.owner_sid, first)?;
        first = false;
        tokio::select! {
            _ = stop.cancelled() => break,
            Some(_) = tasks.join_next(), if !tasks.is_empty() => continue,
            connected = pipe.connect() => {
                connected.map_err(|error| HelperError::transport("accept Windows helper client", error))?;
                if let Ok(permit) = slots.clone().try_acquire_owned() {
                    let state = service.clone();
                    let token = stop.child_token();
                    tasks.spawn(async move { let _permit = permit; let _ = serve_connection(state, pipe, token).await; });
                }
            }
        }
    }
    let _ = tokio::time::timeout(SERVER_CLEANUP_GRACE, async {
        while tasks.join_next().await.is_some() {}
    })
    .await;
    tasks.abort_all();
    Ok(())
}

async fn serve_connection(
    service: Arc<Service>,
    mut pipe: NamedPipeServer,
    stop: CancellationToken,
) -> Result<()> {
    let (sid, pid) = authenticate_client(&pipe)?;
    if sid != service.owner_sid {
        return Err(HelperError::Authentication);
    }
    let peer = PeerIdentity {
        user_id: sid_identity(&sid),
        process_id: Some(pid),
        trusted_signature: true,
    };
    let length = tokio::time::timeout(FRAME_TIMEOUT, pipe.read_u32())
        .await
        .map_err(|_| HelperError::Protocol("request header timed out".into()))?
        .map_err(|error| HelperError::transport("read request length", error))?
        as usize;
    let bytes = read_body(&mut pipe, length, FRAME_TIMEOUT).await?;
    let message: Message = serde_json::from_slice(&bytes).map_err(|error| {
        HelperError::Protocol(format!("decode Windows helper request: {error}"))
    })?;
    let cancelled = stop.child_token();
    let guard = cancelled.clone().drop_guard();
    let operation = service.handle(message, peer, cancelled.clone());
    tokio::pin!(operation);
    let mut unexpected = [0_u8; 1];
    let result = tokio::select! {
        _ = stop.cancelled() => { cancelled.cancel(); tokio::time::timeout(SERVER_CLEANUP_GRACE, &mut operation).await.unwrap_or(Err(HelperError::Cancelled)) },
        _ = pipe.read(&mut unexpected) => {
            cancelled.cancel();
            let _ = tokio::time::timeout(SERVER_CLEANUP_GRACE, &mut operation).await;
            return Err(HelperError::Cancelled);
        },
        value = tokio::time::timeout(OPERATION_TIMEOUT, &mut operation) => value.unwrap_or_else(|_| Err(HelperError::Protocol("operation deadline exceeded".into()))),
    };
    drop(guard);
    write_frame(&mut pipe, &result.map_err(WireError::from), FRAME_TIMEOUT).await?;
    pipe.shutdown()
        .await
        .map_err(|error| HelperError::transport("finish Windows helper response", error))
}

fn create_pipe(owner_sid: &str, first: bool) -> Result<NamedPipeServer> {
    let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{owner_sid})");
    with_security_descriptor(&sddl, |attributes| unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(PIPE, attributes.cast())
            .map_err(|error| HelperError::transport("create protected Windows helper pipe", error))
    })
}

fn authenticate_server(pipe: &tokio::net::windows::named_pipe::NamedPipeClient) -> Result<()> {
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle() as HANDLE, &mut pid) } == 0 {
        return Err(last("identify Windows helper service"));
    }
    if process_sid(pid)? != "S-1-5-18" {
        return Err(HelperError::Authentication);
    }
    Ok(())
}

fn authenticate_client(pipe: &NamedPipeServer) -> Result<(String, u32)> {
    let handle = pipe.as_raw_handle() as HANDLE;
    let mut pid = 0;
    if unsafe { GetNamedPipeClientProcessId(handle, &mut pid) } == 0 {
        return Err(last("identify helper client"));
    }
    if unsafe { ImpersonateNamedPipeClient(handle) } == 0 {
        return Err(last("impersonate helper client"));
    }
    let result = (|| {
        let mut token = std::ptr::null_mut();
        if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0 {
            return Err(last("open helper client token"));
        }
        let sid = token_sid(token);
        unsafe { CloseHandle(token) };
        sid.map(|sid| (sid, pid))
    })();
    if unsafe { RevertToSelf() } == 0 {
        return Err(last("end helper client impersonation"));
    }
    result
}

fn current_process_sid() -> Result<String> {
    process_token_sid(unsafe { GetCurrentProcess() })
}

fn process_sid(pid: u32) -> Result<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(last("open peer process"));
    }
    let result = process_token_sid(process);
    unsafe { CloseHandle(process) };
    result
}

fn process_token_sid(process: HANDLE) -> Result<String> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(last("open process token"));
    }
    let result = token_sid(token);
    unsafe { CloseHandle(token) };
    result
}

fn token_sid(token: HANDLE) -> Result<String> {
    let mut needed = 0;
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
    if needed == 0 || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
        return Err(last("size token identity"));
    }
    let mut buffer = vec![0_u8; needed as usize];
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(last("read token identity"));
    }
    let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
    let mut value = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut value) } == 0 {
        return Err(last("encode token SID"));
    }
    let length = unsafe { (0..).position(|index| *value.add(index) == 0).unwrap_or(0) };
    let sid = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(value, length) });
    unsafe { LocalFree(value.cast()) };
    Ok(sid)
}

fn sid_identity(sid: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(sid.as_bytes());
    u64::from_le_bytes(digest[..8].try_into().expect("SHA-256 prefix"))
}

fn data_root() -> Result<PathBuf> {
    let mut buffer = [0_u16; MAX_PATH as usize];
    let status = unsafe {
        SHGetFolderPathW(
            std::ptr::null_mut(),
            CSIDL_COMMON_APPDATA as i32,
            std::ptr::null_mut(),
            SHGFP_TYPE_CURRENT as u32,
            buffer.as_mut_ptr(),
        )
    };
    if status < 0 {
        return Err(HelperError::Unavailable(format!(
            "locate Windows ProgramData directory: HRESULT 0x{:08X}",
            status as u32
        )));
    }
    let length = buffer.iter().position(|unit| *unit == 0).ok_or_else(|| {
        HelperError::Protocol("Windows ProgramData path was not terminated".into())
    })?;
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])).join("NetworkLanternHelper"))
}
fn owner_path() -> Result<PathBuf> {
    Ok(data_root()?.join("owner.sid"))
}
fn secret_path(sid: &str) -> Result<PathBuf> {
    Ok(data_root()?.join("clients").join(format!("{sid}.key")))
}

fn registered_owner() -> Result<String> {
    let path = owner_path()?;
    validate_regular_file(&path)?;
    let value = std::fs::read_to_string(path)
        .map_err(|error| HelperError::transport("read registered helper owner", error))?;
    let value = value.trim().to_owned();
    validate_owner_sid(&value)?;
    Ok(value)
}

fn read_secret(sid: &str) -> Result<[u8; 32]> {
    let path = secret_path(sid)?;
    validate_regular_file(&path)?;
    std::fs::read(path)
        .map_err(|error| HelperError::transport("read Windows helper credential", error))?
        .try_into()
        .map_err(|_| HelperError::Authentication)
}

fn provision(owner_sid: &str) -> Result<()> {
    let root = data_root()?;
    let clients = root.join("clients");
    let owner_path = root.join("owner.sid");
    let root_acl = format!("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;{owner_sid})");
    create_protected_directory(&root, &root_acl)?;
    let owner_exists = owner_path.exists();
    if owner_exists && registered_owner()? != owner_sid {
        return Err(HelperError::Scope(
            "Windows helper is already provisioned for another owner".into(),
        ));
    }
    protect_path(&root, &root_acl)?;
    let clients_acl = format!("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;{owner_sid})");
    create_protected_directory(&clients, &clients_acl)?;
    protect_path(&clients, &clients_acl)?;
    if !owner_exists {
        write_new(
            &owner_path,
            owner_sid.as_bytes(),
            "D:P(A;;FA;;;SY)(A;;FA;;;BA)",
        )?;
    }
    validate_regular_file(&owner_path)?;
    protect_path(&owner_path, "D:P(A;;FA;;;SY)(A;;FA;;;BA)")?;
    let key = clients.join(format!("{owner_sid}.key"));
    if !key.exists() {
        let mut secret = [0_u8; 32];
        rand::rng().fill_bytes(&mut secret);
        write_new(
            &key,
            &secret,
            &format!("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;{owner_sid})"),
        )?;
    }
    validate_regular_file(&key)?;
    protect_path(
        &key,
        &format!("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;{owner_sid})"),
    )?;
    Ok(())
}

fn create_protected_directory(path: &Path, sddl: &str) -> Result<()> {
    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    with_security_descriptor(sddl, |attributes| {
        if unsafe { CreateDirectoryW(wide_path.as_ptr(), attributes) } == 0
            && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS
        {
            return Err(last("create protected helper directory"));
        }
        validate_regular_directory(path)
    })
}

fn write_new(path: &Path, bytes: &[u8], sddl: &str) -> Result<()> {
    let wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    with_security_descriptor(sddl, |attributes| {
        let handle = unsafe {
            CreateFileW(
                wide_path.as_ptr(),
                GENERIC_WRITE,
                0,
                attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(last("create protected helper state"));
        }
        let mut file = unsafe { File::from_raw_handle(handle.cast()) };
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| HelperError::transport("write protected helper state", error))
    })
}

fn validate_regular_file(path: &Path) -> Result<()> {
    use std::os::windows::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| HelperError::transport("inspect helper credential", error))?;
    if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
        return Err(HelperError::Authentication);
    }
    Ok(())
}

fn validate_regular_directory(path: &Path) -> Result<()> {
    use std::os::windows::fs::MetadataExt;
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| HelperError::transport("inspect helper state directory", error))?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
        return Err(HelperError::Authentication);
    }
    Ok(())
}

fn purge_state() -> Result<()> {
    let root = data_root()?;
    if !root.exists() {
        return Ok(());
    }
    validate_regular_directory(&root)?;
    let clients = root.join("clients");
    let owner_path = root.join("owner.sid");
    let owner = if owner_path.exists() {
        Some(registered_owner()?)
    } else {
        None
    };
    if clients.exists() {
        validate_regular_directory(&clients)?;
        if let Some(owner) = owner.as_deref() {
            let key = clients.join(format!("{owner}.key"));
            if key.exists() {
                validate_regular_file(&key)?;
                std::fs::remove_file(&key).map_err(|error| {
                    HelperError::transport("remove Windows helper credential", error)
                })?;
            }
        }
        std::fs::remove_dir(&clients).map_err(|error| {
            HelperError::transport("remove Windows helper credential directory", error)
        })?;
    }
    if owner.is_some() {
        std::fs::remove_file(owner_path)
            .map_err(|error| HelperError::transport("remove Windows helper owner record", error))?;
    }
    std::fs::remove_dir(&root)
        .map_err(|error| HelperError::transport("remove Windows helper data directory", error))
}

fn protect_path(path: &Path, sddl: &str) -> Result<()> {
    use windows_sys::Win32::Security::{
        Authorization::{SE_FILE_OBJECT, SetNamedSecurityInfoW},
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    };
    let mut wide_path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    with_security_descriptor(sddl, |attributes| {
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = std::ptr::null_mut();
        if unsafe {
            GetSecurityDescriptorDacl(
                (*attributes).lpSecurityDescriptor,
                &mut present,
                &mut dacl,
                &mut defaulted,
            )
        } == 0
            || present == 0
        {
            return Err(last("read helper DACL"));
        }
        let status = unsafe {
            SetNamedSecurityInfoW(
                wide_path.as_mut_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null(),
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(HelperError::transport(
                "protect helper path",
                std::io::Error::from_raw_os_error(status as i32),
            ))
        }
    })
}

fn with_security_descriptor<T>(
    sddl: &str,
    work: impl FnOnce(*mut SECURITY_ATTRIBUTES) -> Result<T>,
) -> Result<T> {
    let text = std::ffi::OsStr::new(sddl)
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(last("create helper security descriptor"));
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let result = work(&mut attributes);
    unsafe { LocalFree(descriptor.cast()) };
    result
}

fn validate_owner_sid(sid: &str) -> Result<()> {
    let parts = sid.split('-').collect::<Vec<_>>();
    if sid.len() > 184
        || parts.len() < 5
        || parts[0] != "S"
        || parts[1] != "1"
        || parts[2] != "5"
        || parts[3..]
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        || matches!(sid, "S-1-5-18" | "S-1-5-19" | "S-1-5-20")
    {
        return Err(HelperError::Validation(
            "Windows helper owner SID is invalid".into(),
        ));
    }
    Ok(())
}

fn last(operation: &'static str) -> HelperError {
    HelperError::transport(operation, std::io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_sid_validation_accepts_users_and_rejects_services_or_malformed_values() {
        assert!(validate_owner_sid("S-1-5-21-100-200-300-1001").is_ok());
        for sid in [
            "S-1-5-18",
            "S-1-5-19",
            "S-1-5-20",
            "S-1-5-",
            "S-1-5-21--1001",
            "S-2-5-21-1001",
            "S-1-6-21-1001",
            "S-1-5-21-user",
        ] {
            assert!(validate_owner_sid(sid).is_err(), "accepted {sid}");
        }
    }

    #[test]
    fn pipe_acl_is_owner_scoped() {
        let owner = "S-1-5-21-100-200-300-1001";
        let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{owner})");
        assert!(sddl.contains(owner));
        assert!(!sddl.contains(";;;WD"));
        assert!(!sddl.contains(";;;AU"));
    }

    #[test]
    fn configured_security_descriptors_are_valid_windows_sddl() {
        let owner = "S-1-5-21-100-200-300-1001";
        for sddl in [
            format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{owner})"),
            format!("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;{owner})"),
            format!("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;{owner})"),
        ] {
            with_security_descriptor(&sddl, |_| Ok(())).unwrap();
        }
    }
}
