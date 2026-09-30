mod data;
mod metrics;
mod traffic_class;
use data::*;
use metrics::*;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use traffic_class::TrafficClassFlow;

use rand::Rng;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpSocket, TcpStream, UdpSocket, lookup_host};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::config::{Direction, IpVersion, Protocol, RetryPolicy};
use crate::error::{Result, ThroughputError};
use crate::plan::{TestResult, TestSpec};

const COOKIE_SIZE: usize = 37;
const PARAM_EXCHANGE: i8 = 9;
const CREATE_STREAMS: i8 = 10;
const SERVER_TERMINATE: i8 = 11;
const CLIENT_TERMINATE: i8 = 12;
const EXCHANGE_RESULTS: i8 = 13;
const DISPLAY_RESULTS: i8 = 14;
const IPERF_DONE: i8 = 16;
const ACCESS_DENIED: i8 = -1;
const SERVER_ERROR: i8 = -2;
const TEST_START: i8 = 1;
const TEST_RUNNING: i8 = 2;
const TEST_END: i8 = 4;
const UDP_CONNECT_MSG: [u8; 4] = *b"9876";
const UDP_CONNECT_REPLY: [u8; 4] = *b"6789";
const UDP_LEGACY_REPLY_LE: [u8; 4] = 987_654_321_u32.to_le_bytes();
const TCP_BLOCK_SIZE: usize = 128 * 1024;
const UDP_BLOCK_SIZE: usize = 1_460;
pub(crate) const MAX_CLIENT_TIMEOUT_MS: u64 = 300_000;

#[derive(Debug, Clone)]
pub struct Iperf3Client {
    connect_timeout: Duration,
    control_timeout: Duration,
    max_control_json_bytes: usize,
}

impl Iperf3Client {
    pub fn new() -> Self {
        Self {
            connect_timeout: Duration::from_millis(60_000),
            control_timeout: Duration::from_millis(60_000),
            max_control_json_bytes: 1024 * 1024,
        }
    }

    pub fn with_limits(
        connect_timeout_ms: u64,
        control_timeout_ms: u64,
        max_control_json_bytes: usize,
    ) -> Result<Self> {
        if !(1..=MAX_CLIENT_TIMEOUT_MS).contains(&connect_timeout_ms)
            || !(1..=MAX_CLIENT_TIMEOUT_MS).contains(&control_timeout_ms)
            || !(2..=1024 * 1024).contains(&max_control_json_bytes)
        {
            return Err(ThroughputError::Validation(format!(
                "client timeouts must be between 1 and {MAX_CLIENT_TIMEOUT_MS} ms and max_control_json_bytes between 2 and 1048576"
            )));
        }
        Ok(Self {
            connect_timeout: Duration::from_millis(connect_timeout_ms),
            control_timeout: Duration::from_millis(control_timeout_ms),
            max_control_json_bytes,
        })
    }

    pub fn from_suite(config: &crate::SuiteConfig) -> Result<Self> {
        Self::with_limits(
            config.connect_timeout_ms,
            config.control_timeout_ms,
            config.max_control_json_bytes,
        )
    }

    pub fn with_config(config: &crate::SuiteConfig) -> Result<Self> {
        Self::from_suite(config)
    }

    pub async fn run_test(
        &self,
        spec: &TestSpec,
        cancellation: &CancellationToken,
    ) -> Result<TestResult> {
        self.run_test_once(spec, cancellation, 1).await
    }

    pub async fn run_test_with_retry(
        &self,
        spec: &TestSpec,
        retry: RetryPolicy,
        cancellation: &CancellationToken,
    ) -> Result<TestResult> {
        let attempts = retry.max_retries.saturating_add(1);
        let mut last = None;
        for attempt in 1..=attempts {
            match self.run_test_once(spec, cancellation, attempt).await {
                Ok(result) => return Ok(result),
                Err(error) if error.retryable() && attempt < attempts => {
                    last = Some(error);
                    Self::retry_backoff(cancellation, retry.backoff_ms).await?;
                }
                Err(error) if attempt == 1 => return Err(error),
                Err(error) => {
                    return Err(ThroughputError::RetriesExhausted {
                        attempts: attempt,
                        last: Box::new(error),
                    });
                }
            }
        }
        Err(ThroughputError::RetriesExhausted {
            attempts,
            last: Box::new(last.unwrap_or(ThroughputError::Cancelled)),
        })
    }

