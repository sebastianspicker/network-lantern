//! Optional interoperability checks against an installed iperf3 server.
//!
//! The production client never launches iperf3. These ignored tests use it as
//! a loopback-only protocol oracle.

use std::io::Read;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use lantern_throughput::{
    Direction, DscpClass, IpVersion, Iperf3Client, Protocol, TestPhase, TestSpec,
};
use tokio_util::sync::CancellationToken;

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_server(port: u16) -> Server {
    start_server_on(port, "127.0.0.1")
}

fn start_server_on(port: u16, address: &str) -> Server {
    let executable =
        std::env::var_os("NETWORK_LANTERN_IPERF3_ORACLE").unwrap_or_else(|| "iperf3".into());
    let child = Command::new(executable)
        .args([
            "--server",
            "--one-off",
            "--bind",
            address,
            "--port",
            &port.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("iperf3 must be installed for this ignored interoperability test");
    Server(child)
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn run(protocol: Protocol, direction: Direction) {
    run_with_streams(protocol, direction, 1).await;
}

async fn run_with_streams(protocol: Protocol, direction: Direction, streams: u16) {
    run_configured(protocol, direction, streams, IpVersion::Ipv4, 0).await;
}

async fn run_configured(
    protocol: Protocol,
    direction: Direction,
    streams: u16,
    family: IpVersion,
    omit: u64,
) {
    let address = if family == IpVersion::Ipv6 {
        "::1"
    } else {
        "127.0.0.1"
    };
    let port = TcpListener::bind((address, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut server = start_server_on(port, address);
    tokio::time::sleep(Duration::from_millis(750)).await;
    let spec = TestSpec {
        id: 1,
        target: address.into(),
        port,
        ip_version: family,
        protocol,
        direction,
        dscp: DscpClass::CS0,
        tos: 0,
        streams,
        tcp_window_bytes: None,
        udp_rate_bps: (protocol == Protocol::Udp).then_some(1_000_000),
        duration_secs: 1,
        omit_secs: omit,
        phase: TestPhase::Single,
    };
    let result = Iperf3Client::new()
        .run_test(&spec, &CancellationToken::new())
        .await;
    if let Err(error) = &result {
        let _ = server.0.kill();
        let mut stderr = String::new();
        if let Some(mut pipe) = server.0.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        let _ = server.0.wait();
        panic!("native client failed: {error}; server stderr: {}", stderr);
    }
    let result = result.unwrap();
    assert!(result.bytes_sent > 0 || result.bytes_received > 0);
    assert!(!result.server_result.is_empty());
}

#[tokio::test]
#[ignore = "requires an installed iperf3 executable as a loopback protocol oracle"]
async fn tcp_tx_interoperates() {
    run(Protocol::Tcp, Direction::Tx).await;
}

#[tokio::test]
#[ignore = "requires an installed iperf3 executable as a loopback protocol oracle"]
async fn tcp_reverse_interoperates() {
    run(Protocol::Tcp, Direction::Rx).await;
}

#[tokio::test]
#[ignore = "requires an installed iperf3 executable as a loopback protocol oracle"]
async fn tcp_bidirectional_interoperates() {
    run(Protocol::Tcp, Direction::Bidirectional).await;
}

#[tokio::test]
#[ignore = "requires an installed iperf3 executable as a loopback protocol oracle"]
async fn tcp_parallel_interoperates() {
    run_with_streams(Protocol::Tcp, Direction::Tx, 4).await;
}

#[tokio::test]
#[ignore = "requires an installed iperf3 executable as a loopback protocol oracle"]
async fn udp_tx_interoperates() {
    run(Protocol::Udp, Direction::Tx).await;
}

#[tokio::test]
#[ignore = "requires an installed iperf3 executable as a loopback protocol oracle"]
async fn udp_reverse_interoperates() {
    run(Protocol::Udp, Direction::Rx).await;
}

#[tokio::test]
#[ignore = "requires an installed iperf3 executable as a loopback protocol oracle"]
async fn omit_time_is_excluded_from_metrics_but_included_in_runtime() {
    let port = free_port();
    let _server = start_server(port);
    tokio::time::sleep(Duration::from_millis(750)).await;
    let spec = TestSpec {
        id: 1,
        target: "127.0.0.1".into(),
        port,
        ip_version: IpVersion::Ipv4,
        protocol: Protocol::Tcp,
        direction: Direction::Tx,
        dscp: DscpClass::CS0,
        tos: 0,
        streams: 1,
        tcp_window_bytes: None,
        udp_rate_bps: None,
        duration_secs: 1,
        omit_secs: 1,
        phase: TestPhase::Single,
    };
    let started = tokio::time::Instant::now();
    let result = Iperf3Client::new()
        .run_test(&spec, &CancellationToken::new())
        .await
        .unwrap();
    assert!(started.elapsed() >= Duration::from_secs(2));
    assert!(result.bytes_sent > 0);
    assert!(result.tx_mbps.is_some_and(|rate| rate > 0.0));
}

#[tokio::test]
#[ignore = "requires an installed iperf3 executable as a loopback protocol oracle"]
async fn cancellation_terminates_an_active_test() {
    let port = free_port();
    let _server = start_server(port);
    tokio::time::sleep(Duration::from_millis(750)).await;
    let spec = TestSpec {
        id: 1,
        target: "127.0.0.1".into(),
        port,
        ip_version: IpVersion::Ipv4,
        protocol: Protocol::Tcp,
        direction: Direction::Tx,
        dscp: DscpClass::CS0,
        tos: 0,
        streams: 1,
        tcp_window_bytes: None,
        udp_rate_bps: None,
        duration_secs: 30,
        omit_secs: 0,
        phase: TestPhase::Single,
    };
    let cancellation = CancellationToken::new();
    let trigger = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        trigger.cancel();
    });
    let started = tokio::time::Instant::now();
    let result = Iperf3Client::new().run_test(&spec, &cancellation).await;
    assert!(matches!(
        result,
        Err(lantern_throughput::ThroughputError::Cancelled)
    ));
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[tokio::test]
#[ignore = "requires an installed iperf3 executable as a loopback protocol oracle"]
async fn ipv6_tcp_and_udp_interoperate() {
    run_configured(Protocol::Tcp, Direction::Tx, 1, IpVersion::Ipv6, 0).await;
    run_configured(Protocol::Udp, Direction::Rx, 1, IpVersion::Ipv6, 0).await;
}

#[tokio::test]
#[ignore = "requires an installed iperf3 executable as a loopback protocol oracle"]
async fn udp_omission_interoperates_in_both_directions() {
    run_configured(Protocol::Udp, Direction::Tx, 1, IpVersion::Ipv4, 1).await;
    run_configured(Protocol::Udp, Direction::Rx, 1, IpVersion::Ipv4, 1).await;
}
