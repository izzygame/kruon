pub mod core;

use std::sync::Arc;

use core::m3::WorldSnapshot;
use core::runtime::{
    accept_task, cancel_run, collect_artifacts, create_sample_task, create_task, create_workspace,
    enqueue_task_run, get_pause_capability, get_recovery_advice, get_run, latest_task_reviews,
    list_approvals, list_artifacts, list_events, list_queue, list_run_audit, list_runs, list_tasks,
    list_workspaces, probe_connections, record_completion_report, replay_run, restart_follow_up,
    return_task, trust_workspace, untrust_workspace,
};
use core::RuntimeCore;
use core::{AlphaMetricsExportRecord, DiagnosticExportRecord, DiagnosticLocation};
use serde::Serialize;
use tauri::{Emitter, Manager};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct WorldRunSelection {
    run_id: String,
}

#[tauri::command]
// Keep this command async: WebView2 can deadlock when a window is created from a synchronous
// Tauri command on Windows.
async fn open_world_view(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("world") {
        window
            .show()
            .and_then(|_| window.set_focus())
            .map_err(|_| {
                "world_view_unavailable: the display window could not be focused".to_owned()
            })?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(&app, "world", tauri::WebviewUrl::App("index.html".into()))
        .title("kruon world view")
        .inner_size(980.0, 720.0)
        .min_inner_size(720.0, 520.0)
        .resizable(true)
        .build()
        .map(|_| ())
        .map_err(|_| "world_view_unavailable: the display window could not be opened".into())
}

#[tauri::command]
fn get_world_snapshot(state: tauri::State<'_, Arc<RuntimeCore>>) -> Result<WorldSnapshot, String> {
    state
        .world_snapshot()
        .map_err(|_| "world_snapshot_unavailable: local run projection failed".into())
}

#[tauri::command]
fn focus_main_run(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RuntimeCore>>,
    run_id: String,
) -> Result<(), String> {
    state
        .get_run(&run_id)
        .map_err(|_| "not_found: requested local run was not found".to_owned())?;
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| "main_view_unavailable: the control window is not open".to_owned())?;
    main.emit("world-run-selected", WorldRunSelection { run_id })
        .map_err(|_| "main_view_unavailable: run selection could not be delivered".to_owned())?;
    main.show()
        .and_then(|_| main.set_focus())
        .map_err(|_| "main_view_unavailable: the control window could not be focused".to_owned())
}

#[tauri::command]
fn export_diagnostic_bundle(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RuntimeCore>>,
) -> Result<DiagnosticExportRecord, String> {
    let (directory, saved_in) = match app.path().download_dir() {
        Ok(directory) => (directory, DiagnosticLocation::Downloads),
        Err(_) => {
            let directory = app
                .path()
                .app_data_dir()
                .map_err(|_| {
                    "diagnostic_export_failed: no safe local export directory is available"
                        .to_owned()
                })?
                .join("diagnostics");
            (directory, DiagnosticLocation::AppData)
        }
    };
    state
        .export_diagnostic_bundle(&directory, saved_in, probe_connections())
        .map_err(|_| {
            "diagnostic_export_failed: the metadata-only diagnostic bundle could not be written"
                .to_owned()
        })
}

#[tauri::command]
fn export_alpha_metrics(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RuntimeCore>>,
    consented: bool,
) -> Result<AlphaMetricsExportRecord, String> {
    if !consented {
        return Err(
            "consent_required: explicit consent is required for each Alpha metrics export"
                .to_owned(),
        );
    }
    let (directory, saved_in) = match app.path().download_dir() {
        Ok(directory) => (directory, DiagnosticLocation::Downloads),
        Err(_) => {
            let directory = app
                .path()
                .app_data_dir()
                .map_err(|_| {
                    "alpha_metrics_export_failed: no safe local export directory is available"
                        .to_owned()
                })?
                .join("alpha-metrics");
            (directory, DiagnosticLocation::AppData)
        }
    };
    state
        .export_alpha_metrics(&directory, saved_in, probe_connections(), consented)
        .map_err(|_| {
            "alpha_metrics_export_failed: the aggregate local metrics file could not be written"
                .to_owned()
        })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let runtime = RuntimeCore::open(core::release::default_database_path())
        .expect("failed to initialize kruon runtime core");
    tauri::Builder::default()
        .manage(runtime)
        .invoke_handler(tauri::generate_handler![
            cancel_run,
            get_run,
            list_events,
            replay_run,
            probe_connections,
            create_workspace,
            list_workspaces,
            trust_workspace,
            untrust_workspace,
            create_task,
            create_sample_task,
            list_tasks,
            enqueue_task_run,
            list_queue,
            list_runs,
            list_approvals,
            list_artifacts,
            collect_artifacts,
            record_completion_report,
            latest_task_reviews,
            list_run_audit,
            accept_task,
            return_task,
            restart_follow_up,
            get_recovery_advice,
            get_pause_capability,
            export_diagnostic_bundle,
            export_alpha_metrics,
            open_world_view,
            get_world_snapshot,
            focus_main_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