    async fn run_test_once(
        &self,
        spec: &TestSpec,
        cancellation: &CancellationToken,
        attempt: u8,
    ) -> Result<TestResult> {
        validate_spec(spec)?;
        let started = Instant::now();
        let address = self.resolve(spec, cancellation).await?;
        let (mut control, _control_traffic_class) = self
            .connect_tcp(address, 0, None, cancellation, "control connection")
            .await?;
        control
            .set_nodelay(true)
            .map_err(|error| ThroughputError::io("control socket setup", error))?;
        let cookie = random_cookie();
        self.phase(cancellation, "control cookie", async {
            control
                .write_all(&cookie)
                .await
                .map_err(|error| ThroughputError::io("control cookie", error))
        })
        .await?;
        self.expect_state(&mut control, PARAM_EXCHANGE, cancellation)
            .await?;
        let parameters = parameters(spec);
        self.write_json(&mut control, &parameters, cancellation)
            .await?;
        self.expect_state(&mut control, CREATE_STREAMS, cancellation)
            .await?;

        let mut streams = self
            .create_streams(spec, address, &cookie, cancellation)
            .await?;
        self.expect_state(&mut control, TEST_START, cancellation)
            .await?;
        self.expect_state(&mut control, TEST_RUNNING, cancellation)
            .await?;

        let data_result = self.run_data(spec, &mut streams, cancellation).await;
        if data_result.is_err() {
            let _ = tokio::time::timeout(
                Duration::from_millis(100),
                control.write_all(&[CLIENT_TERMINATE as u8]),
            )
            .await;
        }
        let (outcomes, held_streams) = data_result?;
        self.phase(cancellation, "test end", async {
            control
                .write_all(&[TEST_END as u8])
                .await
                .map_err(|error| ThroughputError::io("test end", error))
        })
        .await?;
        let drain_stop = cancellation.child_token();
        let exchange_stop = drain_stop.clone();
        let exchange = async {
            let result: Result<Value> = async {
                self.expect_state(&mut control, EXCHANGE_RESULTS, cancellation)
                    .await?;
                self.write_json(&mut control, &client_results(&outcomes), cancellation)
                    .await?;
                let server_result = self.read_json(&mut control, cancellation).await?;
                self.expect_state(&mut control, DISPLAY_RESULTS, cancellation)
                    .await?;
                Ok(server_result)
            }
            .await;
            exchange_stop.cancel();
            result
        };
        let (server_result, _held_streams) =
            tokio::join!(exchange, drain_tcp_receivers(held_streams, drain_stop));
        let server_result = server_result?;
        self.phase(cancellation, "completion state", async {
            control
                .write_all(&[IPERF_DONE as u8])
                .await
                .map_err(|error| ThroughputError::io("completion state", error))
        })
        .await?;

        build_result(
            spec.clone(),
            attempt,
            started.elapsed(),
            outcomes,
            server_result,
        )
    }

    async fn resolve(
        &self,
        spec: &TestSpec,
        cancellation: &CancellationToken,
    ) -> Result<SocketAddr> {
        let target = (spec.target.as_str(), spec.port);
        let addresses = self
            .phase(cancellation, "name resolution", async {
                let resolved = lookup_host(target)
                    .await
                    .map_err(|error| ThroughputError::io("name resolution", error))?;
                Ok(resolved.collect::<Vec<_>>())
            })
            .await?;
        addresses
            .into_iter()
            .find(|address| match spec.ip_version {
                IpVersion::Auto => true,
                IpVersion::Ipv4 => address.is_ipv4(),
                IpVersion::Ipv6 => address.is_ipv6(),
            })
            .ok_or_else(|| {
                ThroughputError::Validation(format!(
                    "target '{}' has no address matching {:?}",
                    spec.target, spec.ip_version
                ))
            })
    }

