//! Befehle für die Oberfläche. Jede Review liegt als SQLite-Datei unter `reviews/<id>.ortreview`.

use std::path::{Path, PathBuf};

use ort_core::criteria::Criteria;
use ort_core::exchange::{self, Bundle, MergeReport};
use ort_core::forms::{Form, FormEntryView, FormStageView, FormsView, Frequency};
use ort_core::fulltext::{
    Assign, FileProblem, FullTextOverview, Location, Suggestion, ZoteroConfirm, ZoteroLink, ZoteroSuggestion,
};
use ort_core::guidance::{self, Guideline};
use ort_core::protocol::Protocol;
use ort_core::registration::RegistrationDraft;
use ort_core::report::Report;
use ort_core::search::Search;
use ort_core::service::{
    ChainReport, Conflict, DuplicateList, ImportSource, NextRecord, OpenStage, PairDecision, RecordDetail,
    RecordFilter, RecordList, ReviewDetail, ReviewListItem, SearchView, Snapshot, StageDetail, StageView, Summary,
};
use ort_core::sources::SourceList;
use ort_core::store::BatchRow;
use ort_core::workflow::{self, GateResult, TemplateInfo};
use ort_core::{i18n, tr, Identity, PublicIdentity, Review};
use serde::Serialize;
use serde_json::Value;
use tauri::State;
use uuid::Uuid;

use crate::{zotero, AppState};

type Res<T> = Result<T, String>;

fn err(e: impl ToString) -> String {
    e.to_string()
}

fn me(state: &AppState) -> Res<Identity> {
    state.me.lock().map_err(err)?.clone().ok_or_else(|| tr!("Bitte zuerst deinen Namen festlegen."))
}

fn review_path(state: &AppState, review_id: &Uuid) -> PathBuf {
    state.data_dir.join("reviews").join(format!("{review_id}.ortreview"))
}

fn uuid(text: &str) -> Res<Uuid> {
    Uuid::parse_str(text).map_err(|_| tr!("Ungültige ID: {}", text))
}

