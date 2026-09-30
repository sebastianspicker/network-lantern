//! Native iperf3 throughput planning and execution.
//!
//! Planning is pure and lazy. Network access happens only through
//! [`Iperf3Client::run_test`] or [`Iperf3Client::run_plan`].

mod config;
mod error;
mod plan;
mod protocol;

pub use config::{
    Direction, DscpClass, IpVersion, MAX_UDP_RATE_BPS, Protocol, RetryPolicy, ServerCapabilities,
    SuiteConfig,
};
pub use error::{Result, ThroughputError};
pub use plan::{PlanRun, TestPhase, TestPlanIter, TestResult, TestSpec, ThroughputPlan, plan};
pub use protocol::Iperf3Client;

mod preflight;
pub use preflight::preflight_check;