    async fn connect_tcp(
        &self,
        address: SocketAddr,
        tos: u8,
        window: Option<u32>,
        cancellation: &CancellationToken,
        phase: &'static str,
    ) -> Result<(TcpStream, Option<TrafficClassFlow>)> {
        let socket = if address.is_ipv4() {
            TcpSocket::new_v4()
        } else {
            TcpSocket::new_v6()
        }
        .map_err(|error| ThroughputError::io(phase, error))?;
        #[cfg(not(windows))]
        if tos != 0 {
            let result = if address.is_ipv4() {
                socket.set_tos_v4(u32::from(tos))
            } else {
                set_ipv6_traffic_class(&socket, tos)
            };
            result.map_err(|error| ThroughputError::io("DSCP socket setup", error))?;
        }
        if let Some(size) = window {
            socket
                .set_send_buffer_size(size)
                .map_err(|error| ThroughputError::io("TCP window setup", error))?;
            socket
                .set_recv_buffer_size(size)
                .map_err(|error| ThroughputError::io("TCP window setup", error))?;
        }
        tokio::select! {
            _ = cancellation.cancelled() => Err(ThroughputError::Cancelled),
            result = tokio::time::timeout(self.connect_timeout, socket.connect(address)) => {
                match result {
                    Ok(Ok(stream)) => {
                        #[cfg(windows)]
                        let traffic_class = if tos == 0 {
                            None
                        } else {
                            Some(traffic_class::apply(&stream, tos).map_err(|error| {
                                ThroughputError::io("Windows qWAVE DSCP setup", error)
                            })?)
                        };
                        #[cfg(not(windows))]
                        let traffic_class = None;
                        Ok((stream, traffic_class))
                    },
                    Ok(Err(error)) => Err(ThroughputError::io(phase, error)),
                    Err(_) => Err(ThroughputError::Timeout {
                        phase,
                        timeout_ms: millis(self.connect_timeout),
                    }),
                }
            }
        }
    }

    async fn create_streams(
        &self,
        spec: &TestSpec,
        address: SocketAddr,
        cookie: &[u8; COOKIE_SIZE],
        cancellation: &CancellationToken,
    ) -> Result<Vec<DataStream>> {
        let mut streams = Vec::new();
        let roles: &[bool] = match spec.direction {
            Direction::Tx => &[true],
            Direction::Rx => &[false],
            Direction::Bidirectional => &[true, false],
        };
        for &sender in roles {
            for _ in 0..spec.streams {
                let stream = match spec.protocol {
                    Protocol::Tcp => {
                        let (mut socket, traffic_class) = self
                            .connect_tcp(
                                address,
                                spec.tos,
                                spec.tcp_window_bytes,
                                cancellation,
                                "TCP data connection",
                            )
                            .await?;
                        self.phase(cancellation, "TCP stream cookie", async {
                            socket
                                .write_all(cookie)
                                .await
                                .map_err(|error| ThroughputError::io("TCP stream cookie", error))
                        })
                        .await?;
                        DataStream::Tcp {
                            traffic_class,
                            socket,
                            sender,
                        }
                    }
                    Protocol::Udp => {
                        let bind_address = if address.is_ipv4() {
                            "0.0.0.0:0"
                        } else {
                            "[::]:0"
                        };
                        let socket = UdpSocket::bind(bind_address)
                            .await
                            .map_err(|error| ThroughputError::io("UDP bind", error))?;
                        #[cfg(not(windows))]
                        if spec.tos != 0 {
                            let result = if address.is_ipv4() {
                                socket.set_tos_v4(u32::from(spec.tos))
                            } else {
                                set_ipv6_traffic_class(&socket, spec.tos)
                            };
                            result
                                .map_err(|error| ThroughputError::io("DSCP socket setup", error))?;
                        }
                        socket
                            .connect(address)
                            .await
                            .map_err(|error| ThroughputError::io("UDP data connection", error))?;
                        #[cfg(windows)]
                        let traffic_class = if spec.tos == 0 {
                            None
                        } else {
                            Some(traffic_class::apply(&socket, spec.tos).map_err(|error| {
                                ThroughputError::io("Windows qWAVE DSCP setup", error)
                            })?)
                        };
                        #[cfg(not(windows))]
                        let traffic_class = None;
                        self.udp_handshake(&socket, !sender, cancellation).await?;
                        DataStream::Udp {
                            traffic_class,
                            socket,
                            sender,
                        }
                    }
                    Protocol::Both => {
                        return Err(ThroughputError::Validation(
                            "a concrete TestSpec protocol cannot be both".into(),
                        ));
                    }
                };
                streams.push(stream);
            }
        }
        Ok(streams)
    }

