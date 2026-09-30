//! MTR's adaptive interval-per-host scheduler, independent of IO.
use std::time::Duration;
use tokio::time::Instant;

pub(super) struct Cadence {
    pub cycles: u16,
    pub ttl: u8,
    pub deadline: Instant,
    first_ttl: u8,
    max_ttl: u8,
    interval: Duration,
    estimated_hosts: u32,
}
impl Cadence {
    pub fn new(start: Instant, interval: Duration, first_ttl: u8, max_ttl: u8) -> Self {
        Self {
            cycles: 0,
            ttl: first_ttl,
            deadline: start + interval / 10,
            first_ttl,
            max_ttl,
            interval,
            estimated_hosts: 10,
        }
    }
    pub fn dispatched(&mut self, now: Instant, known: &[bool], destination: Option<u8>) {
        let unknown = known[usize::from(self.first_ttl)..usize::from(self.ttl)]
            .iter()
            .filter(|known| !**known)
            .count();
        if destination.is_some_and(|bound| bound <= self.ttl)
            || unknown > 12
            || self.ttl == self.max_ttl
        {
            self.estimated_hosts =
                u32::from(destination.unwrap_or(self.ttl).min(self.ttl) - self.first_ttl) + 1;
            self.cycles += 1;
            self.ttl = self.first_ttl;
        } else {
            self.ttl += 1;
        }
        // MTR's event loop assigns lasttime to the actual send time. It never
        // replays every expired deadline after the scheduler has been delayed.
        self.deadline = now + self.interval / self.estimated_hosts;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adapts_discovery_then_retains_three_hundred_full_hop_sweeps() {
        let start = Instant::now();
        let mut cadence = Cadence::new(start, Duration::from_secs(1), 1, 30);
        let mut known = vec![false; 31];
        let mut sent = [0; 31];
        let mut last = start;
        while cadence.cycles < 300 {
            let ttl = cadence.ttl;
            let now = cadence.deadline;
            let before = cadence.cycles;
            let expected_gap = if before == 0 {
                Duration::from_millis(100)
            } else {
                Duration::from_millis(200)
            };
            // The wrap immediately updates the next gap to I / discovered hops.
            if sent[5] == 0 {
                assert_eq!(now - last, Duration::from_millis(100));
            } else {
                assert_eq!(now - last, expected_gap);
            }
            sent[usize::from(ttl)] += 1;
            known[usize::from(ttl)] = true;
            cadence.dispatched(now, &known, (ttl == 5 || before > 0).then_some(5));
            last = now;
        }
        assert_eq!(&sent[1..=5], &[300; 5]);
        assert!(sent[6..].iter().all(|count| *count == 0));
        assert_eq!(last - start, Duration::from_millis(299500));
    }
    #[test]
    fn lost_hops_and_delayed_destination_follow_mtr_wrap_rules() {
        let start = Instant::now();
        let mut cadence = Cadence::new(start, Duration::from_secs(1), 3, 30);
        let known = vec![false; 31];
        for expected in 3..=16 {
            assert_eq!(cadence.ttl, expected);
            cadence.dispatched(cadence.deadline, &known, None);
        }
        assert_eq!(cadence.cycles, 1);
        assert_eq!(cadence.ttl, 3);
        assert_eq!(cadence.estimated_hosts, 14);
        cadence.ttl = 8;
        cadence.dispatched(cadence.deadline, &known, Some(6));
        assert_eq!(cadence.estimated_hosts, 4);
        assert_eq!(cadence.ttl, 3);
    }
    #[test]
    fn delayed_dispatch_moves_next_deadline_instead_of_catching_up() {
        let start = Instant::now();
        let mut cadence = Cadence::new(start, Duration::from_secs(1), 1, 30);
        let late = start + Duration::from_secs(10);
        cadence.dispatched(late, &[false; 31], None);
        assert_eq!(cadence.deadline, late + Duration::from_millis(100));
        assert_eq!(cadence.ttl, 2);
        assert_eq!(cadence.cycles, 0);
    }
}
