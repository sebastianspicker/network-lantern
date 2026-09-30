use super::*;
const REORDER_WINDOW: usize = 65_536;

pub(super) async fn tcp_send(
    id: usize,
    socket: &mut TcpStream,
    omit_end: Instant,
    end: Instant,
    cancellation: CancellationToken,
) -> Result<StreamOutcome> {
    let buffer = vec![0u8; TCP_BLOCK_SIZE];
    let mut bytes = 0u64;
    loop {
        tokio::select! {biased;
            _=cancellation.cancelled()=>return Err(ThroughputError::Cancelled),
            _=tokio::time::sleep_until(end)=>break,
            result=socket.write(&buffer)=>{
                let written = result.map_err(|e| ThroughputError::io("TCP send", e))?;
                if written == 0 {
                    return Err(ThroughputError::io("TCP send", io::ErrorKind::WriteZero.into()));
                }
                if Instant::now() >= omit_end {
                    bytes = bytes.saturating_add(written as u64);
                }
            }
        }
    }
    Ok(outcome(id, true, bytes, omit_end))
}
pub(super) async fn tcp_receive(
    id: usize,
    socket: &mut TcpStream,
    omit_end: Instant,
    end: Instant,
    cancellation: CancellationToken,
) -> Result<StreamOutcome> {
    let mut buffer = vec![0u8; TCP_BLOCK_SIZE];
    let mut bytes = 0u64;
    loop {
        tokio::select! {biased;
            _=cancellation.cancelled()=>return Err(ThroughputError::Cancelled),
            _=tokio::time::sleep_until(end)=>break,
            result=socket.read(&mut buffer)=>{
                let read=result.map_err(|e|ThroughputError::io("TCP receive",e))?;
                if read==0{if Instant::now()+Duration::from_millis(20)<end{return Err(ThroughputError::io("TCP receive",io::ErrorKind::UnexpectedEof.into()));}break;}
                if Instant::now()>=omit_end{bytes=bytes.saturating_add(read as u64);}
            }
        }
    }
    Ok(outcome(id, false, bytes, omit_end))
}
fn outcome(id: usize, sender: bool, bytes: u64, omit_end: Instant) -> StreamOutcome {
    StreamOutcome {
        id,
        sender,
        bytes,
        packets: 0,
        errors: 0,
        jitter_seconds: 0.0,
        duration_seconds: Instant::now()
            .saturating_duration_since(omit_end)
            .as_secs_f64(),
        wire_packets: 0,
        wire_errors: 0,
    }
}
pub(super) async fn udp_send(
    id: usize,
    socket: &UdpSocket,
    rate_bps: u64,
    omit_end: Instant,
    end: Instant,
    cancellation: CancellationToken,
) -> Result<StreamOutcome> {
    let mut packet = vec![0u8; UDP_BLOCK_SIZE];
    let interval = Duration::from_secs_f64(UDP_BLOCK_SIZE as f64 * 8.0 / rate_bps as f64);
    if interval.is_zero() {
        return Err(ThroughputError::Validation(
            "UDP pacing interval rounded to zero".into(),
        ));
    }
    let origin = Instant::now();
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ThroughputError::Validation("System clock predates Unix epoch".into()))?;
    let mut deadline = origin;
    let mut sequence = 0u64;
    let mut bytes = 0u64;
    let mut packets = 0u64;
    loop {
        tokio::select! {biased;
            _=cancellation.cancelled()=>return Err(ThroughputError::Cancelled),
            _=tokio::time::sleep_until(end)=>break,
            _=tokio::time::sleep_until(deadline)=>{}
        }
        let timestamp = wall + origin.elapsed();
        packet[..4].copy_from_slice(&(timestamp.as_secs() as u32).to_be_bytes());
        packet[4..8].copy_from_slice(&timestamp.subsec_micros().to_be_bytes());
        sequence = sequence
            .checked_add(1)
            .ok_or_else(|| ThroughputError::Validation("UDP sequence exhausted".into()))?;
        packet[8..16].copy_from_slice(&sequence.to_be_bytes());
        let sent = tokio::select! {biased;
            _=cancellation.cancelled()=>return Err(ThroughputError::Cancelled),
            _=tokio::time::sleep_until(end)=>break,
            result=socket.send(&packet)=>result.map_err(|e|ThroughputError::io("UDP send",e))?
        };
        if sent != packet.len() {
            return Err(ThroughputError::io(
                "UDP send",
                io::ErrorKind::WriteZero.into(),
            ));
        }
        if Instant::now() >= omit_end {
            bytes = bytes.saturating_add(sent as u64);
            packets += 1;
        }
        deadline += interval;
    }
    let mut result = outcome(id, true, bytes, omit_end);
    result.packets = packets;
    result.wire_packets = sequence;
    Ok(result)
}