    async fn udp_handshake(
        &self,
        socket: &UdpSocket,
        reverse: bool,
        cancellation: &CancellationToken,
    ) -> Result<()> {
        self.phase(cancellation, "UDP stream handshake", async {
            socket
                .send(&UDP_CONNECT_MSG)
                .await
                .map_err(|error| ThroughputError::io("UDP stream handshake", error))?;
            let mut reply = vec![0_u8; 65_507];
            for attempt in 0..=if reverse { 2 } else { 0 } {
                let count = socket
                    .recv(&mut reply)
                    .await
                    .map_err(|error| ThroughputError::io("UDP stream handshake", error))?;
                if count == 4
                    && (reply[..4] == UDP_CONNECT_REPLY || reply[..4] == UDP_LEGACY_REPLY_LE)
                {
                    return Ok(());
                }
                if !reverse || count < 16 || attempt == 2 {
                    return Err(ThroughputError::UnexpectedState {
                        actual: 0,
                        expected: "UDP connect reply",
                    });
                }
            }
            Ok(())
        })
        .await
    }

    async fn run_data(
        &self,
        spec: &TestSpec,
        streams: &mut Vec<DataStream>,
        cancellation: &CancellationToken,
    ) -> Result<(Vec<StreamOutcome>, Vec<DataStream>)> {
        let started = Instant::now();
        let omit_end = started + Duration::from_secs(spec.omit_secs);
        let end = omit_end + Duration::from_secs(spec.duration_secs);
        let udp_rate = spec.udp_rate_bps.unwrap_or(1_000_000);
        let mut join: tokio::task::JoinSet<Result<(StreamOutcome, DataStream)>> =
            tokio::task::JoinSet::new();
        for (index, stream) in streams.drain(..).enumerate() {
            // libiperf's linked-list insertion assigns 1 to the first stream,
            // then existing_count + 2 (so the second stream is 3).
            let stream_id = if index == 0 { 1 } else { index + 2 };
            let token = cancellation.clone();
            join.spawn(async move {
                match stream {
                    DataStream::Tcp {
                        traffic_class,
                        mut socket,
                        sender,
                    } if sender => {
                        let outcome =
                            tcp_send(stream_id, &mut socket, omit_end, end, token).await?;
                        Ok((
                            outcome,
                            DataStream::Tcp {
                                traffic_class,
                                socket,
                                sender,
                            },
                        ))
                    }
                    DataStream::Tcp {
                        traffic_class,
                        mut socket,
                        sender,
                    } => {
                        let outcome =
                            tcp_receive(stream_id, &mut socket, omit_end, end, token).await?;
                        Ok((
                            outcome,
                            DataStream::Tcp {
                                traffic_class,
                                socket,
                                sender,
                            },
                        ))
                    }
                    DataStream::Udp {
                        traffic_class,
                        socket,
                        sender,
                    } if sender => {
                        let outcome =
                            udp_send(stream_id, &socket, udp_rate, omit_end, end, token).await?;
                        Ok((
                            outcome,
                            DataStream::Udp {
                                traffic_class,
                                socket,
                                sender,
                            },
                        ))
                    }
                    DataStream::Udp {
                        traffic_class,
                        socket,
                        sender,
                    } => {
                        let outcome = udp_receive(stream_id, &socket, omit_end, end, token).await?;
                        Ok((
                            outcome,
                            DataStream::Udp {
                                traffic_class,
                                socket,
                                sender,
                            },
                        ))
                    }
                }
            });
        }
        let mut outcomes = Vec::new();
        let mut held_streams = Vec::new();
        while let Some(result) = join.join_next().await {
            let (outcome, stream) = result.map_err(|error| {
                ThroughputError::io("data task", io::Error::other(error.to_string()))
            })??;
            outcomes.push(outcome);
            held_streams.push(stream);
        }
        outcomes.sort_by_key(|outcome| outcome.id);
        Ok((outcomes, held_streams))
    }

    async fn expect_state(
        &self,
        control: &mut TcpStream,
        expected: i8,
        cancellation: &CancellationToken,
    ) -> Result<()> {
        let state = self
            .phase(cancellation, "control state", async {
                let mut byte = [0_u8; 1];
                control
                    .read_exact(&mut byte)
                    .await
                    .map_err(|error| ThroughputError::io("control state", error))?;
                Ok(byte[0] as i8)
            })
            .await?;
        match state {
            ACCESS_DENIED => Err(ThroughputError::ServerBusy),
            SERVER_TERMINATE => Err(ThroughputError::Server {
                iperf_code: SERVER_TERMINATE.into(),
                system_code: 0,
            }),
            SERVER_ERROR => {
                let mut codes = [0_u8; 8];
                self.phase(cancellation, "server error", async {
                    control
                        .read_exact(&mut codes)
                        .await
                        .map_err(|error| ThroughputError::io("server error", error))?;
                    Ok(())
                })
                .await?;
                Err(ThroughputError::Server {
                    iperf_code: i32::from_be_bytes(codes[..4].try_into().unwrap()),
                    system_code: i32::from_be_bytes(codes[4..].try_into().unwrap()),
                })
            }
            actual if actual == expected => Ok(()),
            actual => Err(ThroughputError::UnexpectedState {
                actual,
                expected: state_name(expected),
            }),
        }
    }

