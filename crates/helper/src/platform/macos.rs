//! SMAppService registration and authenticated XPC transport; no shell or launchctl calls.
mod credentials;
mod ffi;
mod identity;
mod transport;
use super::{HelperStatus, RegistrationState};
use crate::{
    HelperError, HelperServer, Result,
    protocol::{OperationResult, SignedFrame},
};
use objc2_foundation::NSString;
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use std::{ffi::CStr, time::Duration};
use tokio_util::sync::CancellationToken;
const APP_ID: &str = "dev.network-lantern.desktop";
const CLI_ID: &str = "dev.network-lantern.cli";
const SERVICE: &CStr = c"dev.network-lantern.helper";
const PLIST: &str = "dev.network-lantern.helper.plist";
fn service() -> objc2::rc::Retained<SMAppService> {
    unsafe { SMAppService::daemonServiceWithPlistName(&NSString::from_str(PLIST)) }
}
fn bundled() -> Result<()> {
    let exe = std::env::current_exe()
        .map_err(|e| HelperError::transport("locate application bundle", e))?;
    let contents = exe
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| {
            HelperError::Unavailable("Helper registration requires an application bundle".into())
        })?;
    if contents.file_name().is_none_or(|name| name != "Contents")
        || !contents.join("Library/LaunchDaemons").join(PLIST).is_file()
    {
        return Err(HelperError::Unavailable("Helper registration requires the signed app bundle with its LaunchDaemon and helper deployment assets".into()));
    }
    identity::peer_requirement(APP_ID)?;
    Ok(())
}
pub(super) async fn status() -> Result<HelperStatus> {
    if let Err(error) = bundled() {
        return Ok(HelperStatus {
            registration: RegistrationState::Unavailable,
            service_reachable: false,
            authorized: false,
            detail: Some(error.to_string()),
        });
    }
    let current = unsafe { service().status() };
    let registration = match current {
        SMAppServiceStatus::Enabled => RegistrationState::Registered,
        SMAppServiceStatus::RequiresApproval => RegistrationState::RequiresApproval,
        SMAppServiceStatus::NotRegistered => RegistrationState::NotRegistered,
        _ => RegistrationState::Unavailable,
    };
    let reachable = if registration == RegistrationState::Registered {
        transport::exchange(
            transport::Message::Status,
            &CancellationToken::new(),
            Duration::from_secs(2),
        )
        .await
        .is_ok()
    } else {
        false
    };
    let authorized = reachable && load_secret().is_ok();
    Ok(HelperStatus{registration,service_reachable:reachable,authorized,detail:Some(match registration {
        RegistrationState::RequiresApproval=>"Approve Network Lantern in System Settings > General > Login Items, then register again to finish the authenticated setup",
        RegistrationState::Registered if !reachable=>"Registered service did not complete an authenticated XPC status request",
        RegistrationState::Registered if !authorized=>"Register again to provision this user's credential over authenticated XPC",
        RegistrationState::Registered=>"Registered helper and application code identities verified",
        RegistrationState::NotRegistered=>"Helper is not registered",
        RegistrationState::Unavailable=>"SMAppService could not find the bundled helper",
    }.into())})
}
pub(super) async fn register() -> Result<HelperStatus> {
    bundled()?;
    unsafe { service().registerAndReturnError() }
        .map_err(|e| HelperError::Authorization(e.to_string()))?;
    if unsafe { service().status() } == SMAppServiceStatus::Enabled {
        transport::exchange(
            transport::Message::Bootstrap,
            &CancellationToken::new(),
            Duration::from_secs(10),
        )
        .await?;
    }
    status().await
}
pub(super) async fn remove() -> Result<HelperStatus> {
    bundled()?;
    let prepared = unsafe { service().status() } == SMAppServiceStatus::Enabled;
    if prepared {
        transport::exchange(
            transport::Message::PrepareRemoval,
            &CancellationToken::new(),
            Duration::from_secs(5),
        )
        .await?;
    }
    let unregister = unsafe { service().unregisterAndReturnError() }
        .map_err(|e| HelperError::Authorization(e.to_string()));
    if let Err(error) = unregister {
        if prepared {
            let _ = transport::exchange(
                transport::Message::AbortRemoval,
                &CancellationToken::new(),
                Duration::from_secs(5),
            )
            .await;
        }
        return Err(error);
    }
    let result = status().await;
    if result.is_err() && prepared {
        let _ = transport::exchange(
            transport::Message::AbortRemoval,
            &CancellationToken::new(),
            Duration::from_secs(5),
        )
        .await;
    }
    result
}
pub(super) fn load_secret() -> Result<[u8; 32]> {
    credentials::read(unsafe { libc::geteuid() })
}
pub(super) async fn request(
    frame: SignedFrame,
    cancel: &CancellationToken,
) -> Result<OperationResult> {
    let response = transport::exchange(
        transport::Message::Operation { frame },
        cancel,
        Duration::from_secs(86_400),
    )
    .await?;
    serde_json::from_value(response).map_err(|e| HelperError::Protocol(e.to_string()))
}
pub(super) fn server_default() -> Result<HelperServer> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(HelperError::Authorization(
            "The helper must be launched by SMAppService as root".into(),
        ));
    }
    identity::peer_requirement(APP_ID)?;
    // Service context cannot authenticate any UID. serve() creates persistent per-user
    // dispatchers only after the kernel verifies the connecting application's signature.
    Ok(HelperServer::new(rand::random(), u64::MAX))
}
pub(super) fn run(_service_context: HelperServer) -> Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| HelperError::transport("start helper runtime", e))?
        .block_on(transport::serve())
}
