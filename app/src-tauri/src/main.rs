//! Tauri-Hülle des Open Review Tool. Dünne Befehle über `ort-core`; Rechte, Verblindung und alle
//! Prüfungen liegen im Kern. Die Oberfläche liest und schreibt keine Dateien selbst.

mod ai;
#[cfg(feature = "bridge")]
mod bridge;
mod commands;
mod identity;
mod zotero;

use std::path::PathBuf;
use std::sync::Mutex;

use ort_core::tr;
use tauri::menu::{Menu, MenuItem};
use tauri::{Emitter, Manager};

pub struct AppState {
    pub data_dir: PathBuf,
    pub identities: identity::IdentityStore,
    pub me: Mutex<Option<ort_core::Identity>>,
}

/// Alle Befehle der Oberfläche. Eine Liste für die App und die Entwicklungsbrücke.
fn handler<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        commands::language_set,
        commands::settings_get,
        commands::settings_set,
        commands::project_settings_get,
        commands::project_settings_set,
        commands::storage_reveal,
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
        commands::searches,
        commands::search_publish,
        commands::search_review,
        commands::duplicates_list,
        commands::duplicates_resolve,
        commands::fulltext_overview,
        commands::fulltext_folder_set,
        commands::fulltext_suggestions,
        commands::fulltext_assign,
        commands::fulltext_unavailable,
        commands::fulltext_reset,
        commands::fulltext_verify,
        commands::fulltext_read,
        commands::fulltext_open_external,
        commands::zotero_libraries,
        commands::zotero_link,
        commands::zotero_suggestions,
        commands::zotero_confirm,
        commands::stage_open,
        commands::stage_detail,
        commands::stage_next,
        commands::stage_records,
        commands::stage_record,
        commands::vote_cast,
        commands::record_flag,
        commands::consensus_record,
        commands::stage_unblind,
        commands::stage_close,
        commands::stage_gate,
        commands::gate_override,
        commands::stage_conflicts,
        commands::stage_summary,
        commands::stage_export,
        commands::sources,
        commands::study_link,
        commands::study_dissolve,
        commands::source_type_set,
        commands::report,
        commands::report_export,
        commands::protocol_register,
        commands::guidance_list,
        commands::registration_draft,
        commands::registration_answer,
        commands::registration_export,
        commands::forms,
        commands::form_publish,
        commands::form_confirm,
        commands::form_stage,
        commands::form_entry,
        commands::form_value,
        commands::form_complete,
        commands::form_consensus,
        commands::form_frequencies,
        commands::form_export,
        commands::package_export,
        commands::return_export,
        commands::bundle_import,
        commands::chain_verify,
        commands::open_url,
        commands::highlights,
        commands::highlight_add,
        commands::highlight_update,
        commands::highlight_remove,
        commands::ai_suggest,
        commands::ai_resolve,
        commands::ai_policy_set,
        commands::ai_test,
        commands::ai_default_prompt,
        commands::ai_key_status,
        commands::ai_key_set,
        commands::ai_key_clear,
    ]
}

/// Zustand der App für ein Datenverzeichnis.
fn state(data_dir: PathBuf) -> std::io::Result<AppState> {
    std::fs::create_dir_all(data_dir.join("reviews"))?;
    let identities = identity::IdentityStore::new(&data_dir);
    let me = identities.load().unwrap_or_else(|e| {
        eprintln!("Identität nicht geladen: {e}");
        None
    });
    Ok(AppState { data_dir, identities, me: Mutex::new(me) })
}

fn main() {
    #[cfg(feature = "bridge")]
    if let Some(port) = std::env::var("ORT_BRIDGE_PORT").ok().and_then(|p| p.parse().ok()) {
        return bridge::run(port);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // ORT_DATA_DIR erlaubt eine zweite Identität auf demselben Rechner (Entwicklung, Tests).
            let data_dir = match std::env::var_os("ORT_DATA_DIR") {
                Some(dir) => PathBuf::from(dir),
                None => app.path().app_data_dir()?,
            };
            // Mit eigener Wahl der Sprache steht sie schon vor dem Fenster fest; sonst meldet die Oberfläche die des Systems.
            ort_core::i18n::set_default(&ort_core::settings::Settings::load(&data_dir).language);
            app.manage(state(data_dir)?);

            // „Einstellungen …“ steht wie in jeder Mac-App im Menü der App, mit ⌘,.
            let menu = Menu::default(app.handle())?;
            if let Some(first) = menu.items()?.first().and_then(|item| item.as_submenu()) {
                let label = tr!("Einstellungen …");
                first.insert(&MenuItem::with_id(app, commands::SETTINGS_MENU, label, true, Some("CmdOrCtrl+,"))?, 1)?;
            }
            app.set_menu(menu)?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id() == commands::SETTINGS_MENU {
                let _ = app.emit("open-settings", ());
            }
        })
        .invoke_handler(handler())
        .run(tauri::generate_context!())
        .expect("Open Review Tool konnte nicht starten");
}
