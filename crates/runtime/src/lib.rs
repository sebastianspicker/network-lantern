//! Typed orchestration and data compatibility shared by CLI and desktop.
mod application;
mod config;
mod errors;
mod execution;
pub mod manager;
mod native;
mod path_config;
pub mod profiles;
pub mod reports;
mod request;
pub mod run_log;
mod workflow;
pub use application::{doctor, helper_operation, plan_request};
pub use execution::{execute_prepared, execute_request_with_exit_code, reserve_request};
pub use profiles::profile_parameters;
pub use reports::list_runs;

mod thresholds;

mod local_config;
pub use local_config::{add_plan_warnings, prepare_local_request};
