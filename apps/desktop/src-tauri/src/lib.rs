pub mod core;

use std::path::PathBuf;

use core::runtime::{cancel_run, get_run, list_events, replay_run, start_run};
use core::RuntimeCore;

fn default_database_path() -> PathBuf {
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("Library")
        .join("Application Support")
        .join("kruon")
        .join("kruon.sqlite3")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let runtime = RuntimeCore::open(default_database_path())
        .expect("failed to initialize kruon runtime core");
    tauri::Builder::default()
        .manage(runtime)
        .invoke_handler(tauri::generate_handler![
            start_run,
            cancel_run,
            get_run,
            list_events,
            replay_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
