//! Typed orchestration and data compatibility shared by CLI and desktop.
mod application;
pub mod config;
pub mod manager;
mod native;
pub mod path_config;
pub mod profiles;
pub mod reports;
pub mod workflow;
pub use application::{
    doctor, execute_prepared, execute_request, execute_request_with_exit_code, helper_operation,
    list_runs, plan_request, profile_parameters, reserve_request,
};

pub mod thresholds;

mod local_config;
pub use local_config::prepare_local_request;