    async fn write_json(
        &self,
        control: &mut TcpStream,
        value: &Value,
        cancellation: &CancellationToken,
    ) -> Result<()> {
        let encoded = serde_json::to_vec(value)?;
        if encoded.len() > self.max_control_json_bytes || encoded.len() > u32::MAX as usize {
            return Err(ThroughputError::ControlMessageTooLarge {
                actual: encoded.len(),
                maximum: self.max_control_json_bytes,
            });
        }
        self.phase(cancellation, "control JSON write", async {
            control
                .write_all(&(encoded.len() as u32).to_be_bytes())
                .await
                .map_err(|error| ThroughputError::io("control JSON write", error))?;
            control
                .write_all(&encoded)
                .await
                .map_err(|error| ThroughputError::io("control JSON write", error))
        })
        .await
    }

    async fn read_json(
        &self,
        control: &mut TcpStream,
        cancellation: &CancellationToken,
    ) -> Result<Value> {
        let maximum = self.max_control_json_bytes;
        self.phase(cancellation, "control JSON read", async {
            let mut length = [0_u8; 4];
            control
                .read_exact(&mut length[..1])
                .await
                .map_err(|error| ThroughputError::io("control JSON read", error))?;
            let state = length[0] as i8;
            if state == SERVER_ERROR {
                let mut codes = [0_u8; 8];
                control
                    .read_exact(&mut codes)
                    .await
                    .map_err(|error| ThroughputError::io("server error", error))?;
                return Err(ThroughputError::Server {
                    iperf_code: i32::from_be_bytes(codes[..4].try_into().unwrap()),
                    system_code: i32::from_be_bytes(codes[4..].try_into().unwrap()),
                });
            }
            if state == ACCESS_DENIED {
                return Err(ThroughputError::ServerBusy);
            }
            control
                .read_exact(&mut length[1..])
                .await
                .map_err(|error| ThroughputError::io("control JSON read", error))?;
            let length = u32::from_be_bytes(length) as usize;
            if length > maximum {
                return Err(ThroughputError::ControlMessageTooLarge {
                    actual: length,
                    maximum,
                });
            }
            let mut encoded = vec![0_u8; length];
            control
                .read_exact(&mut encoded)
                .await
                .map_err(|error| ThroughputError::io("control JSON read", error))?;
            Ok(serde_json::from_slice(&encoded)?)
        })
        .await
    }

    async fn phase<T, F>(
        &self,
        cancellation: &CancellationToken,
        phase: &'static str,
        future: F,
    ) -> Result<T>
    where
        F: Future<Output = Result<T>>,
    {
        tokio::select! {
            _ = cancellation.cancelled() => Err(ThroughputError::Cancelled),
            result = tokio::time::timeout(self.control_timeout, future) => {
                result.unwrap_or_else(|_| Err(ThroughputError::Timeout {
                    phase,
                    timeout_ms: millis(self.control_timeout),
                }))
            }
        }
    }
}

async fn drain_tcp_receivers(streams: Vec<DataStream>, stop: CancellationToken) -> Vec<DataStream> {
    let mut draining = tokio::task::JoinSet::new();
    let mut held = Vec::with_capacity(streams.len());
    for stream in streams {
        match stream {
            DataStream::Tcp {
                traffic_class,
                mut socket,
                sender: false,
            } => {
                let stop = stop.clone();
                draining.spawn(async move {
                    let mut buffer = vec![0_u8; TCP_BLOCK_SIZE];
                    loop {
                        tokio::select! { biased;
                            _ = stop.cancelled() => break,
                            result = socket.read(&mut buffer) => match result {
                                Ok(0) | Err(_) => break,
                                Ok(_) => {}
                            },
                        }
                    }
                    DataStream::Tcp {
                        traffic_class,
                        socket,
                        sender: false,
                    }
                });
            }
            stream => held.push(stream),
        }
    }
    while let Some(result) = draining.join_next().await {
        if let Ok(stream) = result {
            held.push(stream);
        }
    }
    held
}

