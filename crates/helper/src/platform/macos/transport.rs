use super::{APP_ID, SERVICE, credentials, ffi::*, identity};
use crate::{
    HelperError, HelperServer, Result,
    protocol::{MAX_FRAME_BYTES, SignedFrame, WireError},
    server::PeerIdentity,
};
use block2::RcBlock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    ptr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, oneshot};
use tokio_util::sync::CancellationToken;
const CANCEL_RESULT_GRACE: Duration = Duration::from_secs(10);
const CONNECTION_CLEANUP_TIMEOUT: Duration = Duration::from_secs(12);
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    Status,
    Bootstrap,
    PrepareRemoval,
    AbortRemoval,
    CancelRun { run_id: uuid::Uuid },
    Operation { frame: SignedFrame },
}
fn unavailable(message: &str) -> HelperError {
    HelperError::Unavailable(message.into())
}
fn payload(object: Object) -> Result<Vec<u8>> {
    if object.is_null()
        || unsafe { xpc_get_type(object) != std::ptr::addr_of!(_xpc_type_dictionary).cast() }
    {
        return Err(unavailable(
            "XPC service is unavailable or its code identity was rejected",
        ));
    }
    let mut length = 0;
    let bytes = unsafe { xpc_dictionary_get_data(object, c"payload".as_ptr(), &mut length) };
    if bytes.is_null() || length == 0 || length > MAX_FRAME_BYTES {
        return Err(HelperError::Protocol(
            "Invalid or oversized XPC message".into(),
        ));
    }
    Ok(unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), length) }.to_vec())
}
pub async fn exchange(
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
        result = &mut response => result,
        _ = cancel.cancelled() => {
            if let Some(run_id) = run_id {
                let _acknowledged = roundtrip(Message::CancelRun { run_id }, Duration::from_secs(5)).await?;
                return tokio::time::timeout(CANCEL_RESULT_GRACE, &mut response).await
                    .unwrap_or(Err(HelperError::Cancelled));
            }
            Err(HelperError::Cancelled)
        }
    }
}
async fn roundtrip(message: Message, deadline: Duration) -> Result<Value> {
    let (connection, receiver) = begin_exchange(message)?;
    let bytes = tokio::time::timeout(deadline, receiver)
        .await
        .map_err(|_| unavailable("XPC response deadline exceeded"))?
        .map_err(|_| unavailable("XPC reply was disconnected"))??;
    drop(connection);
    let response: std::result::Result<Value, WireError> =
        serde_json::from_slice(&bytes).map_err(|e| HelperError::Protocol(e.to_string()))?;
    response.map_err(Into::into)
}

fn begin_exchange(message: Message) -> Result<(Connection, oneshot::Receiver<Result<Vec<u8>>>)> {
    let requirement = identity::peer_requirement(SERVICE.to_str().unwrap())?;
    let raw = unsafe { xpc_connection_create_mach_service(SERVICE.as_ptr(), ptr::null_mut(), 2) };
    if raw.is_null() {
        return Err(unavailable("Unable to create privileged XPC connection"));
    }
    let connection = Connection(Xpc(raw));
    if unsafe { xpc_connection_set_peer_code_signing_requirement(raw, requirement.as_ptr()) } != 0 {
        return Err(HelperError::Authentication);
    }
    let event = RcBlock::new(|_: Object| {});
    unsafe {
        xpc_connection_set_event_handler(raw, &event);
        xpc_connection_resume(raw);
    }
    let bytes = serde_json::to_vec(&message).map_err(|e| HelperError::Protocol(e.to_string()))?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(HelperError::Protocol("XPC request exceeds 1 MiB".into()));
    }
    let object = Xpc(unsafe { xpc_dictionary_create(ptr::null(), ptr::null(), 0) });
    let (sender, receiver) = oneshot::channel();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let reply = RcBlock::new(move |response: Object| {
        if let Ok(mut sender) = sender.lock()
            && let Some(sender) = sender.take()
        {
            let _ = sender.send(payload(response));
        }
    });
    unsafe {
        xpc_dictionary_set_data(
            object.0,
            c"payload".as_ptr(),
            bytes.as_ptr().cast(),
            bytes.len(),
        );
        xpc_connection_send_message_with_reply(raw, object.0, ptr::null_mut(), &reply);
    }
    Ok((connection, receiver))
}

struct State {
    runtime: tokio::runtime::Handle,
    servers: Mutex<HashMap<u32, HelperServer>>,
    connections: Mutex<HashMap<usize, PeerConnection>>,
    admission: Arc<Semaphore>,
    removal: Mutex<Option<OwnedSemaphorePermit>>,
    requests: AtomicUsize,
    request_idle: Notify,
    shutting_down: AtomicBool,
}

