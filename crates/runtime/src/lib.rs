//! Typed orchestration and data compatibility shared by CLI and desktop.
mod application;
pub mod config;
mod errors;
mod execution;
pub mod manager;
mod native;
pub mod path_config;
pub mod profiles;
pub mod reports;
pub mod workflow;
pub use application::{doctor, helper_operation, plan_request};
pub use execution::{
    execute_prepared, execute_request, execute_request_with_exit_code, reserve_request,
};
pub use profiles::profile_parameters;
pub use reports::list_runs;

pub mod thresholds;

mod local_config;
pub use local_config::prepare_local_request;