struct DatagramStats {
    baseline: u64,
    highest: u64,
    unique: u64,
    seen: Vec<u64>,
    bytes: u64,
    prior_transit: Option<f64>,
    jitter: f64,
}
impl DatagramStats {
    fn new(baseline: u64) -> Self {
        Self {
            baseline,
            highest: baseline,
            unique: 0,
            seen: vec![0; REORDER_WINDOW],
            bytes: 0,
            prior_transit: None,
            jitter: 0.0,
        }
    }
    fn after_omission(all: &Self) -> Self {
        let mut measured = Self::new(all.highest);
        measured.prior_transit = all.prior_transit;
        measured
    }
    fn observe(&mut self, sequence: u64, bytes: usize, arrival: f64, sent: f64) -> Result<()> {
        if sequence == 0 {
            return Err(ThroughputError::MalformedResult("UDP sequence zero".into()));
        }
        if sequence <= self.baseline {
            return Ok(());
        }
        if self.highest.saturating_sub(sequence) >= REORDER_WINDOW as u64 {
            return Err(ThroughputError::MalformedResult(
                "UDP reordering exceeds the bounded correlation window".into(),
            ));
        }
        let slot = sequence as usize % REORDER_WINDOW;
        if self.seen[slot] == sequence {
            return Ok(());
        }
        self.seen[slot] = sequence;
        self.highest = self.highest.max(sequence);
        self.unique += 1;
        self.bytes = self.bytes.saturating_add(bytes as u64);
        let transit = arrival - sent;
        if let Some(previous) = self.prior_transit {
            self.jitter += ((transit - previous).abs() - self.jitter) / 16.0;
        }
        self.prior_transit = Some(transit);
        Ok(())
    }
    fn expected(&self) -> u64 {
        self.highest - self.baseline
    }
    fn lost(&self) -> u64 {
        self.expected().saturating_sub(self.unique)
    }
}
pub(super) async fn udp_receive(
    id: usize,
    socket: &UdpSocket,
    omit_end: Instant,
    end: Instant,
    cancellation: CancellationToken,
) -> Result<StreamOutcome> {
    let mut buffer = vec![0u8; 65_507];
    let origin = Instant::now();
    let mut all = DatagramStats::new(0);
    let mut measured: Option<DatagramStats> = None;
    loop {
        let read = tokio::select! {biased;
            _=cancellation.cancelled()=>return Err(ThroughputError::Cancelled),
            _=tokio::time::sleep_until(end)=>break,
            result=socket.recv(&mut buffer)=>result.map_err(|e|ThroughputError::io("UDP receive",e))?
        };
        if read < 16 {
            return Err(ThroughputError::MalformedResult(
                "Truncated UDP datagram".into(),
            ));
        }
        let micros = u32::from_be_bytes(buffer[4..8].try_into().unwrap());
        if micros >= 1_000_000 {
            return Err(ThroughputError::MalformedResult(
                "Invalid UDP timestamp".into(),
            ));
        }
        let sequence = u64::from_be_bytes(buffer[8..16].try_into().unwrap());
        let sent = u32::from_be_bytes(buffer[..4].try_into().unwrap()) as f64
            + micros as f64 / 1_000_000.0;
        let arrival = origin.elapsed().as_secs_f64();
        if Instant::now() >= omit_end {
            let measured = measured.get_or_insert_with(|| DatagramStats::after_omission(&all));
            measured.observe(sequence, read, arrival, sent)?;
        }
        all.observe(sequence, read, arrival, sent)?;
    }
    let measured = measured.unwrap_or_else(|| DatagramStats::new(all.highest));
    let mut result = outcome(id, false, measured.bytes, omit_end);
    result.packets = measured.expected();
    result.errors = measured.lost();
    result.jitter_seconds = measured.jitter;
    result.wire_packets = all.highest;
    result.wire_errors = all.lost();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicates_reordering_loss_and_omission() {
        let mut stats = DatagramStats::new(100);
        for (seq, arrival) in [(101, 1.0), (103, 2.0), (103, 2.1), (102, 3.0)] {
            stats.observe(seq, 16, arrival, arrival - 0.1).unwrap();
        }
        assert_eq!(stats.expected(), 3);
        assert_eq!(stats.unique, 3);
        assert_eq!(stats.lost(), 0);
        assert_eq!(stats.bytes, 48);
        stats.observe(105, 16, 4.0, 3.9).unwrap();
        assert_eq!(stats.lost(), 1);
        stats.observe(100, 16, 5.0, 4.9).unwrap();
        assert_eq!(stats.unique, 4);
    }
    #[test]
    fn jitter_uses_differences_and_bounded_window() {
        let mut stats = DatagramStats::new(0);
        stats.observe(1, 16, 1.0, 0.0).unwrap();
        stats.observe(2, 16, 2.0, 0.5).unwrap();
        assert_eq!(stats.jitter, 0.5 / 16.0);
        stats
            .observe(REORDER_WINDOW as u64 + 3, 16, 3.0, 1.0)
            .unwrap();
        assert!(stats.observe(1, 16, 4.0, 2.0).is_err());
    }
    #[test]
    fn omission_resets_jitter_but_preserves_the_previous_transit() {
        let mut all = DatagramStats::new(0);
        all.observe(1, 16, 1.0, 0.0).unwrap();
        let mut measured = DatagramStats::after_omission(&all);
        measured.observe(2, 16, 2.0, 0.5).unwrap();
        assert_eq!(measured.jitter, 0.5 / 16.0);
        assert_eq!(measured.bytes, 16);
        assert_eq!(measured.expected(), 1);
    }
    #[tokio::test]
    async fn premature_tcp_eof_is_a_failure() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (peer, _) = listener.accept().await.unwrap();
        drop(peer);
        let now = Instant::now();
        assert!(
            tcp_receive(
                1,
                &mut client,
                now,
                now + Duration::from_secs(1),
                CancellationToken::new()
            )
            .await
            .is_err()
        );
    }
}
