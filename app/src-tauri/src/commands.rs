//! Befehle für die Oberfläche. Jede Review liegt als SQLite-Datei unter `reviews/<id>.ortreview`.

use std::path::{Path, PathBuf};

use ort_core::exchange::{self, Bundle, MergeReport};
use ort_core::service::{
    ChainReport, Conflict, Criteria, NextRecord, OpenStage, ReviewDetail, ReviewListItem, Snapshot, StageDetail,
    StageView, Summary,
};
use ort_core::store::BatchRow;
use ort_core::{Identity, PublicIdentity, Review};
use serde::Serialize;
use serde_json::Value;
use tauri::State;
use uuid::Uuid;

use crate::AppState;

type Res<T> = Result<T, String>;

fn err(e: impl ToString) -> String {
    e.to_string()
}

fn me(state: &AppState) -> Res<Identity> {
    state.me.lock().map_err(err)?.clone().ok_or_else(|| "Bitte zuerst deinen Namen festlegen.".to_string())
}

fn review_path(state: &AppState, review_id: &Uuid) -> PathBuf {
    state.data_dir.join("reviews").join(format!("{review_id}.ortreview"))
}

fn uuid(text: &str) -> Res<Uuid> {
    Uuid::parse_str(text).map_err(|_| format!("Ungültige ID: {text}"))
}

fn open(state: &AppState, review_id: &str) -> Res<Review> {
    Review::open(&review_path(state, &uuid(review_id)?), me(state)?).map_err(err)
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

// ---------------------------------------------------------------- Identität

#[tauri::command]
pub async fn identity_get(state: State<'_, AppState>) -> Res<Option<PublicIdentity>> {
    Ok(state.me.lock().map_err(err)?.as_ref().map(Identity::public))
}

#[tauri::command]
pub async fn identity_create(state: State<'_, AppState>, name: String) -> Res<PublicIdentity> {
    let identity = state.identities.create(&name)?;
    let public = identity.public();
    *state.me.lock().map_err(err)? = Some(identity);
    Ok(public)
}

#[tauri::command]
pub async fn identity_export(state: State<'_, AppState>, path: String) -> Res<()> {
    let text = exchange::identity_file(&me(&state)?.public()).map_err(err)?;
    std::fs::write(path, text).map_err(err)
}

// ---------------------------------------------------------------- Reviews

#[tauri::command]
pub async fn reviews_list(state: State<'_, AppState>) -> Res<Vec<ReviewListItem>> {
    let me = me(&state)?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(state.data_dir.join("reviews")).map_err(err)?.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "ortreview") {
            match Review::open(&path, me.clone()).and_then(|r| r.list_item()) {
                Ok(item) => out.push(item),
                Err(e) => eprintln!("{}: {e}", path.display()),
            }
        }
    }
    out.sort_by_key(|r| r.title.to_lowercase());
    Ok(out)
}

#[tauri::command]
pub async fn review_create(state: State<'_, AppState>, title: String) -> Res<ReviewListItem> {
    // Die Datei heißt nach der Review-ID; die entsteht erst beim Anlegen, also vorläufiger Name.
    let tmp = state.data_dir.join("reviews").join(format!("neu-{}.tmp", Uuid::new_v4()));
    let review = Review::create(&tmp, me(&state)?, &title).map_err(err)?;
    let item = review.list_item().map_err(err)?;
    drop(review);
    std::fs::rename(&tmp, review_path(&state, &item.id)).map_err(err)?;
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", tmp.display()));
    }
    Ok(item)
}

#[tauri::command]
pub async fn review_detail(state: State<'_, AppState>, review_id: String) -> Res<ReviewDetail> {
    open(&state, &review_id)?.detail().map_err(err)
}

#[tauri::command]
pub async fn member_add(state: State<'_, AppState>, review_id: String, path: String, role: String) -> Res<String> {
    let text = std::fs::read_to_string(&path).map_err(err)?;
    let person = exchange::read_identity_file(&text).map_err(err)?;
    open(&state, &review_id)?.set_member(&person, &role).map_err(err)?;
    Ok(person.name)
}

#[tauri::command]
pub async fn member_set_role(state: State<'_, AppState>, review_id: String, actor_id: String, role: String) -> Res<()> {
    open(&state, &review_id)?.set_member_role(&uuid(&actor_id)?, &role).map_err(err)
}

#[tauri::command]
pub async fn criteria_publish(state: State<'_, AppState>, review_id: String, data: Value) -> Res<Criteria> {
    open(&state, &review_id)?.publish_criteria(&data).map_err(err)
}

#[tauri::command]
pub async fn records_import(
    state: State<'_, AppState>,
    review_id: String,
    path: String,
    source: String,
) -> Res<BatchRow> {
    let bytes = std::fs::read(&path).map_err(err)?;
    let text = String::from_utf8_lossy(&bytes);
    open(&state, &review_id)?.import_file(&file_name(Path::new(&path)), &text, &source).map_err(err)
}

// ---------------------------------------------------------------- Stufen