impl Default for Iperf3Client {
    fn default() -> Self {
        Self::new()
    }
}

enum DataStream {
    Tcp {
        traffic_class: Option<TrafficClassFlow>,
        socket: TcpStream,
        sender: bool,
    },
    Udp {
        traffic_class: Option<TrafficClassFlow>,
        socket: UdpSocket,
        sender: bool,
    },
}

#[derive(Debug)]
struct StreamOutcome {
    id: usize,
    sender: bool,
    bytes: u64,
    packets: u64,
    errors: u64,
    jitter_seconds: f64,
    duration_seconds: f64,
    wire_packets: u64,
    wire_errors: u64,
}

fn parameters(spec: &TestSpec) -> Value {
    let mut value = json!({
        "omit": spec.omit_secs,
        "time": spec.duration_secs,
        "num": 0,
        "blockcount": 0,
        "parallel": spec.streams,
        "client_version": concat!("network-lantern/", env!("CARGO_PKG_VERSION")),
    });
    let object = value.as_object_mut().unwrap();
    object.insert(
        if spec.protocol == Protocol::Udp {
            "udp"
        } else {
            "tcp"
        }
        .into(),
        Value::Bool(true),
    );
    if spec.direction == Direction::Rx {
        object.insert("reverse".into(), Value::Bool(true));
    } else if spec.direction == Direction::Bidirectional {
        object.insert("bidirectional".into(), Value::Bool(true));
    }
    if spec.tos != 0 {
        object.insert("TOS".into(), Value::from(spec.tos));
    }
    if let Some(window) = spec.tcp_window_bytes {
        object.insert("window".into(), Value::from(window));
    }
    if spec.protocol == Protocol::Udp {
        object.insert(
            "bandwidth".into(),
            Value::from(spec.udp_rate_bps.unwrap_or(1_000_000)),
        );
        object.insert("len".into(), Value::from(UDP_BLOCK_SIZE));
        object.insert("udp_counters_64bit".into(), Value::from(1));
    } else {
        object.insert("len".into(), Value::from(TCP_BLOCK_SIZE));
    }
    value
}

fn client_results(outcomes: &[StreamOutcome]) -> Value {
    let streams = outcomes
        .iter()
        .map(|outcome| {
            json!({
                "id": outcome.id,
                "bytes": outcome.bytes,
                "retransmits": -1,
                "jitter": outcome.jitter_seconds,
                "errors": outcome.wire_errors,
                "packets": outcome.wire_packets,
                "start_time": 0.0,
                "end_time": outcome.duration_seconds,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "cpu_util_total": -1.0,
        "cpu_util_user": -1.0,
        "cpu_util_system": -1.0,
        "sender_has_retransmits": -1,
        "streams": streams,
    })
}

fn validate_spec(spec: &TestSpec) -> Result<()> {
    if spec.id == 0
        || spec.target.trim().is_empty()
        || spec.target.len() > 253
        || spec.target.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '/' | '\\' | ';' | '&' | '|' | '`' | '$')
        })
        || spec.port == 0
        || !(1..=3_600).contains(&spec.duration_secs)
        || spec.omit_secs > 60
        || spec.streams == 0
        || spec.streams > 128
        || spec.tos != spec.dscp.tos()
    {
        return Err(ThroughputError::Validation(
            "invalid concrete test specification".into(),
        ));
    }
    if spec.protocol == Protocol::Both {
        return Err(ThroughputError::Validation(
            "a concrete test cannot use protocol both".into(),
        ));
    }
    if spec.protocol == Protocol::Udp {
        let rate = spec.udp_rate_bps.unwrap_or(0);
        if rate == 0 || rate > crate::MAX_UDP_RATE_BPS {
            return Err(ThroughputError::Validation(format!(
                "UDP tests require a rate between 1 and {} bits per second",
                crate::MAX_UDP_RATE_BPS
            )));
        }
    }
    Ok(())
}

impl TestSpec {
    /// Validates a single bounded measurement independently of suite planning.
    pub fn validate(&self) -> Result<()> {
        validate_spec(self)
    }
}

