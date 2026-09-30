use serde::{Deserialize, Serialize};

pub const MAX_UDP_RATE_BPS: u64 = 10_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Tcp,
    Udp,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Tx,
    Rx,
    Bidirectional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DscpClass {
    CS0,
    CS1,
    CS2,
    CS3,
    CS4,
    AF11,
    AF12,
    AF13,
    AF21,
    AF22,
    AF23,
    AF31,
    AF32,
    AF33,
    AF41,
    AF42,
    AF43,
    CS5,
    CS6,
    CS7,
    EF,
}

impl DscpClass {
    pub const fn tos(self) -> u8 {
        match self {
            Self::CS0 => 0,
            Self::CS1 => 32,
            Self::CS2 => 64,
            Self::CS3 => 96,
            Self::CS4 => 128,
            Self::AF11 => 40,
            Self::AF12 => 48,
            Self::AF13 => 56,
            Self::AF21 => 72,
            Self::AF22 => 80,
            Self::AF23 => 88,
            Self::AF31 => 104,
            Self::AF32 => 112,
            Self::AF33 => 120,
            Self::AF41 => 136,
            Self::AF42 => 144,
            Self::AF43 => 152,
            Self::CS5 => 160,
            Self::CS6 => 192,
            Self::CS7 => 224,
            Self::EF => 184,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpVersion {
    Auto,
    Ipv4,
    Ipv6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_retries: u8,
    pub backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 0,
            backoff_ms: 2_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub bidirectional: bool,
}

impl Default for ServerCapabilities {
    fn default() -> Self {
        Self {
            bidirectional: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuiteConfig {
    pub target: String,
    pub skip_reachability_check: bool,
    pub disable_mtu_probe: bool,
    pub mtu_sizes: Vec<u16>,
    pub port: u16,
    pub duration_secs: u64,
    pub omit_secs: u64,
    pub protocol: Protocol,
    pub single_test: bool,
    pub udp_start_bps: u64,
    pub udp_max_bps: u64,
    pub udp_step_bps: u64,
    pub udp_loss_threshold_pct: f64,
    pub tcp_streams: Vec<u16>,
    pub tcp_windows_bytes: Vec<Option<u32>>,
    pub dscp_classes: Vec<DscpClass>,
    pub ip_version: IpVersion,
    pub retry: RetryPolicy,
    pub max_total_tests: Option<u64>,
    pub connect_timeout_ms: u64,
    pub control_timeout_ms: u64,
    pub max_control_json_bytes: usize,
}

impl Default for SuiteConfig {
    fn default() -> Self {
        Self {
            target: String::new(),
            skip_reachability_check: false,
            disable_mtu_probe: false,
            mtu_sizes: vec![1400, 1472, 1600],
            port: 5201,
            duration_secs: 10,
            omit_secs: 1,
            protocol: Protocol::Both,
            single_test: false,
            udp_start_bps: 1_000_000,
            udp_max_bps: 1_000_000_000,
            udp_step_bps: 10_000_000,
            udp_loss_threshold_pct: 5.0,
            tcp_streams: vec![1, 4, 8],
            tcp_windows_bytes: vec![None, Some(128 * 1024), Some(256 * 1024)],
            dscp_classes: vec![
                DscpClass::CS0,
                DscpClass::AF11,
                DscpClass::CS5,
                DscpClass::EF,
                DscpClass::AF41,
            ],
            ip_version: IpVersion::Auto,
            retry: RetryPolicy::default(),
            max_total_tests: None,
            connect_timeout_ms: 60_000,
            control_timeout_ms: 60_000,
            max_control_json_bytes: 1024 * 1024,
        }
    }
}