#[tauri::command]
pub async fn stage_open(state: State<'_, AppState>, review_id: String, params: OpenStage) -> Res<StageView> {
    open(&state, &review_id)?.open_stage(&params).map_err(err)
}

#[tauri::command]
pub async fn stage_detail(state: State<'_, AppState>, review_id: String, stage_id: String) -> Res<StageDetail> {
    open(&state, &review_id)?.stage_detail(&uuid(&stage_id)?).map_err(err)
}

#[tauri::command]
pub async fn stage_next(state: State<'_, AppState>, review_id: String, stage_id: String) -> Res<NextRecord> {
    open(&state, &review_id)?.next_record(&uuid(&stage_id)?).map_err(err)
}

#[tauri::command]
pub async fn vote_cast(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    record_id: String,
    decision: String,
    reasons: Vec<String>,
    note: String,
) -> Res<()> {
    open(&state, &review_id)?.cast_vote(&uuid(&stage_id)?, &record_id, &decision, &reasons, &note).map_err(err)
}

#[tauri::command]
pub async fn record_flag(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    record_id: String,
    note: String,
) -> Res<()> {
    open(&state, &review_id)?.flag_undecidable(&uuid(&stage_id)?, &record_id, &note).map_err(err)
}

#[tauri::command]
pub async fn consensus_record(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    record_id: String,
    decision: String,
) -> Res<()> {
    open(&state, &review_id)?
        .record_consensus(&uuid(&stage_id)?, &record_id, &decision, "discussion", &[], "")
        .map_err(err)
}

#[tauri::command]
pub async fn stage_unblind(state: State<'_, AppState>, review_id: String, stage_id: String) -> Res<()> {
    open(&state, &review_id)?.unblind_stage(&uuid(&stage_id)?).map_err(err)
}

#[tauri::command]
pub async fn stage_close(state: State<'_, AppState>, review_id: String, stage_id: String) -> Res<Snapshot> {
    open(&state, &review_id)?.close_stage(&uuid(&stage_id)?).map_err(err)
}

#[tauri::command]
pub async fn stage_conflicts(state: State<'_, AppState>, review_id: String, stage_id: String) -> Res<Vec<Conflict>> {
    open(&state, &review_id)?.conflicts(&uuid(&stage_id)?).map_err(err)
}

#[tauri::command]
pub async fn stage_summary(state: State<'_, AppState>, review_id: String, stage_id: String) -> Res<Summary> {
    open(&state, &review_id)?.summary(&uuid(&stage_id)?).map_err(err)
}

#[tauri::command]
pub async fn stage_export_csv(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    path: String,
) -> Res<()> {
    let csv = open(&state, &review_id)?.export_csv(&uuid(&stage_id)?).map_err(err)?;
    std::fs::write(path, csv).map_err(err)
}

// ---------------------------------------------------------------- Austausch

#[tauri::command]
pub async fn package_export(state: State<'_, AppState>, review_id: String, actor_id: String, path: String) -> Res<()> {
    let review = open(&state, &review_id)?;
    let bundle = exchange::export_package(&review, &uuid(&actor_id)?).map_err(err)?;
    std::fs::write(path, bundle.to_json().map_err(err)?).map_err(err)
}

#[tauri::command]
pub async fn return_export(state: State<'_, AppState>, review_id: String, path: String) -> Res<usize> {
    let bundle = exchange::export_return(&open(&state, &review_id)?).map_err(err)?;
    std::fs::write(path, bundle.to_json().map_err(err)?).map_err(err)?;
    Ok(bundle.events.len())
}

#[derive(Serialize)]
pub struct BundleImported {
    review_id: Uuid,
    title: String,
    created: bool,
    report: MergeReport,
}

/// Paket oder Rückmeldung einlesen. Ein Paket zu einem unbekannten Review legt es neu an.
#[tauri::command]
pub async fn bundle_import(state: State<'_, AppState>, path: String) -> Res<BundleImported> {
    let text = std::fs::read_to_string(&path).map_err(err)?;
    let bundle = Bundle::from_json(&text).map_err(err)?;
    let target = review_path(&state, &bundle.review_id);
    let me = me(&state)?;
    let (created, report) = if target.exists() {
        (false, exchange::merge(&Review::open(&target, me).map_err(err)?, &bundle).map_err(err)?)
    } else {
        (true, exchange::create_from_package(&target, me, &bundle).map_err(err)?.1)
    };
    Ok(BundleImported { review_id: bundle.review_id, title: bundle.title, created, report })
}

#[tauri::command]
pub async fn chain_verify(state: State<'_, AppState>, review_id: String) -> Res<ChainReport> {
    open(&state, &review_id)?.verify_chain().map_err(err)
}

/// Öffnet eine DOI oder einen Link im Standardbrowser. Nur https.
#[tauri::command]
pub async fn open_url(url: String) -> Res<()> {
    if !url.starts_with("https://") {
        return Err("Nur https-Links werden geöffnet.".into());
    }
    std::process::Command::new("open").arg(&url).spawn().map_err(err)?;
    Ok(())
}