struct PeerConnection {
    _connection: Connection,
    disconnected: CancellationToken,
}
impl Drop for PeerConnection {
    fn drop(&mut self) {
        self.disconnected.cancel();
    }
}

struct RequestGuard(Arc<State>);
impl Drop for RequestGuard {
    fn drop(&mut self) {
        self.0.requests.fetch_sub(1, Ordering::SeqCst);
        self.0.request_idle.notify_waiters();
    }
}

impl State {
    fn server(&self, uid: u32, bootstrap: bool) -> Result<HelperServer> {
        let mut servers = self
            .servers
            .lock()
            .map_err(|_| HelperError::Protocol("Credential state lock poisoned".into()))?;
        if let Some(server) = servers.get(&uid) {
            return Ok(server.clone());
        }
        if servers.len() >= 64 {
            return Err(unavailable("Helper client limit reached"));
        }
        let secret = if bootstrap {
            credentials::provision(uid)?
        } else {
            credentials::read(uid)?
        };
        let server = HelperServer::new(secret, u64::from(uid));
        servers.insert(uid, server.clone());
        Ok(server)
    }
    async fn handle(
        self: Arc<Self>,
        message: Message,
        uid: u32,
        pid: u32,
        disconnected: CancellationToken,
    ) -> Result<Value> {
        match message {
            Message::Status => {
                Ok(json!({"protocol":1,"active":self.admission.available_permits()==0}))
            }
            Message::CancelRun { run_id } => {
                Ok(json!({"cancelled":self.server(uid, false)?.cancel_run(run_id)}))
            }
            Message::Bootstrap => {
                self.server(uid, true)?;
                Ok(json!({"credential_provisioned":true}))
            }
            Message::PrepareRemoval => {
                let mut removal = self
                    .removal
                    .lock()
                    .map_err(|_| HelperError::Protocol("Removal lock poisoned".into()))?;
                if removal.is_none() {
                    let permit = self.admission.clone().try_acquire_owned().map_err(|_| {
                        HelperError::Scope(
                            "Cannot remove helper while an operation is active".into(),
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
                let server = self.server(uid, false)?;
                let result = server
                    .handle(
                        frame,
                        PeerIdentity {
                            user_id: u64::from(uid),
                            process_id: Some(pid),
                            trusted_signature: true,
                        },
                        disconnected,
                    )
                    .await?;
                serde_json::to_value(result).map_err(|e| HelperError::Protocol(e.to_string()))
            }
        }
    }
}
fn finish_reply(state: &Arc<State>, id: usize, peer: &Xpc, message: &Xpc, result: Result<Value>) {
    send_reply(peer, message, result);
    // Wait until the reply has entered XPC's outgoing channel before cancelling.
    // A completed request never depends on the client voluntarily disconnecting.
    let weak = Arc::downgrade(state);
    let barrier = RcBlock::new(move || {
        if let Some(state) = weak.upgrade()
            && let Ok(mut connections) = state.connections.lock()
        {
            connections.remove(&id);
        }
    });
    unsafe { xpc_connection_send_barrier(peer.0, &barrier) };
}
fn send_reply(peer: &Xpc, message: &Xpc, result: Result<Value>) {
    let response = result.map_err(WireError::from);
    let mut bytes = match serde_json::to_vec(&response) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    if bytes.len() > MAX_FRAME_BYTES {
        bytes = serde_json::to_vec(&std::result::Result::<Value, WireError>::Err(
            WireError::from(HelperError::Protocol(
                "Helper response exceeds 1 MiB".into(),
            )),
        ))
        .expect("bounded error serializes");
    }
    let raw = unsafe { xpc_dictionary_create_reply(message.0) };
    if raw.is_null() {
        return;
    }
    let reply = Xpc(raw);
    unsafe {
        xpc_dictionary_set_data(
            reply.0,
            c"payload".as_ptr(),
            bytes.as_ptr().cast(),
            bytes.len(),
        );
        xpc_connection_send_message(peer.0, reply.0);
    }
}
pub async fn serve() -> Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(HelperError::Authorization(
            "LaunchDaemon must run as root".into(),
        ));
    }
    let requirement = identity::peer_requirement(APP_ID)?;
    let raw = unsafe { xpc_connection_create_mach_service(SERVICE.as_ptr(), ptr::null_mut(), 1) };
    if raw.is_null() {
        return Err(unavailable("Mach service must be registered with launchd"));
    }
    let listener = Connection(Xpc(raw));
    if unsafe { xpc_connection_set_peer_code_signing_requirement(raw, requirement.as_ptr()) } != 0 {
        return Err(HelperError::Authentication);
    }
    let state = Arc::new(State {
        runtime: tokio::runtime::Handle::current(),
        servers: Mutex::new(HashMap::new()),
        connections: Mutex::new(HashMap::new()),
        admission: Arc::new(Semaphore::new(1)),
        removal: Mutex::new(None),
        requests: AtomicUsize::new(0),
        request_idle: Notify::new(),
        shutting_down: AtomicBool::new(false),
    });
    let service = state.clone();
    let handler = RcBlock::new(move |peer: Object| {
        if peer.is_null()
            || unsafe { xpc_get_type(peer) != std::ptr::addr_of!(_xpc_type_connection).cast() }
        {
            return;
        }
        if service.shutting_down.load(Ordering::SeqCst) {
            unsafe { xpc_connection_cancel(peer) };
            return;
        }
        let id = peer as usize;
        let disconnected = CancellationToken::new();
        let Ok(mut connections) = service.connections.lock() else {
            unsafe { xpc_connection_cancel(peer) };
            return;
        };
        if connections.len() >= 64 {
            unsafe { xpc_connection_cancel(peer) };
            return;
        }
        let connection = unsafe { Xpc::retain(peer) };
        connections.insert(
            id,
            PeerConnection {
                _connection: Connection(connection),
                disconnected: disconnected.clone(),
            },
        );
        drop(connections);
        let used = Arc::new(AtomicBool::new(false));
        let weak = Arc::downgrade(&service);
        let token = disconnected.clone();
        let once = used.clone();
        let event = RcBlock::new(move |message: Object| {
            let Some(state) = weak.upgrade() else { return };
            if message.is_null()
                || unsafe {
                    xpc_get_type(message) != std::ptr::addr_of!(_xpc_type_dictionary).cast()
                }
            {
                token.cancel();
                if let Ok(mut connections) = state.connections.lock() {
                    connections.remove(&id);
                }
                return;
            }
            let peer = unsafe { Xpc::retain(id as Object) };
            let message = unsafe { Xpc::retain(message) };
            if once.swap(true, Ordering::SeqCst) {
                finish_reply(
                    &state,
                    id,
                    &peer,
                    &message,
                    Err(HelperError::Scope(
                        "One request per connection is allowed".into(),
                    )),
                );
                return;
            }
            if let Err(error) = identity::verify_message(message.0, APP_ID) {
                finish_reply(&state, id, &peer, &message, Err(error));
                return;
            }
            let parsed = payload(message.0).and_then(|bytes| {
                serde_json::from_slice::<Message>(&bytes)
                    .map_err(|e| HelperError::Protocol(e.to_string()))
            });
            let uid = unsafe { xpc_connection_get_euid(peer.0) };
            let pid = unsafe { xpc_connection_get_pid(peer.0) };
            if pid <= 0 {
                finish_reply(
                    &state,
                    id,
                    &peer,
                    &message,
                    Err(HelperError::Authentication),
                );
                return;
            }
            if state.shutting_down.load(Ordering::SeqCst) {
                finish_reply(&state, id, &peer, &message, Err(HelperError::Cancelled));
                return;
            }
            state.requests.fetch_add(1, Ordering::SeqCst);
            if state.shutting_down.load(Ordering::SeqCst) {
                state.requests.fetch_sub(1, Ordering::SeqCst);
                state.request_idle.notify_waiters();
                finish_reply(&state, id, &peer, &message, Err(HelperError::Cancelled));
                return;
            }
            let token = token.clone();
            let runtime = state.runtime.clone();
            runtime.spawn(async move {
                let _request_guard = RequestGuard(state.clone());
                let result = match parsed {
                    Ok(request) => state.clone().handle(request, uid, pid as u32, token).await,
                    Err(error) => Err(error),
                };
                finish_reply(&state, id, &peer, &message, result);
            });
        });
        unsafe {
            xpc_connection_set_event_handler(peer, &event);
            xpc_connection_resume(peer);
        }
        let idle = Arc::downgrade(&service);
        service.runtime.spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if !used.load(Ordering::SeqCst)
                && let Some(state) = idle.upgrade()
                && let Ok(mut connections) = state.connections.lock()
            {
                connections.remove(&id);
            }
        });
    });
    unsafe {
        xpc_connection_set_event_handler(raw, &handler);
        xpc_connection_resume(raw);
    }
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|e| HelperError::transport("install helper shutdown signal", e))?;
    tokio::select! {_ = tokio::signal::ctrl_c()=>(),_ = term.recv()=>()};
    state.shutting_down.store(true, Ordering::SeqCst);
    drop(listener);
    if let Ok(mut connections) = state.connections.lock() {
        connections.clear();
    }
    let _ = tokio::time::timeout(CONNECTION_CLEANUP_TIMEOUT, async {
        loop {
            let idle = state.request_idle.notified();
            if state.requests.load(Ordering::SeqCst) == 0 {
                break;
            }
            idle.await;
        }
    })
    .await;
    Ok(())
}
