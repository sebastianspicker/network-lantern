#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use lantern_contracts::Result;
use lantern_runtime::{self as runtime, manager::RunManager, profiles::ProfileStore, reports};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{Manager, State};

#[derive(Default)]
struct AppState {
    runs: RunManager,
    operation_gate: tokio::sync::Mutex<()>,
    shutting_down: AtomicBool,
    /// A helper registration change is in flight; exiting mid-change can leave a Linux
    /// helper holding its removal reservation until it restarts.
    helper_changing: AtomicBool,
}
/// Clears a flag when dropped, including when the owning future is cancelled.
struct ClearOnDrop<'a>(&'a AtomicBool);
impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
#[tauri::command]
async fn doctor() -> Value {
    runtime::doctor().await
}
#[tauri::command]
fn plan(request: Value) -> Result<Value> {
    let (request, warnings) = runtime::prepare_local_request(&request)?;
    let mut result = runtime::plan_request(&request)?;
    runtime::add_plan_warnings(&mut result, warnings);
    result["resolved_request"] = request;
    Ok(result)
}
#[tauri::command]
fn start_run(request: Value, out: String, state: State<'_, AppState>) -> Result<Value> {
    let _gate = state.operation_gate.try_lock().map_err(|_| busy())?;
    if state.shutting_down.load(Ordering::Acquire) {
        return Err(busy());
    }
    let handle = runtime::reserve_request(&request, &state.runs)?;
    let id = handle.run_id.clone();
    let manager = state.runs.clone();
    let task_id = id.clone();
    let throughput = request.get("capability").and_then(Value::as_str) == Some("throughput");
    tauri::async_runtime::spawn(async move {
        if let Err(error) = runtime::execute_prepared(&request, &PathBuf::from(out), handle).await {
            manager.failure(&task_id, &error, throughput);
        }
    });
    Ok(json!({"run_id":id}))
}
#[tauri::command]
fn cancel_run(run_id: String, state: State<'_, AppState>) -> Result<()> {
    state.runs.cancel(&run_id)
}
#[tauri::command]
fn run_status(state: State<'_, AppState>) -> Value {
    json!(state.runs.snapshot())
}
async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(operation).await.map_err(|e| {
        lantern_contracts::Error::new(lantern_contracts::ErrorCategory::Internal, e.to_string())
    })?
}
#[tauri::command]
async fn profiles_list(store: String) -> Result<Vec<String>> {
    blocking(move || ProfileStore { path: store.into() }.list()).await
}
#[tauri::command]
async fn profiles_get(store: String, name: String) -> Result<Value> {
    blocking(move || ProfileStore { path: store.into() }.get(&name)).await
}
#[tauri::command]
async fn profiles_save(store: String, name: String, parameters: Value) -> Result<()> {
    blocking(move || ProfileStore { path: store.into() }.save(&name, &parameters)).await
}
#[tauri::command]
async fn profiles_save_request(store: String, name: String, request: Value) -> Result<()> {
    blocking(move || {
        let parameters = runtime::profile_parameters(&request)?;
        ProfileStore { path: store.into() }.save(&name, &parameters)
    })
    .await
}
#[tauri::command]
async fn profiles_delete(store: String, name: String) -> Result<bool> {
    blocking(move || ProfileStore { path: store.into() }.delete(&name)).await
}
#[tauri::command]
async fn runs_list(directory: String, offset: usize, limit: usize) -> Result<Value> {
    blocking(move || runtime::list_runs(&PathBuf::from(directory), offset, limit)).await
}
#[tauri::command]
async fn report_read(path: String, offset: usize, limit: usize) -> Result<Value> {
    blocking(move || reports::read_page(&PathBuf::from(path), offset, limit)).await
}
#[tauri::command]
async fn report_compare(baseline: String, current: String) -> Result<Value> {
    blocking(move || {
        Ok(reports::compare(
            &reports::read(&PathBuf::from(baseline))?,
            &reports::read(&PathBuf::from(current))?,
        ))
    })
    .await
}
#[tauri::command]
async fn report_export(path: String, destination: String) -> Result<()> {
    blocking(move || reports::export_file(&PathBuf::from(path), &PathBuf::from(destination))).await
}
#[tauri::command]
async fn helper_status() -> Result<Value> {
    runtime::helper_operation("status").await
}
#[tauri::command]
async fn helper_register(state: State<'_, AppState>) -> Result<Value> {
    helper_change("register", &state).await
}
#[tauri::command]
async fn helper_remove(state: State<'_, AppState>) -> Result<Value> {
    helper_change("remove", &state).await
}
fn busy() -> lantern_contracts::Error {
    lantern_contracts::Error::new(
        lantern_contracts::ErrorCategory::Busy,
        "A measurement or helper registration change is active",
    )
}
async fn helper_change(operation: &str, state: &AppState) -> Result<Value> {
    let _gate = state.operation_gate.try_lock().map_err(|_| busy())?;
    // Flag first, then check shutdown: a close event either sees the change in flight or
    // happened before it started (SeqCst orders the two across threads).
    state.helper_changing.store(true, Ordering::SeqCst);
    let _changing = ClearOnDrop(&state.helper_changing);
    if state.shutting_down.load(Ordering::SeqCst) {
        return Err(busy());
    }
    if state.runs.snapshot().is_some_and(|p| p.state.is_active()) {
        return Err(busy());
    }
    runtime::helper_operation(operation).await
}
fn main() {
    let builder = tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            // A released app starts in "/" (Finder) or its install folder (Explorer); keep the
            // relative defaults (results, .iperf3/profiles.json, config/hosts.conf) per user.
            #[cfg(not(debug_assertions))]
            {
                let data = app.path().app_data_dir()?;
                std::fs::create_dir_all(&data)?;
                std::env::set_current_dir(&data)?;
            }
            #[cfg(debug_assertions)]
            let _ = app;
            Ok(())
        });
    #[cfg(feature = "e2e")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());
    let app = builder
        .invoke_handler(tauri::generate_handler![
            doctor,
            plan,
            start_run,
            cancel_run,
            run_status,
            profiles_list,
            profiles_get,
            profiles_save,
            profiles_save_request,
            profiles_delete,
            runs_list,
            report_read,
            report_compare,
            report_export,
            helper_status,
            helper_register,
            helper_remove
        ])
        .build(tauri::generate_context!())
        .expect("Desktop initialization failed");
    app.run(|app, event| {
        let state = app.state::<AppState>();
        if !measurement_active(&state.runs) && !state.helper_changing.load(Ordering::SeqCst) {
            return;
        }
        match event {
            tauri::RunEvent::WindowEvent {
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } => {
                api.prevent_close();
                begin_shutdown(app, &state);
            }
            tauri::RunEvent::ExitRequested { api, .. } => {
                api.prevent_exit();
                begin_shutdown(app, &state);
            }
            _ => {}
        }
    });
}
fn measurement_active(manager: &RunManager) -> bool {
    manager
        .snapshot()
        .is_some_and(|progress| progress.state.is_active())
}
async fn await_measurement_cleanup(manager: &RunManager, deadline: std::time::Duration) -> bool {
    let mut updates = manager.subscribe();
    tokio::time::timeout(deadline, async {
        while updates
            .borrow()
            .as_ref()
            .is_some_and(|progress| progress.state.is_active())
        {
            if updates.changed().await.is_err() {
                break;
            }
        }
    })
    .await
    .is_ok()
}
/// Waits until `flag` is false; false when the deadline passes first.
async fn await_flag_clear(flag: &AtomicBool, deadline: std::time::Duration) -> bool {
    tokio::time::timeout(deadline, async {
        while flag.load(Ordering::Acquire) {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .is_ok()
}
fn begin_shutdown(app: &tauri::AppHandle, state: &AppState) {
    if state.shutting_down.swap(true, Ordering::SeqCst) {
        return;
    }
    state.runs.cancel_active();
    let handle = app.clone();
    let manager = state.runs.clone();
    tauri::async_runtime::spawn(async move {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let measurement_done = await_measurement_cleanup(
            &manager,
            deadline.saturating_duration_since(tokio::time::Instant::now()),
        )
        .await;
        // A helper change is never cancelled mid-way; it is waited for under the same deadline.
        let helper_done = await_flag_clear(
            &handle.state::<AppState>().helper_changing,
            deadline.saturating_duration_since(tokio::time::Instant::now()),
        )
        .await;
        if measurement_done && helper_done {
            handle.exit(0);
        } else {
            // Keep the window and cancellation state visible while cleanup continues.
            handle
                .state::<AppState>()
                .shutting_down
                .store(false, Ordering::Release);
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn shutdown_waits_for_actual_completion_and_times_out_without_discarding_the_run() {
        let manager = RunManager::default();
        let run = manager.begin(1).unwrap();
        manager.cancel_active();
        assert!(run.cancel.is_cancelled());
        assert!(!await_measurement_cleanup(&manager, std::time::Duration::from_millis(5)).await);
        assert!(measurement_active(&manager));
        run.finish(lantern_contracts::exit::CANCELLED, None);
        assert!(await_measurement_cleanup(&manager, std::time::Duration::from_millis(5)).await);
        assert!(!measurement_active(&manager));
    }
    #[tokio::test]
    async fn shutdown_waits_for_a_helper_change_and_its_guard_clears_the_flag() {
        let state = AppState::default();
        state.helper_changing.store(true, Ordering::Release);
        assert!(
            !await_flag_clear(&state.helper_changing, std::time::Duration::from_millis(5)).await
        );
        drop(ClearOnDrop(&state.helper_changing));
        assert!(
            await_flag_clear(&state.helper_changing, std::time::Duration::from_millis(5)).await
        );
    }
}
