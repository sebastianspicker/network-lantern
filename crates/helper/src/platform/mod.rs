use crate::{
    Result,
    protocol::{OperationResult, SignedFrame},
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[cfg(any(target_os = "linux", windows, test))]
mod framing;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrationState {
    Registered,
    RequiresApproval,
    NotRegistered,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperStatus {
    pub registration: RegistrationState,
    pub service_reachable: bool,
    pub authorized: bool,
    pub detail: Option<String>,
}

pub(crate) async fn status() -> Result<HelperStatus> {
    #[cfg(target_os = "linux")]
    return linux::status().await;
    #[cfg(target_os = "macos")]
    return macos::status().await;
    #[cfg(windows)]
    return windows::status().await;
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    Err(crate::HelperError::Unavailable(
        "no helper transport for this platform".into(),
    ))
}

pub(crate) async fn register() -> Result<HelperStatus> {
    #[cfg(target_os = "linux")]
    return linux::register().await;
    #[cfg(target_os = "macos")]
    return macos::register().await;
    #[cfg(windows)]
    return windows::register().await;
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    Err(crate::HelperError::Unavailable(
        "no helper registration for this platform".into(),
    ))
}

pub(crate) async fn remove() -> Result<HelperStatus> {
    #[cfg(target_os = "linux")]
    return linux::remove().await;
    #[cfg(target_os = "macos")]
    return macos::remove().await;
    #[cfg(windows)]
    return windows::remove().await;
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    Err(crate::HelperError::Unavailable(
        "no helper removal for this platform".into(),
    ))
}

pub(crate) async fn request(
    frame: SignedFrame,
    cancellation: &CancellationToken,
) -> Result<OperationResult> {
    #[cfg(target_os = "linux")]
    return linux::request(frame, cancellation).await;
    #[cfg(target_os = "macos")]
    return macos::request(frame, cancellation).await;
    #[cfg(windows)]
    return windows::request(frame, cancellation).await;
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (frame, cancellation);
        Err(crate::HelperError::Unavailable(
            "no helper transport for this platform".into(),
        ))
    }
}

pub(crate) fn load_secret() -> Result<[u8; 32]> {
    #[cfg(target_os = "linux")]
    return linux::load_secret();
    #[cfg(target_os = "macos")]
    return macos::load_secret();
    #[cfg(windows)]
    return windows::load_secret();
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    Err(crate::HelperError::Unavailable(
        "no helper credential store for this platform".into(),
    ))
}

#[cfg(target_os = "linux")]
pub(crate) fn cleanup_credentials() -> Result<()> {
    linux::cleanup_credentials()
}

pub(crate) fn server_default() -> Result<crate::HelperServer> {
    #[cfg(target_os = "linux")]
    return linux::server_default();
    #[cfg(target_os = "macos")]
    return macos::server_default();
    #[cfg(windows)]
    return windows::server_default();
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    Err(crate::HelperError::Unavailable(
        "no helper service for this platform".into(),
    ))
}

pub(crate) fn run(server: crate::HelperServer) -> Result<()> {
    #[cfg(target_os = "linux")]
    return linux::run(server);
    #[cfg(target_os = "macos")]
    return macos::run(server);
    #[cfg(windows)]
    return windows::run(server);
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = server;
        Err(crate::HelperError::Unavailable(
            "no helper service for this platform".into(),
        ))
    }
}

#[cfg(windows)]
pub(crate) fn install_service(owner_sid: &str) -> Result<()> {
    windows::install_service(owner_sid)
}

#[cfg(windows)]
pub(crate) fn remove_service() -> Result<()> {
    windows::remove_service()
}
