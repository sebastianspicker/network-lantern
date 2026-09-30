//! Authenticated platform helper transport for privileged tuning operations.

mod client;
mod error;
mod platform;
mod protocol;
mod server;

pub use client::HelperClient;
pub use error::{HelperError, Result};
pub use platform::{HelperStatus, RegistrationState};
pub use protocol::{
    AuthorizedRequest, HelperOperation, OperationResult, ProbeOperation, ProbeRequest, ProbeResult,
    ThroughputLimits, reviewed_hash,
};
pub use server::HelperServer;

/// Removes the credential for the systemd-scoped Linux helper instance.
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub fn cleanup_platform_credentials() -> Result<()> {
    platform::cleanup_credentials()
}