fn open(state: &AppState, review_id: &str) -> Res<Review> {
    Review::open(&review_path(state, &uuid(review_id)?), me(state)?).map_err(err)
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

/// Führt `run` in der Sprache `lang` aus, etwa für einen Bericht in einer anderen Sprache als der
/// der Oberfläche. Ohne Angabe gilt die Sprache der App.
fn in_language<T>(lang: Option<&str>, run: impl FnOnce() -> T) -> Res<T> {
    match lang.filter(|code| !code.is_empty()) {
        None => Ok(run()),
        Some(code) if i18n::is_known(code) => Ok(i18n::with(code, run)),
        Some(code) => Err(tr!("Unbekannte Sprache: {}", code)),
    }
}

// ---------------------------------------------------------------- Sprache

/// Setzt die Sprache der App: Fehlermeldungen, Texte der Vorlagen, Leitlinien und Bericht.
#[tauri::command]
pub async fn language_set(code: String) -> Res<()> {
    match i18n::set_default(&code) {
        true => Ok(()),
        false => Err(tr!("Unbekannte Sprache: {}", code)),
    }
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

/// Mitgelieferte Vorlagen für die Auswahl des Review-Typs beim Anlegen.
#[tauri::command]
pub async fn templates_list() -> Res<Vec<TemplateInfo>> {
    Ok(workflow::template_infos())
}

#[tauri::command]
pub async fn review_create(state: State<'_, AppState>, title: String, template: Option<String>) -> Res<ReviewListItem> {
    // Die Datei heißt nach der Review-ID; die entsteht erst beim Anlegen, also vorläufiger Name.
    let tmp = state.data_dir.join("reviews").join(format!("neu-{}.tmp", Uuid::new_v4()));
    let review = Review::create_with(&tmp, me(&state)?, &title, template.as_deref()).map_err(err)?;
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
    from: ImportSource,
) -> Res<BatchRow> {
    let bytes = std::fs::read(&path).map_err(err)?;
    let text = String::from_utf8_lossy(&bytes);
    open(&state, &review_id)?.import_file_with(&file_name(Path::new(&path)), &text, &from).map_err(err)
}

// ---------------------------------------------------------------- Suche und Dubletten

#[tauri::command]
pub async fn searches(state: State<'_, AppState>, review_id: String) -> Res<Vec<SearchView>> {
    open(&state, &review_id)?.searches().map_err(err)
}

#[tauri::command]
pub async fn search_publish(state: State<'_, AppState>, review_id: String, data: Value) -> Res<Search> {
    open(&state, &review_id)?.publish_search(&data).map_err(err)
}

#[tauri::command]
pub async fn search_review(
    state: State<'_, AppState>,
    review_id: String,
    search_id: String,
    version: i64,
    reviewed_on: String,
    note: String,
    reviewer: Option<String>,
) -> Res<()> {
    let reviewer = reviewer.filter(|r| !r.is_empty()).map(|r| uuid(&r)).transpose()?;
    open(&state, &review_id)?.review_search(&uuid(&search_id)?, version, &reviewed_on, &note, reviewer).map_err(err)
}

#[tauri::command]
pub async fn duplicates_list(state: State<'_, AppState>, review_id: String) -> Res<DuplicateList> {
    open(&state, &review_id)?.duplicate_candidates().map_err(err)
}

#[tauri::command]
pub async fn duplicates_resolve(state: State<'_, AppState>, review_id: String, pairs: Vec<PairDecision>) -> Res<usize> {
    open(&state, &review_id)?.resolve_duplicates(&pairs).map_err(err)
}

// ---------------------------------------------------------------- Volltexte

#[tauri::command]
pub async fn fulltext_overview(state: State<'_, AppState>, review_id: String) -> Res<FullTextOverview> {
    open(&state, &review_id)?.fulltext_overview().map_err(err)
}

/// Verknüpft den Ordner auf diesem Rechner; ohne Pfad wird die Verknüpfung gelöst.
#[tauri::command]
pub async fn fulltext_folder_set(state: State<'_, AppState>, review_id: String, path: Option<String>) -> Res<()> {
    open(&state, &review_id)?.set_fulltext_folder(path.as_deref().map(Path::new)).map_err(err)
}

#[tauri::command]
pub async fn fulltext_suggestions(state: State<'_, AppState>, review_id: String) -> Res<Vec<Suggestion>> {
    open(&state, &review_id)?.fulltext_suggestions().map_err(err)
}

#[tauri::command]
pub async fn fulltext_assign(state: State<'_, AppState>, review_id: String, list: Vec<Assign>) -> Res<usize> {
    open(&state, &review_id)?.assign_fulltexts(&list).map_err(err)
}

#[tauri::command]
pub async fn fulltext_unavailable(
    state: State<'_, AppState>,
    review_id: String,
    record_id: String,
    reason: String,
) -> Res<()> {
    open(&state, &review_id)?.set_fulltext_unavailable(&uuid(&record_id)?, &reason).map_err(err)
}

#[tauri::command]
pub async fn fulltext_reset(state: State<'_, AppState>, review_id: String, record_id: String) -> Res<()> {
    open(&state, &review_id)?.reset_fulltext(&uuid(&record_id)?).map_err(err)
}

#[tauri::command]
pub async fn fulltext_verify(state: State<'_, AppState>, review_id: String) -> Res<Vec<FileProblem>> {
    open(&state, &review_id)?.verify_fulltexts().map_err(err)
}

/// Pfad eines Volltexts aus Zotero auf diesem Rechner; bei Volltexten aus dem Ordner `None`.
fn zotero_file(review: &Review, record_id: &Uuid) -> Res<Option<PathBuf>> {
    match review.fulltext_location(record_id).map_err(err)? {
        Location::Folder(_) => Ok(None),
        Location::Zotero(key) => {
            let link = review.fulltext_overview().map_err(err)?.zotero;
            let link =
                link.ok_or_else(|| tr!("Der Volltext liegt in Zotero, aber es ist keine Bibliothek verknüpft."))?;
            zotero::file_path(&link, &key).map(Some)
        }
    }
}

/// Bytes des Volltexts für die Vorschau. Der Kern prüft Pfad und Prüfsumme; die Oberfläche bekommt
/// nur den Inhalt, nie einen Pfad zum Selberlesen.
#[tauri::command]
pub async fn fulltext_read(
    state: State<'_, AppState>,
    review_id: String,
    record_id: String,
) -> Res<tauri::ipc::Response> {
    let review = open(&state, &review_id)?;
    let record = uuid(&record_id)?;
    let file = zotero_file(&review, &record)?;
    Ok(tauri::ipc::Response::new(review.read_fulltext(&record, file.as_deref()).map_err(err)?))
}

/// Öffnet den Volltext im Standardprogramm, nach derselben Prüfung wie die Vorschau.
#[tauri::command]
pub async fn fulltext_open_external(state: State<'_, AppState>, review_id: String, record_id: String) -> Res<()> {
    let review = open(&state, &review_id)?;
    let record = uuid(&record_id)?;
    let file = zotero_file(&review, &record)?;
    review.read_fulltext(&record, file.as_deref()).map_err(err)?;
    let path = match (review.fulltext_location(&record).map_err(err)?, file) {
        (Location::Folder(path), _) => path,
        (_, Some(path)) => path,
        _ => return Err(tr!("Die Datei ist nicht zu finden.")),
    };
    std::process::Command::new("open").arg(path).spawn().map_err(err)?;
    Ok(())
}

#[tauri::command]
pub async fn zotero_libraries() -> Res<Vec<zotero::Library>> {
    zotero::libraries()
}

#[tauri::command]
pub async fn zotero_link(state: State<'_, AppState>, review_id: String, link: ZoteroLink) -> Res<()> {
    open(&state, &review_id)?.link_zotero(&link).map_err(err)
}

fn linked_zotero(review: &Review) -> Res<ZoteroLink> {
    let link = review.fulltext_overview().map_err(err)?.zotero;
    link.ok_or_else(|| tr!("Es ist keine Zotero-Bibliothek verknüpft."))
}

/// Liest die verknüpfte Bibliothek und schlägt Zuordnungen für die offenen Einträge vor.
#[tauri::command]
pub async fn zotero_suggestions(state: State<'_, AppState>, review_id: String) -> Res<Vec<ZoteroSuggestion>> {
    let review = open(&state, &review_id)?;
    let items = zotero::items(&linked_zotero(&review)?)?;
    review.zotero_suggestions(&items).map_err(err)
}

/// Bestätigt Zuordnungen. Den Pfad der PDF fragt die App bei Zotero ab; die Oberfläche nennt nur Schlüssel.
#[tauri::command]
pub async fn zotero_confirm(state: State<'_, AppState>, review_id: String, list: Vec<ZoteroConfirm>) -> Res<usize> {
    let review = open(&state, &review_id)?;
    let link = linked_zotero(&review)?;
    let mut confirmed = Vec::new();
    for mut item in list {
        item.file = match &item.attachment_key {
            Some(key) => Some(zotero::file_path(&link, key)?),
            None => None,
        };
        confirmed.push(item);
    }
    review.confirm_zotero(&confirmed).map_err(err)
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
#[allow(clippy::too_many_arguments)]
pub async fn vote_cast(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    record_id: String,
    decision: String,
    primary_reason: Option<String>,
    reasons: Vec<String>,
    note: String,
) -> Res<()> {
    open(&state, &review_id)?
        .cast_vote(&uuid(&stage_id)?, &record_id, &decision, primary_reason.as_deref(), &reasons, &note)
        .map_err(err)
}

#[tauri::command]
pub async fn stage_records(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    filter: RecordFilter,
) -> Res<RecordList> {
    open(&state, &review_id)?.stage_records(&uuid(&stage_id)?, &filter).map_err(err)
}

#[tauri::command]
pub async fn stage_record(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    record_id: String,
) -> Res<RecordDetail> {
    open(&state, &review_id)?.stage_record(&uuid(&stage_id)?, &record_id).map_err(err)
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
    primary_reason: Option<String>,
) -> Res<()> {
    // Die Methode bleibt leer: Der Kern setzt sie nach der Klärungsregel der Stufe.
    open(&state, &review_id)?
        .record_consensus(&uuid(&stage_id)?, &record_id, &decision, "", primary_reason.as_deref(), &[], "")
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

/// Was das Gate beim Schließen ergäbe; `None`, wenn die Station keines hat.
#[tauri::command]
pub async fn stage_gate(state: State<'_, AppState>, review_id: String, stage_id: String) -> Res<Option<GateResult>> {
    open(&state, &review_id)?.gate_preview(&uuid(&stage_id)?).map_err(err)
}

#[tauri::command]
pub async fn gate_override(state: State<'_, AppState>, review_id: String, station: String, reason: String) -> Res<()> {
    open(&state, &review_id)?.override_gate(&station, &reason).map_err(err)
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

// ---------------------------------------------------------------- Bericht

/// Eingeschlossene Quellen mit Bündeln und Quellenart.
#[tauri::command]
pub async fn sources(state: State<'_, AppState>, review_id: String) -> Res<SourceList> {
    open(&state, &review_id)?.sources().map_err(err)
}

/// Bündelt Einträge zu einer Quelle; mit `study_id` wird ein bestehendes Bündel ersetzt.
#[tauri::command]
pub async fn study_link(
    state: State<'_, AppState>,
    review_id: String,
    study_id: Option<String>,
    label: String,
    records: Vec<String>,
    primary: String,
) -> Res<String> {
    let study_id = study_id.as_deref().map(uuid).transpose()?;
    let records = records.iter().map(|id| uuid(id)).collect::<Res<Vec<Uuid>>>()?;
    let review = open(&state, &review_id)?;
    review.link_study(study_id, &label, &records, &uuid(&primary)?).map(|id| id.to_string()).map_err(err)
}

#[tauri::command]
pub async fn study_dissolve(state: State<'_, AppState>, review_id: String, study_id: String) -> Res<()> {
    open(&state, &review_id)?.dissolve_study(&uuid(&study_id)?).map_err(err)
}

#[tauri::command]
pub async fn source_type_set(
    state: State<'_, AppState>,
    review_id: String,
    records: Vec<String>,
    source_type: String,
) -> Res<usize> {
    let records = records.iter().map(|id| uuid(id)).collect::<Res<Vec<Uuid>>>()?;
    open(&state, &review_id)?.set_source_type(&records, &source_type).map_err(err)
}

#[tauri::command]
pub async fn report(state: State<'_, AppState>, review_id: String, lang: Option<String>) -> Res<Report> {
    let review = open(&state, &review_id)?;
    in_language(lang.as_deref(), || review.report())?.map_err(err)
}

/// Hält die Registrierung des Protokolls fest; ein weiterer Eintrag berichtigt die Angaben.
#[tauri::command]
pub async fn protocol_register(
    state: State<'_, AppState>,
    review_id: String,
    registered_on: String,
    registry: String,
    identifier: String,
    note: String,
) -> Res<Protocol> {
    open(&state, &review_id)?.register_protocol(&registered_on, &registry, &identifier, &note).map_err(err)
}

/// Mitgelieferte Leitlinien: Zusammenfassungen je Vorlage mit Verweis auf das Original.
#[tauri::command]
pub async fn guidance_list() -> Res<Vec<Guideline>> {
    Ok(guidance::guidelines())
}

/// Registrierungsformular mit Vorschlägen aus dem Review und den eigenen Texten.
#[tauri::command]
pub async fn registration_draft(
    state: State<'_, AppState>,
    review_id: String,
    lang: Option<String>,
) -> Res<RegistrationDraft> {
    let review = open(&state, &review_id)?;
    in_language(lang.as_deref(), || review.registration_draft())?.map_err(err)
}

/// Hält den eigenen Text zu einem Feld fest; ein leerer Text nimmt ihn zurück.
#[tauri::command]
pub async fn registration_answer(
    state: State<'_, AppState>,
    review_id: String,
    key: String,
    text: String,
    lang: Option<String>,
) -> Res<RegistrationDraft> {
    let review = open(&state, &review_id)?;
    in_language(lang.as_deref(), || review.draft_registration(&key, &text))?.map_err(err)
}

/// Schreibt die Vorbereitung als Markdown: je Feld der Name im Register und der Text zum Kopieren.
/// Mit `lang` in dieser Sprache statt in der der App.
#[tauri::command]
pub async fn registration_export(
    state: State<'_, AppState>,
    review_id: String,
    path: String,
    lang: Option<String>,
) -> Res<()> {
    let review = open(&state, &review_id)?;
    let text = in_language(lang.as_deref(), || review.registration_markdown())?.map_err(err)?;
    std::fs::write(path, text).map_err(err)
}

/// Schreibt eine Datei des Berichts, etwa Diagramm, Tabelle, Methodentext oder Checkliste. Mit `lang`
/// in dieser Sprache statt in der der App.
#[tauri::command]
pub async fn report_export(
    state: State<'_, AppState>,
    review_id: String,
    kind: String,
    path: String,
    lang: Option<String>,
) -> Res<()> {
    let review = open(&state, &review_id)?;
    let file = in_language(lang.as_deref(), || review.report_file(&kind))?.map_err(err)?;
    std::fs::write(path, file.content).map_err(err)
}

// ---------------------------------------------------------------- Formulare

#[tauri::command]
pub async fn forms(state: State<'_, AppState>, review_id: String) -> Res<FormsView> {
    open(&state, &review_id)?.forms().map_err(err)
}

#[tauri::command]
pub async fn form_publish(state: State<'_, AppState>, review_id: String, data: Value) -> Res<Form> {
    open(&state, &review_id)?.publish_form(&data).map_err(err)
}

#[tauri::command]
pub async fn form_confirm(state: State<'_, AppState>, review_id: String, form_id: String) -> Res<()> {
    open(&state, &review_id)?.confirm_form(&uuid(&form_id)?).map_err(err)
}

#[tauri::command]
pub async fn form_stage(state: State<'_, AppState>, review_id: String, stage_id: String) -> Res<FormStageView> {
    open(&state, &review_id)?.form_stage(&uuid(&stage_id)?).map_err(err)
}

#[tauri::command]
pub async fn form_entry(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    record_id: String,
) -> Res<FormEntryView> {
    open(&state, &review_id)?.form_entry(&uuid(&stage_id)?, &record_id).map_err(err)
}

/// Hält den Wert eines Feldes fest. `false`, wenn er schon so dastand.
#[tauri::command]
pub async fn form_value(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    record_id: String,
    field: String,
    value: Value,
) -> Res<bool> {
    open(&state, &review_id)?.record_form_value(&uuid(&stage_id)?, &record_id, &field, &value).map_err(err)
}

#[tauri::command]
pub async fn form_complete(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    record_id: String,
    seconds: Option<i64>,
) -> Res<()> {
    open(&state, &review_id)?.complete_form_entry(&uuid(&stage_id)?, &record_id, seconds).map_err(err)
}

#[tauri::command]
pub async fn form_consensus(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    record_id: String,
    field: String,
    value: Value,
    note: String,
) -> Res<()> {
    open(&state, &review_id)?.record_form_consensus(&uuid(&stage_id)?, &record_id, &field, &value, &note).map_err(err)
}

#[tauri::command]
pub async fn form_frequencies(state: State<'_, AppState>, review_id: String, stage_id: String) -> Res<Vec<Frequency>> {
    open(&state, &review_id)?.form_frequencies(&uuid(&stage_id)?).map_err(err)
}

/// Schreibt die Tabelle (`table`) oder die Häufigkeiten (`frequencies`) einer Formular-Stufe als CSV.
#[tauri::command]
pub async fn form_export(
    state: State<'_, AppState>,
    review_id: String,
    stage_id: String,
    kind: String,
    path: String,
) -> Res<()> {
    let file = open(&state, &review_id)?.form_file(&uuid(&stage_id)?, &kind).map_err(err)?;
    std::fs::write(path, file.content).map_err(err)
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
        return Err(tr!("Nur https-Links werden geöffnet."));
    }
    std::process::Command::new("open").arg(&url).spawn().map_err(err)?;
    Ok(())
}