fn random_cookie() -> [u8; COOKIE_SIZE] {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut cookie = [0_u8; COOKIE_SIZE];
    let mut rng = rand::rng();
    for byte in &mut cookie[..COOKIE_SIZE - 1] {
        *byte = ALPHABET[rng.random_range(0..ALPHABET.len())];
    }
    cookie
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

fn state_name(state: i8) -> &'static str {
    match state {
        PARAM_EXCHANGE => "PARAM_EXCHANGE",
        CREATE_STREAMS => "CREATE_STREAMS",
        TEST_START => "TEST_START",
        TEST_RUNNING => "TEST_RUNNING",
        EXCHANGE_RESULTS => "EXCHANGE_RESULTS",
        DISPLAY_RESULTS => "DISPLAY_RESULTS",
        IPERF_DONE => "IPERF_DONE",
        _ => "known state",
    }
}

#[cfg(unix)]
fn set_ipv6_traffic_class<S: std::os::fd::AsFd>(socket: &S, value: u8) -> std::io::Result<()> {
    socket2::SockRef::from(socket).set_tclass_v6(u32::from(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_has_protocol_size_and_terminator() {
        let cookie = random_cookie();
        assert_eq!(cookie.len(), COOKIE_SIZE);
        assert_eq!(cookie[COOKIE_SIZE - 1], 0);
        assert!(
            cookie[..COOKIE_SIZE - 1]
                .iter()
                .all(|byte| b"abcdefghijklmnopqrstuvwxyz234567".contains(byte))
        );
    }

    #[test]
    fn diagnostic_bound_preserves_utf8_and_signals_truncation() {
        let (text, truncated) = bounded_diagnostic(&"é".repeat(10_000), 16_383);
        assert!(truncated);
        assert_eq!(text.len(), 16_382);
        assert!(text.is_char_boundary(text.len()));
    }

    #[test]
    fn parameters_cover_tcp_reverse_bidir_and_udp() {
        let base = TestSpec {
            id: 1,
            target: "127.0.0.1".into(),
            port: 5201,
            ip_version: IpVersion::Ipv4,
            protocol: Protocol::Tcp,
            direction: Direction::Rx,
            dscp: crate::DscpClass::EF,
            tos: crate::DscpClass::EF.tos(),
            streams: 4,
            tcp_window_bytes: Some(128 * 1024),
            udp_rate_bps: None,
            duration_secs: 10,
            omit_secs: 1,
            phase: crate::TestPhase::TcpMatrix,
        };
        let reverse = parameters(&base);
        assert_eq!(reverse["tcp"], true);
        assert_eq!(reverse["reverse"], true);
        assert_eq!(reverse["parallel"], 4);
        assert_eq!(reverse["TOS"], 184);
        let mut bidir = base.clone();
        bidir.direction = Direction::Bidirectional;
        assert_eq!(parameters(&bidir)["bidirectional"], true);
        let mut udp = base;
        udp.protocol = Protocol::Udp;
        udp.direction = Direction::Tx;
        udp.udp_rate_bps = Some(10_000_000);
        let udp = parameters(&udp);
        assert_eq!(udp["udp"], true);
        assert_eq!(udp["bandwidth"], 10_000_000);
        assert_eq!(udp["udp_counters_64bit"], 1);
    }

    #[test]
    fn concrete_spec_validation_binds_dscp_and_runtime_bounds() {
        let mut spec = TestSpec {
            id: 1,
            target: "127.0.0.1".into(),
            port: 5201,
            ip_version: IpVersion::Ipv4,
            protocol: Protocol::Tcp,
            direction: Direction::Tx,
            dscp: crate::DscpClass::EF,
            tos: crate::DscpClass::EF.tos(),
            streams: 1,
            tcp_window_bytes: None,
            udp_rate_bps: None,
            duration_secs: 10,
            omit_secs: 1,
            phase: crate::TestPhase::Single,
        };
        spec.validate().unwrap();

        spec.tos = crate::DscpClass::AF11.tos();
        assert!(matches!(
            spec.validate(),
            Err(ThroughputError::Validation(_))
        ));
        spec.tos = crate::DscpClass::EF.tos();
        spec.duration_secs = 0;
        assert!(matches!(
            spec.validate(),
            Err(ThroughputError::Validation(_))
        ));
    }

    #[test]
    fn client_timeout_limits_are_finite() {
        assert!(Iperf3Client::with_limits(1, MAX_CLIENT_TIMEOUT_MS, 2).is_ok());
        assert!(Iperf3Client::with_limits(0, 1, 2).is_err());
        assert!(Iperf3Client::with_limits(1, MAX_CLIENT_TIMEOUT_MS + 1, 2).is_err());
    }

    #[tokio::test]
    async fn reverse_tcp_drain_unblocks_a_full_sender_and_stops_promptly() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let socket = TcpSocket::new_v4().unwrap();
        socket.set_send_buffer_size(4 * 1024).unwrap();
        let mut sender = socket
            .connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (receiver, _) = listener.accept().await.unwrap();
        let mut write = tokio::spawn(async move {
            let result = sender.write_all(&vec![0_u8; 8 * 1024 * 1024]).await;
            (sender, result)
        });

        assert!(
            tokio::time::timeout(Duration::from_millis(25), &mut write)
                .await
                .is_err()
        );

        let stop = CancellationToken::new();
        let drain = tokio::spawn(drain_tcp_receivers(
            vec![DataStream::Tcp {
                traffic_class: None,
                socket: receiver,
                sender: false,
            }],
            stop.clone(),
        ));
        let (_sender, result) = tokio::time::timeout(Duration::from_secs(2), &mut write)
            .await
            .expect("reverse receiver drain should unblock the data sender")
            .unwrap();
        result.unwrap();
        stop.cancel();
        let held = tokio::time::timeout(Duration::from_millis(250), drain)
            .await
            .expect("reverse receiver drain should honor cancellation")
            .unwrap();
        assert_eq!(held.len(), 1);
    }

    async fn frame_result(bytes: Vec<u8>) -> Result<Value> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            stream.write_all(&bytes).await.unwrap();
        });
        let mut stream = TcpStream::connect(address).await.unwrap();
        let result = Iperf3Client::with_limits(100, 100, 1024)
            .unwrap()
            .read_json(&mut stream, &CancellationToken::new())
            .await;
        peer.await.unwrap();
        result
    }
    #[tokio::test]
    async fn bounded_control_frames_reject_truncation_oversize_and_malformed_json() {
        for bytes in [
            vec![],
            vec![0, 0],
            vec![0, 0, 0, 5, b'{'],
            vec![0, 0, 0, 1, b'{'],
        ] {
            assert!(frame_result(bytes).await.is_err());
        }
        assert!(matches!(
            frame_result(2048u32.to_be_bytes().to_vec()).await,
            Err(ThroughputError::ControlMessageTooLarge { .. })
        ));
        let mut valid = 2u32.to_be_bytes().to_vec();
        valid.extend_from_slice(b"{}");
        assert_eq!(frame_result(valid).await.unwrap(), json!({}));
        assert!(matches!(
            frame_result(vec![255]).await,
            Err(ThroughputError::ServerBusy)
        ));
        let mut error = vec![254];
        error.extend_from_slice(&42i32.to_be_bytes());
        error.extend_from_slice(&13i32.to_be_bytes());
        assert!(matches!(
            frame_result(error).await,
            Err(ThroughputError::Server {
                iperf_code: 42,
                system_code: 13
            })
        ));
        assert!(frame_result(vec![254, 0, 0]).await.is_err());
    }
    #[tokio::test]
    async fn reverse_udp_handshake_accepts_at_most_two_early_datagrams() {
        for early in 0..=3 {
            let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            client.connect(peer.local_addr().unwrap()).await.unwrap();
            let task = tokio::spawn(async move {
                let mut buffer = [0u8; 4];
                let (_, address) = peer.recv_from(&mut buffer).await.unwrap();
                for _ in 0..early {
                    peer.send_to(&[0u8; 16], address).await.unwrap();
                }
                peer.send_to(&UDP_CONNECT_REPLY, address).await.unwrap();
            });
            let result = Iperf3Client::with_limits(100, 500, 1024)
                .unwrap()
                .udp_handshake(&client, true, &CancellationToken::new())
                .await;
            assert_eq!(result.is_ok(), early <= 2);
            task.await.unwrap();
        }
    }
    #[tokio::test]
    async fn stalled_control_is_timed_out_and_cancellable() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (_peer, _) = listener.accept().await.unwrap();
        let engine = Iperf3Client::with_limits(50, 50, 1024).unwrap();
        assert!(matches!(
            engine
                .expect_state(&mut client, PARAM_EXCHANGE, &CancellationToken::new())
                .await,
            Err(ThroughputError::Timeout { .. })
        ));
        let token = CancellationToken::new();
        token.cancel();
        assert!(matches!(
            engine
                .expect_state(&mut client, PARAM_EXCHANGE, &token)
                .await,
            Err(ThroughputError::Cancelled)
        ));
    }
}
