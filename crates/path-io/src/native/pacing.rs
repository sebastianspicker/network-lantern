use crate::ProbeError;
use std::{
    thread,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const POLL_INTERVAL: Duration = Duration::from_millis(5);

pub(super) struct PacedResult<T> {
    pub(super) completed: Vec<(usize, T)>,
    pub(super) failure: Option<ProbeError>,
}

pub(super) fn wait_until(
    deadline: Instant,
    cancellation: &CancellationToken,
) -> Result<(), ProbeError> {
    loop {
        if cancellation.is_cancelled() {
            return Err(ProbeError::Cancelled);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        thread::sleep(remaining.min(POLL_INTERVAL));
    }
}

pub(super) fn run_sequential<I, T, F>(
    jobs: impl IntoIterator<Item = (usize, I)>,
    interval: Duration,
    cancellation: &CancellationToken,
    mut probe: F,
) -> PacedResult<T>
where
    F: FnMut(I, &CancellationToken) -> Result<T, ProbeError>,
{
    let mut completed = Vec::new();
    let mut next_send = Instant::now();
    for (owner, input) in jobs {
        if let Err(error) = wait_until(next_send, cancellation) {
            return PacedResult {
                completed,
                failure: Some(error),
            };
        }
        let sent = Instant::now();
        match probe(input, cancellation) {
            Ok(output) => completed.push((owner, output)),
            Err(error) => {
                return PacedResult {
                    completed,
                    failure: Some(error),
                };
            }
        }
        // Advance from the actual send. A delayed probe can consume one interval,
        // but the sequential runner cannot emit a catch-up burst.
        next_send = sent + interval;
    }
    PacedResult {
        completed,
        failure: None,
    }
}

pub(super) fn run_concurrent<I, T, F>(
    jobs: impl IntoIterator<Item = (usize, I)>,
    interval: Duration,
    max_outstanding: usize,
    cancellation: &CancellationToken,
    probe: F,
) -> PacedResult<T>
where
    I: Send,
    T: Send,
    F: Fn(I, &CancellationToken) -> Result<T, ProbeError> + Sync,
{
    assert!(max_outstanding > 0);
    let worker_cancellation = cancellation.child_token();
    let mut completed = Vec::new();
    let mut failure = None;

    thread::scope(|scope| {
        let mut workers: Vec<(usize, thread::ScopedJoinHandle<'_, Result<T, ProbeError>>)> =
            Vec::new();
        let mut next_send = Instant::now();

        'jobs: for (owner, input) in jobs {
            if let Err(error) = wait_until(next_send, cancellation) {
                record_failure(&mut failure, &worker_cancellation, error);
                break;
            }
            loop {
                while let Some(position) =
                    workers.iter().position(|(_, worker)| worker.is_finished())
                {
                    let (finished_owner, worker) = workers.remove(position);
                    collect_worker(
                        finished_owner,
                        worker,
                        &mut completed,
                        &mut failure,
                        &worker_cancellation,
                    );
                }
                if cancellation.is_cancelled() {
                    record_failure(&mut failure, &worker_cancellation, ProbeError::Cancelled);
                    break 'jobs;
                }
                if failure.is_some() {
                    break 'jobs;
                }
                if workers.len() < max_outstanding {
                    break;
                }
                thread::sleep(POLL_INTERVAL);
            }

            let worker_cancellation = worker_cancellation.clone();
            let probe = &probe;
            workers.push((
                owner,
                scope.spawn(move || probe(input, &worker_cancellation)),
            ));
            // Capacity waits and scheduler stalls never shorten the next interval.
            next_send = Instant::now() + interval;
        }

        while !workers.is_empty() {
            if cancellation.is_cancelled() && failure.is_none() {
                record_failure(&mut failure, &worker_cancellation, ProbeError::Cancelled);
            }
            if let Some(position) = workers.iter().position(|(_, worker)| worker.is_finished()) {
                let (owner, worker) = workers.remove(position);
                collect_worker(
                    owner,
                    worker,
                    &mut completed,
                    &mut failure,
                    &worker_cancellation,
                );
            } else {
                thread::sleep(POLL_INTERVAL);
            }
        }
    });

    PacedResult { completed, failure }
}

fn collect_worker<T>(
    owner: usize,
    worker: thread::ScopedJoinHandle<'_, Result<T, ProbeError>>,
    completed: &mut Vec<(usize, T)>,
    failure: &mut Option<ProbeError>,
    cancellation: &CancellationToken,
) {
    match worker.join() {
        Ok(Ok(output)) => completed.push((owner, output)),
        Ok(Err(error)) => record_failure(failure, cancellation, error),
        Err(_) => record_failure(
            failure,
            cancellation,
            ProbeError::Io {
                operation: "paced probe worker",
                message: "probe worker panicked".into(),
            },
        ),
    }
}

fn record_failure(
    failure: &mut Option<ProbeError>,
    cancellation: &CancellationToken,
    error: ProbeError,
) {
    if failure.is_none()
        || matches!(failure, Some(ProbeError::Cancelled)) && !matches!(error, ProbeError::Cancelled)
    {
        *failure = Some(error);
    }
    cancellation.cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    fn update_max(maximum: &AtomicUsize, value: usize) {
        maximum.fetch_max(value, Ordering::SeqCst);
    }

    #[test]
    fn sequential_order_has_no_multi_probe_catch_up() {
        let starts = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&starts);
        let result = run_sequential(
            [(0, 0u8), (1, 1), (2, 2)],
            Duration::from_millis(20),
            &CancellationToken::new(),
            move |job, _| {
                observed.lock().unwrap().push((job, Instant::now()));
                if job == 0 {
                    thread::sleep(Duration::from_millis(35));
                }
                Ok(job)
            },
        );

        assert!(result.failure.is_none());
        assert_eq!(
            result
                .completed
                .iter()
                .map(|(_, job)| *job)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        let starts = starts.lock().unwrap();
        assert!(starts[1].1.duration_since(starts[0].1) >= Duration::from_millis(30));
        assert!(starts[2].1.duration_since(starts[1].1) >= Duration::from_millis(15));
    }

    #[test]
    fn concurrent_probes_do_not_serialize_and_stay_bounded() {
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let active_probe = Arc::clone(&active);
        let maximum_probe = Arc::clone(&maximum);
        let result = run_concurrent(
            (0..18).map(|job| (job, job)),
            Duration::ZERO,
            3,
            &CancellationToken::new(),
            move |job, _| {
                let current = active_probe.fetch_add(1, Ordering::SeqCst) + 1;
                update_max(&maximum_probe, current);
                thread::sleep(Duration::from_millis(15));
                active_probe.fetch_sub(1, Ordering::SeqCst);
                Ok(job)
            },
        );

        assert!(result.failure.is_none());
        assert_eq!(result.completed.len(), 18);
        assert!((2..=3).contains(&maximum.load(Ordering::SeqCst)));
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn worker_error_cancels_and_joins_outstanding_peers() {
        let active = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::new(AtomicUsize::new(0));
        let active_probe = Arc::clone(&active);
        let stopped_probe = Arc::clone(&stopped);
        let started = Instant::now();
        let result = run_concurrent(
            (0..8).map(|job| (job, job)),
            Duration::ZERO,
            4,
            &CancellationToken::new(),
            move |job, cancellation| {
                active_probe.fetch_add(1, Ordering::SeqCst);
                if job == 0 {
                    while active_probe.load(Ordering::SeqCst) < 4 {
                        thread::yield_now();
                    }
                    return Err(ProbeError::Io {
                        operation: "test probe",
                        message: "injected failure".into(),
                    });
                }
                while !cancellation.is_cancelled() {
                    thread::sleep(Duration::from_millis(1));
                }
                stopped_probe.fetch_add(1, Ordering::SeqCst);
                Ok(job)
            },
        );

        assert!(matches!(result.failure, Some(ProbeError::Io { .. })));
        assert_eq!(stopped.load(Ordering::SeqCst), 3);
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
