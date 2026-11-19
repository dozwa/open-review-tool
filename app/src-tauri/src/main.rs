//! Tauri-Hülle des Open Review Tool. Dünne Befehle über `ort-core`; Rechte, Verblindung und alle
//! Prüfungen liegen im Kern. Die Oberfläche liest und schreibt keine Dateien selbst.

mod commands;
mod identity;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::Manager;

pub struct AppState {
    pub data_dir: PathBuf,
    pub identities: identity::IdentityStore,
    pub me: Mutex<Option<ort_core::Identity>>,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // ORT_DATA_DIR erlaubt eine zweite Identität auf demselben Rechner (Entwicklung, Tests).
            let data_dir = match std::env::var_os("ORT_DATA_DIR") {
                Some(dir) => PathBuf::from(dir),
                None => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(data_dir.join("reviews"))?;
            let identities = identity::IdentityStore::new(&data_dir);
            let me = identities.load().unwrap_or_else(|e| {
                eprintln!("Identität nicht geladen: {e}");
                None
            });
            app.manage(AppState { data_dir, identities, me: Mutex::new(me) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::identity_get,
            commands::identity_create,
            commands::identity_export,
            commands::reviews_list,
            commands::templates_list,
            commands::review_create,
            commands::review_detail,
            commands::member_add,
            commands::member_set_role,
            commands::criteria_publish,
            commands::records_import,
            commands::stage_open,
            commands::stage_detail,
            commands::stage_next,
            commands::vote_cast,
            commands::record_flag,
            commands::consensus_record,
            commands::stage_unblind,
            commands::stage_close,
            commands::stage_gate,
            commands::gate_override,
            commands::stage_conflicts,
            commands::stage_summary,
            commands::stage_export_csv,
            commands::package_export,
            commands::return_export,
            commands::bundle_import,
            commands::chain_verify,
            commands::open_url,
        ])
        .run(tauri::generate_context!())
        .expect("Open Review Tool konnte nicht starten");
}
