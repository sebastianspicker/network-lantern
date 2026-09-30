//! One active measurement and bounded progress snapshots shared by application adapters.
use lantern_contracts::{Error, ErrorCategory, Result, bounded_text, exit, new_run_id, now};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Running,
    Cancelling,
    Succeeded,
    PartialFailure,
    Failed,
    Cancelled,
}
impl RunState {
    /// The final state of a run that ended with a process status.
    pub fn from_exit_code(code: u8, partial: bool) -> Self {
        match code {
            exit::SUCCESS => Self::Succeeded,
            exit::PARTIAL_FAILURE => Self::PartialFailure,
            code if exit::is_interrupted(code) => Self::Cancelled,
            _ if partial => Self::PartialFailure,
            _ => Self::Failed,
        }
    }
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub run_id: String,
    pub started_utc: String,
    pub state: RunState,
    pub completed: u64,
    pub total: u64,
    pub logs: VecDeque<String>,
    pub exit_code: Option<u8>,
    pub report_path: Option<String>,
}
struct Active {
    progress: Progress,
    cancel: CancellationToken,
    last_publish: Instant,
}
struct Inner {
    active: Option<Active>,
}
#[derive(Clone)]
pub struct RunManager {
    inner: Arc<Mutex<Inner>>,
    updates: watch::Sender<Option<Progress>>,
}
impl Default for RunManager {
    fn default() -> Self {
        let (updates, _) = watch::channel(None);
        Self {
            inner: Arc::new(Mutex::new(Inner { active: None })),
            updates,
        }
    }
}
impl RunManager {
    pub fn begin(&self, total: u64) -> Result<RunHandle> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| Error::new(ErrorCategory::Internal, "Run lock poisoned"))?;
        if inner
            .active
            .as_ref()
            .is_some_and(|a| a.progress.state.is_active())
        {
            return Err(Error::new(
                ErrorCategory::Busy,
                "A measurement is already active",
            ));
        }
        let progress = Progress {
            run_id: new_run_id(),
            started_utc: now(),
            state: RunState::Running,
            completed: 0,
            total,
            logs: VecDeque::new(),
            exit_code: None,
            report_path: None,
        };
        let cancel = CancellationToken::new();
        let id = progress.run_id.clone();
        self.updates.send_replace(Some(progress.clone()));
        inner.active = Some(Active {
            progress,
            cancel: cancel.clone(),
            last_publish: Instant::now(),
        });
        Ok(RunHandle {
            manager: self.clone(),
            run_id: id,
            cancel,
            finished: false,
        })
    }
    pub fn subscribe(&self) -> watch::Receiver<Option<Progress>> {
        self.updates.subscribe()
    }
    pub fn snapshot(&self) -> Option<Progress> {
        self.inner
            .lock()
            .ok()?
            .active
            .as_ref()
            .map(|a| a.progress.clone())
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| Error::new(ErrorCategory::Internal, "Run lock poisoned"))?;
        let active = inner
            .active
            .as_mut()
            .filter(|a| a.progress.run_id == id)
            .ok_or_else(|| Error::validation("Run ID is not active"))?;
        if active.progress.state.is_active() {
            active.progress.state = RunState::Cancelling;
            active.cancel.cancel();
            self.updates.send_replace(Some(active.progress.clone()));
        }
        Ok(())
    }
    pub fn failure(&self, id: &str, error: &Error) {
        if let Ok(mut inner) = self.inner.lock()
            && let Some(active) = inner.active.as_mut().filter(|a| a.progress.run_id == id)
        {
            active
                .progress
                .logs
                .push_back(bounded_text(&error.message, 256));
            if active.progress.logs.len() > 64 {
                active.progress.logs.pop_front();
            }
            active.progress.state = RunState::Failed;
            active.progress.exit_code = Some(error.category.throughput_exit_code());
            self.updates.send_replace(Some(active.progress.clone()));
        }
    }
    pub fn cancel_active(&self) {
        if let Some(p) = self.snapshot() {
            let _ = self.cancel(&p.run_id);
        }
    }
}
pub struct RunHandle {
    manager: RunManager,
    pub run_id: String,
    pub cancel: CancellationToken,
    finished: bool,
}
impl RunHandle {
    pub fn progress(&self, completed: u64, message: Option<&str>) {
        if let Ok(mut inner) = self.manager.inner.lock()
            && let Some(active) = inner
                .active
                .as_mut()
                .filter(|a| a.progress.run_id == self.run_id)
        {
            active.progress.completed = completed.min(active.progress.total);
            if let Some(message) = message {
                if active.progress.logs.len() == 64 {
                    active.progress.logs.pop_front();
                }
                active.progress.logs.push_back(bounded_text(message, 256));
            }
            if active.last_publish.elapsed() >= Duration::from_millis(100) {
                self.manager
                    .updates
                    .send_replace(Some(active.progress.clone()));
                active.last_publish = Instant::now();
            }
        }
    }
    pub fn finish(mut self, code: u8, path: Option<String>) {
        self.complete(code, path, false);
        self.finished = true;
    }
    pub fn finish_outcome(mut self, code: u8, path: Option<String>, partial: bool) {
        self.complete(code, path, partial);
        self.finished = true;
    }
    fn complete(&self, code: u8, path: Option<String>, partial: bool) {
        if let Ok(mut inner) = self.manager.inner.lock()
            && let Some(active) = inner
                .active
                .as_mut()
                .filter(|a| a.progress.run_id == self.run_id)
        {
            active.progress.state = RunState::from_exit_code(code, partial);
            active.progress.exit_code = Some(code);
            active.progress.report_path = path;
            self.manager
                .updates
                .send_replace(Some(active.progress.clone()));
        }
    }
}
impl Drop for RunHandle {
    fn drop(&mut self) {
        if !self.finished {
            let code = if self.cancel.is_cancelled() {
                exit::CANCELLED
            } else {
                exit::INTERNAL
            };
            self.cancel.cancel();
            self.complete(code, None, false);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exclusive_and_nonce_bound_cancellation() {
        let manager = RunManager::default();
        let run = manager.begin(100).unwrap();
        assert!(manager.begin(1).is_err());
        assert!(manager.cancel("foreign").is_err());
        manager.cancel(&run.run_id).unwrap();
        assert!(run.cancel.is_cancelled());
        run.finish(130, None);
        assert!(manager.begin(1).is_ok());
    }
    #[test]
    fn bounded_logs_and_final_state() {
        let manager = RunManager::default();
        let run = manager.begin(200).unwrap();
        for i in 0..200 {
            run.progress(i, Some(&"é".repeat(500)));
        }
        let p = manager.snapshot().unwrap();
        assert_eq!(p.logs.len(), 64);
        assert!(p.logs.iter().all(|s| s.len() <= 256));
        run.finish(0, None);
        assert_eq!(
            manager.subscribe().borrow().as_ref().unwrap().state,
            RunState::Succeeded
        );
    }

    #[test]
    fn workflow_exit_one_preserves_partial_progress_state() {
        let manager = RunManager::default();
        let run = manager.begin(2).unwrap();
        run.progress(2, None);
        run.finish_outcome(1, Some("fixture/summary.json".into()), true);
        let progress = manager.snapshot().unwrap();
        assert_eq!(progress.exit_code, Some(1));
        assert_eq!(progress.state, RunState::PartialFailure);
    }

    #[test]
    fn wire_enums_match_the_shared_fixture() {
        // The positional matches fail to compile when a variant is added without listing it here.
        let run_states = [
            RunState::Running,
            RunState::Cancelling,
            RunState::Succeeded,
            RunState::PartialFailure,
            RunState::Failed,
            RunState::Cancelled,
        ];
        for (index, state) in run_states.iter().enumerate() {
            let position = match state {
                RunState::Running => 0,
                RunState::Cancelling => 1,
                RunState::Succeeded => 2,
                RunState::PartialFailure => 3,
                RunState::Failed => 4,
                RunState::Cancelled => 5,
            };
            assert_eq!(index, position);
        }
        let categories = [
            ErrorCategory::Validation,
            ErrorCategory::Prerequisite,
            ErrorCategory::Connectivity,
            ErrorCategory::PartialFailure,
            ErrorCategory::TotalFailure,
            ErrorCategory::Internal,
            ErrorCategory::Permission,
            ErrorCategory::Cancelled,
            ErrorCategory::Busy,
        ];
        for (index, category) in categories.iter().enumerate() {
            let position = match category {
                ErrorCategory::Validation => 0,
                ErrorCategory::Prerequisite => 1,
                ErrorCategory::Connectivity => 2,
                ErrorCategory::PartialFailure => 3,
                ErrorCategory::TotalFailure => 4,
                ErrorCategory::Internal => 5,
                ErrorCategory::Permission => 6,
                ErrorCategory::Cancelled => 7,
                ErrorCategory::Busy => 8,
            };
            assert_eq!(index, position);
        }
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/contracts/wire-enums.json"
        ))
        .unwrap();
        assert_eq!(
            serde_json::json!({"run_state":run_states,"error_category":categories}),
            fixture
        );
    }
}
