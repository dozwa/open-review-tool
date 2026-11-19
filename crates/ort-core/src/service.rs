//! Fachlogik eines Reviews. Port von `backend/reviews/services.py` samt den Rechte- und
//! Verblindungsprüfungen aus `api.py`: Sie werden hier erzwungen, nicht in der Oberfläche.
//!
//! Alle Entscheidungen werden als Ereignisse angehängt; gelesen wird aus den Projektionen.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rand::seq::index::sample;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::agreement::{self, Alpha, CohenPair, Estimate, Metric, Percent};
use crate::canonical::to_canonical;
use crate::catalog;
use crate::criteria::{check_reasons, Criteria};
use crate::error::{conflict, forbidden, invalid, not_found, Result};
use crate::event::{now_timestamp, parse_public_key, sha256_hex, Event, Identity, PublicIdentity};
use crate::import;
use crate::projection::{FinalDecision, MaybeMode, ReviewState, StageState, Verify};
use crate::store::{AssignmentRow, BatchRow, RecordRow, Store};
use crate::workflow::{self, Facts, GateResult, SchemaElement, StationView, Template, Workflow, WORKFLOW_TYPES};

pub const ROLES: [&str; 4] = ["owner", "admin", "reviewer", "viewer"];
pub const MANAGE_ROLES: [&str; 2] = ["owner", "admin"];
pub const WRITE_ROLES: [&str; 3] = ["owner", "admin", "reviewer"];
const DECISIONS: [&str; 3] = ["include", "exclude", "maybe"];
const MANAGEMENT_TYPES: [&str; 4] =
    ["ReviewCreated", "MemberRoleSet", "CriteriaVersionPublished", "StageInstanceOpened"];
pub const BOOTSTRAP_SEED: u64 = 20261117;
pub const BOOTSTRAP_N: usize = 1000;

// ---------------------------------------------------------------- Ansichten für die Oberfläche

#[derive(Clone, Debug, Serialize)]
pub struct ReviewListItem {
    pub id: Uuid,
    pub title: String,
    pub role: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemberView {
    pub actor_id: Uuid,
    pub name: String,
    pub role: String,
    pub is_me: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct StageView {
    pub id: Uuid,
    pub review_id: Uuid,
    pub name: String,
    pub kind: String,
    pub blind: bool,
    pub min_reviewers: i64,
    pub maybe_as: Option<String>,
    pub criteria_version: Option<String>,
    pub scope: Option<String>,
    pub sample_size: Option<i64>,
    pub seed: Option<i64>,
    pub source_stage: Option<String>,
    pub n_records: i64,
    pub opened_at: String,
    pub unblinded: bool,
    pub closed: bool,
    pub blind_active: bool,
    pub station: Option<String>,
    pub round: i64,
    pub decision_rule: String,
    pub conflict_rule: String,
    pub verify: Option<Verify>,
    pub reason_required: bool,
}

/// Ablauf des Reviews für die Oberfläche: Vorlage, Begriffe und Stationen mit Zustand.
#[derive(Clone, Debug, Serialize)]
pub struct WorkflowView {
    /// `false` bei Reviews ohne veröffentlichte Vorlage; sie laufen im freien Ablauf.
    pub published: bool,
    pub version: i64,
    pub template_key: String,
    pub template_name: String,
    pub review_type: String,
    pub terms: BTreeMap<String, String>,
    /// Elemente, denen Kriterien zugeordnet werden, z. B. Population, Concept, Context.
    pub criteria_schema: Vec<SchemaElement>,
    pub stations: Vec<StationView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewDetail {
    pub id: Uuid,
    pub title: String,
    pub role: String,
    pub me: Uuid,
    pub members: Vec<MemberView>,
    pub n_records: i64,
    pub n_duplicates: i64,
    pub criteria: Criteria,
    pub stages: Vec<StageView>,
    pub imports: Vec<BatchRow>,
    pub workflow: WorkflowView,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecordItem {
    pub id: Uuid,
    pub title: String,
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    pub authors: String,
    pub year: Option<i64>,
    pub journal: String,
    pub doi: String,
    pub url: String,
    pub keywords: String,
    pub duplicate_of: Option<Uuid>,
}

impl From<RecordRow> for RecordItem {
    fn from(r: RecordRow) -> Self {
        RecordItem {
            id: r.id,
            title: r.title,
            abstract_text: r.abstract_text,
            authors: r.authors,
            year: r.year,
            journal: r.journal,
            doi: r.doi,
            url: r.url,
            keywords: r.keywords,
            duplicate_of: r.duplicate_of,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct StageDetail {
    pub stage: StageView,
    pub role: String,
    pub progress: Progress,
    /// Kriterien, mit denen die Stufe geöffnet wurde.
    pub criteria: Criteria,
    /// Es gibt eine neuere Kriterienversion; sie gilt ab der nächsten Stufe.
    pub newer_criteria: bool,
    pub criteria_schema: Vec<SchemaElement>,
    pub members: Vec<MemberView>,
    /// Begriffe der Vorlage, z. B. „Einträge“ oder „Quellen“.
    pub terms: BTreeMap<String, String>,
}

/// Filter der Liste auf der Screening-Seite. Leere Felder filtern nicht.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct RecordFilter {
    /// `open`, `include`, `exclude`, `maybe` (Stimme der Person) oder `conflict`.
    pub decision: Option<String>,
    /// Wessen Stimme der Filter meint; ohne Angabe die eigene. Ohne `decision` bleiben die Einträge,
    /// zu denen diese Person abgestimmt hat.
    pub person: Option<Uuid>,
    pub votes_min: Option<usize>,
    pub votes_max: Option<usize>,
    /// Schlagwort der Kriterien, gesucht in Titel und Abstract.
    pub keyword: Option<String>,
    pub query: Option<String>,
    pub offset: usize,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ListRow {
    pub id: Uuid,
    pub title: String,
    pub year: Option<i64>,
    pub my_decision: Option<String>,
    /// Stimmenzahl, Status und Entscheidung gibt es erst nach dem Entblinden.
    pub n_votes: Option<usize>,
    pub status: Option<String>,
    pub decision: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KeywordCount {
    pub keyword: String,
    /// `include` oder `exclude`.
    pub kind: String,
    pub n: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecordList {
    pub items: Vec<ListRow>,
    /// Treffer des Filters, auch jenseits von `limit`.
    pub total: usize,
    pub keywords: Vec<KeywordCount>,
    /// Filter, die in dieser Stufe gerade gesperrt sind: `conflict`, `person`, `votes`.
    pub locked: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MyVote {
    pub decision: String,
    pub primary_reason: Option<String>,
    pub reasons: Vec<String>,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecordDetail {
    pub record: RecordItem,
    pub my_vote: Option<MyVote>,
}

pub const LIST_LIMIT: usize = 200;

#[derive(Clone, Debug, Serialize)]
pub struct NextRecord {
    pub record: Option<RecordItem>,
    pub progress: Progress,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConflictVote {
    pub name: String,
    pub decision: String,
    pub primary_reason: Option<String>,
    pub reasons: Vec<String>,
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Conflict {
    pub record: RecordItem,
    pub status: String,
    pub votes: Vec<ConflictVote>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReviewerCount {
    pub name: String,
    pub n_votes: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Decisions {
    pub include: usize,
    pub exclude: usize,
    pub maybe: usize,
    pub disagreement: usize,
    pub needs_clarification: usize,
    pub pending: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct AgreementView {
    pub percent: Percent,
    pub percent_ci: Estimate,
    pub krippendorff: Alpha,
    pub alpha_ci: Estimate,
    pub cohen_pairs: Vec<CohenPair>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub stage: StageView,
    pub progress: Progress,
    pub reviewers: Option<Vec<ReviewerCount>>,
    pub decisions: Option<Decisions>,
    pub agreement: Option<AgreementView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub label: String,
    pub hash: String,
    pub n_heads: usize,
    /// Ergebnis des Gates, wenn die Station der Stufe eines hat.
    pub gate: Option<GateResult>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChainReport {
    pub n_events: usize,
    pub n_bad: usize,
    pub problems: Vec<String>,
    pub ok: bool,
}

/// Eingaben für eine neue Stufe, Namen wie im früheren API.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct OpenStage {
    pub name: String,
    pub kind: String,
    pub blind: bool,
    pub min_reviewers: i64,
    pub maybe_as: Option<String>,
    pub scope: String,
    pub sample_size: Option<i64>,
    pub seed: Option<i64>,
    pub source_stage: Option<Uuid>,
    /// Station der Vorlage. Ohne veröffentlichte Vorlage darf sie fehlen.
    pub station: Option<String>,
    /// Ohne Angabe gilt die Regel der Vorlage.
    pub decision_rule: Option<String>,
    pub conflict_rule: Option<String>,
    /// Nur bei `single_verified`; ohne Angabe werden alle Ausschlüsse geprüft.
    pub verify: Option<Verify>,
    /// Ausschluss nur mit Hauptgrund. Ohne Angabe gilt die Station der Vorlage.
    pub reason_required: Option<bool>,
}

impl Default for OpenStage {
    fn default() -> Self {
        OpenStage {
            name: String::new(),
            kind: "title_abstract".into(),
            blind: true,
            min_reviewers: 2,
            maybe_as: None,
            scope: "all".into(),
            sample_size: None,
            seed: None,
            source_stage: None,
            station: None,
            decision_rule: None,
            conflict_rule: None,
            verify: None,
            reason_required: None,
        }
    }
}

// ---------------------------------------------------------------- Review

pub struct Review {
    pub(crate) store: Store,
    pub(crate) me: Identity,
}

fn str_list(value: Option<&Value>) -> Vec<String> {
    let items: Vec<String> = match value {
        Some(Value::String(s)) => s.replace('\n', ",").split(',').map(str::to_string).collect(),
        Some(Value::Array(a)) => {
            a.iter().map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).collect()
        }
        _ => Vec::new(),
    };
    items.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

pub(crate) fn require<'a>(state: &'a ReviewState, me: &Uuid, allowed: &[&str]) -> Result<&'a str> {
    let role = state.role_of(me).ok_or_else(|| forbidden("Du bist nicht Mitglied dieses Reviews."))?;
    if allowed.contains(&role) {
        Ok(role)
    } else {
        Err(forbidden("Dafür fehlt dir die Berechtigung."))
    }
}

/// Gründe ohne Leerzeichen und Leereinträge. Der Hauptgrund steht nicht noch einmal in der Liste.
fn clean_reasons<'a>(primary_reason: Option<&'a str>, reasons: &[String]) -> (Option<&'a str>, Vec<String>) {
    let primary = primary_reason.map(str::trim).filter(|r| !r.is_empty());
    let mut out: Vec<String> = Vec::new();
    for reason in reasons.iter().map(|r| r.trim()).filter(|r| !r.is_empty() && Some(*r) != primary) {
        if !out.iter().any(|r| r == reason) {
            out.push(reason.to_string());
        }
    }
    (primary, out)
}

fn parse_record_id(record_id: &str) -> Result<Uuid> {
    Uuid::parse_str(record_id).map_err(|_| invalid("Ungültige Record-ID."))
}

impl Review {
    /// Legt ein neues Review in einer neuen Datei an. Wer es anlegt, ist Owner.
    /// Ohne Vorlage läuft es im freien Ablauf wie Reviews aus der Zeit vor den Vorlagen.
    pub fn create(path: &Path, me: Identity, title: &str) -> Result<Review> {
        Self::create_with(path, me, title, None)
    }

    /// Legt ein Review an und kopiert die gewählte mitgelieferte Vorlage ins Log.
    pub fn create_with(path: &Path, me: Identity, title: &str, template: Option<&str>) -> Result<Review> {
        if title.trim().is_empty() {
            return Err(invalid("Titel fehlt."));
        }
        if path.exists() {
            return Err(conflict("Für dieses Review gibt es schon eine Datei."));
        }
        let template = Self::builtin(template)?;
        Self::init(Store::open(path)?, me, title, template)
    }

    /// Für Tests und Werkzeuge: Review nur im Speicher.
    pub fn create_in_memory(me: Identity, title: &str) -> Result<Review> {
        Self::init(Store::in_memory()?, me, title, None)
    }

    pub fn create_in_memory_with(me: Identity, title: &str, template: Option<&str>) -> Result<Review> {
        Self::init(Store::in_memory()?, me, title, Self::builtin(template)?)
    }

    fn builtin(key: Option<&str>) -> Result<Option<Template>> {
        key.map(|k| workflow::builtin_template(k).ok_or_else(|| invalid(format!("Unbekannte Vorlage „{k}“."))))
            .transpose()
    }

    fn init(store: Store, me: Identity, title: &str, template: Option<Template>) -> Result<Review> {
        let title = title.trim();
        if title.is_empty() {
            return Err(invalid("Titel fehlt."));
        }
        let review = Review { store, me };
        let review_id = Uuid::new_v4();
        let tx = review.store.conn().unchecked_transaction()?;
        review.store.set_meta("review_id", &review_id.to_string())?;
        let me = &review.me;
        let mut created = json!({"title": title, "owner": me.actor_id.to_string(), "public_key": me.public_key()});
        if let Some(t) = &template {
            created["review_type"] = json!(t.review_type);
        }
        review.append("ReviewCreated", None, created)?;
        review.append(
            "MemberRoleSet",
            None,
            json!({"actor_id": me.actor_id.to_string(), "role": "owner", "public_key": me.public_key()}),
        )?;
        if let Some(t) = &template {
            review.append(
                "WorkflowVersionPublished",
                None,
                json!({"version": 1, "template": serde_json::to_value(t)?, "change_note": ""}),
            )?;
        }
        review.store.set_person(&me.actor_id, &me.name)?;
        tx.commit()?;
        Ok(review)
    }

    pub fn open(path: &Path, me: Identity) -> Result<Review> {
        if !path.exists() {
            return Err(not_found("Review-Datei nicht gefunden."));
        }
        let store = Store::open(path)?;
        store.review_id()?;
        Ok(Review { store, me })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn me(&self) -> &Identity {
        &self.me
    }

    pub fn id(&self) -> Result<Uuid> {
        self.store.review_id()
    }

    pub fn events(&self) -> Result<Vec<Event>> {
        self.store.events()
    }

    pub fn state(&self) -> Result<ReviewState> {
        Ok(ReviewState::build(&self.store.events_of_types(&MANAGEMENT_TYPES)?))
    }

    /// Einziger Weg der Fachlogik ins Log: prüft den Payload gegen das Register und hängt dann an.
    fn append(&self, event_type: &str, stage_id: Option<Uuid>, payload: Value) -> Result<Event> {
        let version = catalog::spec(event_type).map_or(1, |s| s.version);
        catalog::validate(event_type, version, stage_id.as_ref(), &payload).map_err(invalid)?;
        self.store.append(&self.me, event_type, stage_id, payload)
    }

    fn stage_state(&self, stage_id: &Uuid) -> Result<StageState> {
        StageState::build(stage_id, &self.store.stage_events(stage_id)?)
            .ok_or_else(|| not_found("Stufe nicht gefunden."))
    }

    // ------------------------------------------------------------ Ablauf

    /// Vorlage, Gate-Ergebnisse und Übersteuerungen. Ohne veröffentlichte Vorlage der freie Ablauf.
    pub fn workflow(&self) -> Result<Workflow> {
        Ok(Workflow::build(&self.store.events_of_types(&WORKFLOW_TYPES)?))
    }

    fn stage_states(&self, state: &ReviewState) -> Result<Vec<StageState>> {
        state.stage_ids.iter().map(|id| self.stage_state(id)).collect()
    }

    fn facts(&self, state: &ReviewState) -> Result<Facts> {
        let n_records: i64 =
            self.store.conn().query_row("SELECT count(*) FROM record WHERE duplicate_of IS NULL", [], |r| r.get(0))?;
        Ok(Facts {
            criteria_published: Criteria::from_payload(state.criteria.as_ref()).version > 0,
            n_writers: state.members.values().filter(|m| WRITE_ROLES.contains(&m.role.as_str())).count() as i64,
            n_records,
        })
    }

    /// Stationen der Vorlage mit Zustand, Runden und Gates.
    pub fn stations(&self) -> Result<Vec<StationView>> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &ROLES)?;
        Ok(self.workflow()?.stations(&self.facts(&state)?, &self.stage_states(&state)?))
    }

    pub fn station(&self, key: &str) -> Result<StationView> {
        self.stations()?.into_iter().find(|s| s.key == key).ok_or_else(|| not_found("Diese Station gibt es nicht."))
    }

    /// Eingaben für die nächste Stufe einer Station, wie die Vorlage sie vorsieht.
    pub fn stage_defaults(&self, station: &str) -> Result<OpenStage> {
        let view = self.station(station)?;
        let d = view.defaults.ok_or_else(|| invalid("Diese Station arbeitet nicht mit Stufen."))?;
        Ok(OpenStage {
            name: d.name,
            kind: view.key.clone(),
            blind: d.rules.blind.value && d.rules.decision_rule.value != "single_verified",
            min_reviewers: d.rules.min_reviewers.value,
            maybe_as: d.rules.maybe_as.value,
            scope: d.scope,
            sample_size: d.sample_size,
            seed: None,
            source_stage: d.source_stage,
            station: Some(view.key),
            decision_rule: Some(d.rules.decision_rule.value),
            conflict_rule: Some(d.rules.conflict_rule.value),
            verify: None,
            reason_required: Some(d.reason_required),
        })
    }

    /// Veröffentlicht eine geänderte Vorlage als neue Version. Stufen behalten die Regeln, mit denen
    /// sie geöffnet wurden.
    pub fn publish_workflow(&self, template: &Value, change_note: &str) -> Result<i64> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        Template::parse(template).map_err(invalid)?;
        let current = self.workflow()?;
        if current.published() && change_note.trim().is_empty() {
            return Err(invalid("Eine geänderte Vorlage braucht eine Begründung."));
        }
        let version = current.version + 1;
        self.append(
            "WorkflowVersionPublished",
            None,
            json!({"version": version, "template": template, "change_note": change_note.trim()}),
        )?;
        Ok(version)
    }

    /// Schaltet eine Station mit Begründung frei, obwohl ihr Gate nicht erfüllt ist.
    pub fn override_gate(&self, station: &str, reason: &str) -> Result<()> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        let reason = reason.trim();
        if reason.is_empty() {
            return Err(invalid("Übersteuern braucht eine Begründung."));
        }
        let view = self.station(station)?;
        if !view.blocking {
            return Err(invalid(format!("Die Station „{}“ sperrt nichts.", view.label)));
        }
        if view.state == "done" {
            return Err(conflict(format!("Die Station „{}“ ist bereits abgeschlossen.", view.label)));
        }
        if view.rounds.iter().any(|r| !r.closed) {
            return Err(conflict("Erst die laufende Stufe dieser Station schließen."));
        }
        let round = view.rounds.last().map_or(0, |r| r.round);
        self.append("GateOverridden", None, json!({"station": view.key, "round": round, "reason": reason}))?;
        Ok(())
    }

    /// Ergebnis des Gates, wie es beim Schließen ins Log käme. Schreibt nichts.
    pub fn gate_preview(&self, stage_id: &Uuid) -> Result<Option<GateResult>> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if stage.blind_active() {
            return Err(forbidden("Die Stufe ist verblindet. Das Gate ist erst nach dem Entblinden einsehbar."));
        }
        self.evaluate_gate(&self.workflow()?, &stage)
    }

    fn evaluate_gate(&self, workflow: &Workflow, stage: &StageState) -> Result<Option<GateResult>> {
        let Some(station) = workflow.station_of(stage).filter(|s| !s.gate.is_empty()) else { return Ok(None) };
        let assigned: Vec<String> =
            self.store.assignments(Some(&stage.config.id))?.into_iter().map(|a| a.record_id.to_string()).collect();
        Ok(Some(workflow::evaluate_gate(station, stage, &assigned)))
    }

    pub fn name_of(&self, actor: &Uuid) -> Result<String> {
        Ok(self.store.person_name(actor)?.unwrap_or_else(|| format!("Unbekannt ({})", &actor.to_string()[..8])))
    }

    pub fn list_item(&self) -> Result<ReviewListItem> {
        let state = self.state()?;
        Ok(ReviewListItem {
            id: self.id()?,
            title: state.title.clone(),
            role: state.role_of(&self.me.actor_id).map(str::to_string),
        })
    }

    // ------------------------------------------------------------ Mitglieder

    pub fn members(&self) -> Result<Vec<MemberView>> {
        let state = self.state()?;
        let mut out = Vec::new();
        for m in state.members.values() {
            out.push(MemberView {
                actor_id: m.actor_id,
                name: self.name_of(&m.actor_id)?,
                role: m.role.clone(),
                is_me: m.actor_id == self.me.actor_id,
            });
        }
        out.sort_by_key(|m| m.name.to_lowercase());
        Ok(out)
    }

    /// Fügt eine Person über ihre Identitätsdatei hinzu oder ändert ihre Rolle.
    pub fn set_member(&self, person: &PublicIdentity, role: &str) -> Result<()> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        if !ROLES.contains(&role) {
            return Err(invalid("Unbekannte Rolle."));
        }
        parse_public_key(&person.public_key)?;
        if let Some(current) = state.members.get(&person.actor_id) {
            if current.public_key != person.public_key {
                return Err(conflict("Diese Person ist schon mit einem anderen Schlüssel Mitglied."));
            }
            let owners = state.members.values().filter(|m| m.role == "owner").count();
            if current.role == "owner" && role != "owner" && owners == 1 {
                return Err(invalid("Ein Review braucht mindestens einen Owner."));
            }
        }
        let tx = self.store.conn().unchecked_transaction()?;
        self.append(
            "MemberRoleSet",
            None,
            json!({"actor_id": person.actor_id.to_string(), "role": role, "public_key": person.public_key}),
        )?;
        if !person.name.trim().is_empty() {
            self.store.set_person(&person.actor_id, person.name.trim())?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_member_role(&self, actor_id: &Uuid, role: &str) -> Result<()> {
        let state = self.state()?;
        let member = state.members.get(actor_id).ok_or_else(|| not_found("Diese Person ist kein Mitglied."))?;
        let person = PublicIdentity {
            actor_id: *actor_id,
            name: self.name_of(actor_id)?,
            public_key: member.public_key.clone(),
        };
        self.set_member(&person, role)
    }

    // ------------------------------------------------------------ Kriterien

    pub fn current_criteria(&self) -> Result<Criteria> {
        Ok(Criteria::from_payload(self.state()?.criteria.as_ref()))
    }

    pub fn publish_criteria(&self, data: &Value) -> Result<Criteria> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        let previous = Criteria::from_payload(state.criteria.as_ref());
        let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_string();
        let schema = self.workflow()?.template.criteria_schema;
        let mut items = Vec::new();
        for item in data.get("items").and_then(Value::as_array).into_iter().flatten() {
            let element = text(item, "element");
            if !element.is_empty() && !schema.iter().any(|s| s.key == element) {
                return Err(invalid(format!("Das Element „{element}“ gibt es im Kriterienschema der Vorlage nicht.")));
            }
            items.push(json!({
                "code": text(item, "code"), "element": element, "kind": text(item, "kind"),
                "text": text(item, "text"), "note": text(item, "note"),
            }));
        }
        if items.is_empty() {
            return Err(invalid("Mindestens ein Kriterium ist nötig."));
        }
        let change_note = text(data, "change_note");
        if previous.version > 0 && change_note.is_empty() {
            return Err(invalid("Was hat sich an den Kriterien geändert? Die Angabe fehlt."));
        }
        let payload = json!({
            "version": previous.version + 1,
            "previous_version": if previous.version > 0 { Some(previous.version) } else { None },
            "items": items,
            "guidance": text(data, "guidance"),
            "include_keywords": str_list(data.get("include_keywords")),
            "exclude_keywords": str_list(data.get("exclude_keywords")),
            "change_note": change_note,
        });
        self.append("CriteriaVersionPublished", None, payload.clone())?;
        Ok(Criteria::from_payload(Some(&payload)))
    }

    // ------------------------------------------------------------ Import

    /// Liest eine RIS- oder BibTeX-Datei ein. Fehler in der Datei landen im Importstatus.
    pub fn import_file(&self, filename: &str, text: &str, source: &str) -> Result<BatchRow> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        let format = import::detect_format(filename, text)?;
        let mut batch = BatchRow {
            id: Uuid::new_v4(),
            filename: filename.chars().take(255).collect(),
            format: format.into(),
            source: source.chars().take(255).collect(),
            sha256: sha256_hex(text.as_bytes()),
            status: "running".into(),
            n_records: 0,
            n_duplicates: 0,
            error: String::new(),
            created_by: self.me.actor_id,
            created_at: now_timestamp(),
        };
        let items = match import::parse(format, text) {
            Ok(items) if !items.is_empty() => items,
            Ok(_) => return self.fail_batch(batch, "Die Datei enthält keine Einträge."),
            Err(e) => return self.fail_batch(batch, &e.to_string()),
        };
        let tx = self.store.conn().unchecked_transaction()?;
        self.store.insert_batch(&batch)?;
        let mut known: BTreeMap<String, Uuid> = self
            .store
            .records_where("WHERE duplicate_of IS NULL", &[])?
            .into_iter()
            .map(|r| (r.dedup_key, r.id))
            .collect();
        let mut ids = Vec::new();
        let mut n_duplicates = 0;
        for item in items {
            let id = Uuid::new_v4();
            let key = match import::dedup_key(&item) {
                k if k.is_empty() => format!("id:{id}"),
                k => k,
            };
            let original = known.get(&key).copied();
            match original {
                Some(_) => n_duplicates += 1,
                None => {
                    known.insert(key.clone(), id);
                }
            }
            self.store.insert_record(&RecordRow {
                id,
                batch_id: batch.id,
                title: item.title,
                abstract_text: item.abstract_text,
                authors: item.authors,
                year: item.year,
                journal: item.journal,
                doi: item.doi,
                pmid: item.pmid,
                url: item.url,
                keywords: item.keywords,
                raw: item.raw,
                dedup_key: key,
                duplicate_of: original,
            })?;
            ids.push(id.to_string());
        }
        ids.sort();
        batch.status = "done".into();
        batch.n_records = ids.len() as i64;
        batch.n_duplicates = n_duplicates;
        self.append(
            "RecordsImported",
            None,
            json!({
                "batch_id": batch.id.to_string(), "source": batch.source, "filename": batch.filename,
                "file_sha256": batch.sha256, "format": batch.format, "n_records": batch.n_records,
                "n_duplicates": n_duplicates, "record_ids_sha256": sha256_hex(ids.join("\n").as_bytes()),
                "records_sha256": self.store.records_digest(&batch.id)?,
                "created_at": batch.created_at,
            }),
        )?;
        self.store.finish_batch(&batch)?;
        tx.commit()?;
        Ok(batch)
    }

    fn fail_batch(&self, mut batch: BatchRow, error: &str) -> Result<BatchRow> {
        batch.status = "failed".into();
        batch.error = error.chars().take(2000).collect();
        self.store.insert_batch(&batch)?;
        Ok(batch)
    }

    pub fn records(&self, query: Option<&str>, include_duplicates: bool) -> Result<Vec<RecordItem>> {
        let rows = self.store.records_where("ORDER BY title, id", &[])?;
        let q = query.map(str::to_lowercase).filter(|q| !q.is_empty());
        Ok(rows
            .into_iter()
            .filter(|r| include_duplicates || r.duplicate_of.is_none())
            .filter(|r| {
                q.as_ref().is_none_or(|q| {
                    r.title.to_lowercase().contains(q)
                        || r.abstract_text.to_lowercase().contains(q)
                        || r.authors.to_lowercase().contains(q)
                })
            })
            .map(RecordItem::from)
            .collect())
    }

    // ------------------------------------------------------------ Überblick

    pub fn detail(&self) -> Result<ReviewDetail> {
        let state = self.state()?;
        let role =
            state.role_of(&self.me.actor_id).ok_or_else(|| forbidden("Du bist nicht Mitglied dieses Reviews."))?;
        let count = |sql: &str| -> Result<i64> { Ok(self.store.conn().query_row(sql, [], |r| r.get(0))?) };
        Ok(ReviewDetail {
            id: self.id()?,
            title: state.title.clone(),
            role: role.to_string(),
            me: self.me.actor_id,
            members: self.members()?,
            n_records: count("SELECT count(*) FROM record")?,
            n_duplicates: count("SELECT count(*) FROM record WHERE duplicate_of IS NOT NULL")?,
            criteria: Criteria::from_payload(state.criteria.as_ref()),
            stages: self.list_stages()?,
            imports: self.store.batches()?,
            workflow: self.workflow_view(&state)?,
        })
    }

    fn workflow_view(&self, state: &ReviewState) -> Result<WorkflowView> {
        let workflow = self.workflow()?;
        let stations = workflow.stations(&self.facts(state)?, &self.stage_states(state)?);
        Ok(WorkflowView {
            published: workflow.published(),
            version: workflow.version,
            template_key: workflow.template.key,
            template_name: workflow.template.name,
            review_type: workflow.template.review_type,
            terms: workflow.template.terms,
            criteria_schema: workflow.template.criteria_schema,
            stations,
        })
    }

    // ------------------------------------------------------------ Stufen

    pub fn list_stages(&self) -> Result<Vec<StageView>> {
        let state = self.state()?;
        state.stage_ids.iter().map(|id| Ok(stage_view(&self.stage_state(id)?))).collect()
    }

    pub fn get_stage(&self, stage_id: &Uuid) -> Result<StageView> {
        Ok(stage_view(&self.stage_state(stage_id)?))
    }

    pub fn open_stage(&self, params: &OpenStage) -> Result<StageView> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        if !matches!(params.maybe_as.as_deref(), None | Some("include") | Some("exclude")) {
            return Err(invalid("maybe_as muss leer, include oder exclude sein."));
        }
        let name = params.name.trim();
        if name.is_empty() {
            return Err(invalid("Name der Stufe fehlt."));
        }
        if params.min_reviewers < 1 {
            return Err(invalid("Mindestens eine Stimme je Record ist nötig."));
        }

        // Station, Sperren und Regeln aus der Vorlage
        let workflow = self.workflow()?;
        let stages = self.stage_states(&state)?;
        let views = workflow.stations(&self.facts(&state)?, &stages);
        let key = match (&params.station, workflow.published()) {
            (Some(key), _) => key.clone(),
            (None, false) => workflow.template.default_station().map(|s| s.key.clone()).unwrap_or_default(),
            (None, true) => return Err(invalid("Zu welcher Station gehört die Stufe? Die Angabe fehlt.")),
        };
        let station = workflow.template.station(&key).ok_or_else(|| not_found("Diese Station gibt es nicht."))?;
        if station.kind != "screening" {
            return Err(invalid(format!("Die Station „{}“ arbeitet nicht mit Screening-Stufen.", station.label)));
        }
        let view = views.iter().find(|v| v.key == key).ok_or_else(|| not_found("Diese Station gibt es nicht."))?;
        if let Some(blocker) = view.blocked_by.first() {
            return Err(conflict(format!(
                "Die Station „{}“ ist gesperrt: „{}“ ist noch nicht abgeschlossen. Die Leitung kann das mit einer Begründung übersteuern.",
                station.label, blocker.label
            )));
        }
        if !station.free_rounds && view.rounds.iter().any(|r| !r.closed) {
            return Err(conflict(format!("In der Station „{}“ ist noch eine Stufe offen.", station.label)));
        }
        let round = view.rounds.last().map_or(0, |r| r.round) + 1;
        let rules = workflow.template.rules_for(station);
        let decision_rule = params.decision_rule.clone().unwrap_or_else(|| rules.decision_rule.value.clone());
        let conflict_rule = params.conflict_rule.clone().unwrap_or_else(|| rules.conflict_rule.value.clone());
        if !workflow::DECISION_RULES.contains(&decision_rule.as_str()) {
            return Err(invalid("Unbekannte Auswahlregel."));
        }
        if !workflow::CONFLICT_RULES.contains(&conflict_rule.as_str()) {
            return Err(invalid("Unbekannte Klärungsregel."));
        }
        let fixed = |editable: bool, same: bool, what: &str| -> Result<()> {
            if editable || same {
                Ok(())
            } else {
                Err(invalid(format!("Die Vorlage legt {what} für die Station „{}“ fest.", station.label)))
            }
        };
        fixed(rules.decision_rule.editable, decision_rule == rules.decision_rule.value, "die Auswahlregel")?;
        fixed(rules.conflict_rule.editable, conflict_rule == rules.conflict_rule.value, "die Klärungsregel")?;
        fixed(rules.min_reviewers.editable, params.min_reviewers == rules.min_reviewers.value, "die Zahl der Stimmen")?;
        fixed(rules.maybe_as.editable, params.maybe_as == rules.maybe_as.value, "die Maybe-Regel")?;
        fixed(rules.blind.editable, params.blind == rules.blind.value, "die Verblindung")?;
        let verify = if decision_rule == "single_verified" {
            if params.blind {
                return Err(invalid(
                    "Bei „eine Stimme mit Prüfung“ sieht die zweite Person die Ausschlüsse. Die Stufe kann deshalb nicht verblindet sein.",
                ));
            }
            let mut verify = params.verify.clone().unwrap_or(Verify { scope: "all".into(), percent: None, seed: None });
            match verify.scope.as_str() {
                "all" => (verify.percent, verify.seed) = (None, None),
                "sample" => {
                    if !verify.percent.is_some_and(|p| (1..=100).contains(&p)) {
                        return Err(invalid("Der Anteil der Stichprobe muss zwischen 1 und 100 Prozent liegen."));
                    }
                    verify.seed = Some(verify.seed.unwrap_or_else(|| rand::thread_rng().gen_range(1..2_147_483_647)));
                }
                _ => return Err(invalid("Die Prüfung gilt für alle Ausschlüsse oder für eine Stichprobe.")),
            }
            Some(verify)
        } else {
            None
        };
        let criteria = Criteria::from_payload(state.criteria.as_ref());
        let reason_required = params.reason_required.unwrap_or(station.reason_required);
        if station.reason_required && !reason_required {
            return Err(invalid(format!(
                "Die Vorlage verlangt in der Station „{}“ für jeden Ausschluss einen Hauptgrund.",
                station.label
            )));
        }
        if reason_required && criteria.reason_codes().is_empty() {
            return Err(invalid(
                "In dieser Stufe braucht jeder Ausschluss einen Hauptgrund. Dafür fehlen Ausschlusskriterien mit Kürzel.",
            ));
        }
        let source_stage = params.source_stage.or(view.defaults.as_ref().and_then(|d| d.source_stage));

        let mut seed = params.seed;
        let ids: Vec<String> = match params.scope.as_str() {
            "from_stage" => {
                let source_id = source_stage.ok_or_else(|| invalid("Quellstufe fehlt."))?;
                let source = self.stage_state(&source_id)?;
                source
                    .final_decisions()
                    .into_iter()
                    .filter(|(_, d)| d.decision.as_deref() == Some("include"))
                    .map(|(id, _)| id)
                    .collect()
            }
            "all" | "sample" => {
                let mut ids: Vec<String> = self
                    .store
                    .records_where("WHERE duplicate_of IS NULL", &[])?
                    .into_iter()
                    .map(|r| r.id.to_string())
                    .collect();
                ids.sort();
                if params.scope == "sample" {
                    let s = seed.unwrap_or_else(|| rand::thread_rng().gen_range(1..2_147_483_647));
                    seed = Some(s);
                    let amount = (params.sample_size.unwrap_or(25).max(1) as usize).min(ids.len());
                    let mut rng = ChaCha8Rng::seed_from_u64(s as u64);
                    let picked = sample(&mut rng, ids.len(), amount).into_vec();
                    ids = picked.into_iter().map(|i| ids[i].clone()).collect();
                }
                ids
            }
            _ => return Err(invalid("Unbekannter Umfang der Stufe.")),
        };
        if ids.is_empty() {
            return Err(invalid("Für diese Stufe gibt es keine Records."));
        }
        let stage_id = Uuid::new_v4();
        let tx = self.store.conn().unchecked_transaction()?;
        for (position, id) in ids.iter().enumerate() {
            self.store.insert_assignment(&AssignmentRow {
                stage_instance_id: stage_id,
                record_id: parse_record_id(id)?,
                position: position as i64,
            })?;
        }
        let sample_scope = params.scope == "sample";
        self.append(
            "StageInstanceOpened",
            Some(stage_id),
            json!({
                "name": name, "kind": params.kind, "blind": params.blind, "min_reviewers": params.min_reviewers,
                "maybe_as": params.maybe_as, "criteria_version": criteria.version.to_string(),
                "scope": params.scope, "sample_size": if sample_scope { Some(ids.len()) } else { None },
                "seed": if sample_scope { seed } else { None },
                "source_stage": if params.scope == "from_stage" { source_stage.map(|s| s.to_string()) } else { None },
                "n_records": ids.len(), "record_ids_sha256": self.store.assignment_digest(&stage_id)?,
                "station": key, "round": round, "decision_rule": decision_rule, "conflict_rule": conflict_rule,
                "verify": verify, "reason_required": reason_required,
            }),
        )?;
        tx.commit()?;
        self.get_stage(&stage_id)
    }

    pub fn unblind_stage(&self, stage_id: &Uuid) -> Result<()> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if !stage.blind_active() {
            return Err(conflict("Die Stufe ist nicht verblindet."));
        }
        self.append("StageUnblinded", Some(*stage_id), json!({}))?;
        Ok(())
    }

    /// Schließt die Stufe und hält den Stand aller Ketten als Snapshot fest.
    pub fn close_stage(&self, stage_id: &Uuid) -> Result<Snapshot> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if stage.closed {
            return Err(conflict("Die Stufe ist bereits geschlossen."));
        }
        let label = format!("Abschluss {}", stage.config.name);
        let workflow = self.workflow()?;
        let gate = self.evaluate_gate(&workflow, &stage)?;
        let tx = self.store.conn().unchecked_transaction()?;
        self.append("StageInstanceClosed", Some(*stage_id), json!({"label": label}))?;
        if let Some(result) = &gate {
            let mut payload = serde_json::to_value(result)?;
            payload["station"] = json!(workflow.station_of(&stage).map(|s| s.key.clone()));
            payload["round"] = json!(stage.config.round);
            self.append("GateEvaluated", Some(*stage_id), payload)?;
        }
        let heads: Vec<Value> = self
            .store
            .heads()?
            .into_iter()
            .map(|(actor, stream, seq, hash)| json!({"actor_id": actor.to_string(), "stream": stream, "seq": seq, "hash": hash}))
            .collect();
        let hash = sha256_hex(to_canonical(&Value::Array(heads.clone()))?.as_bytes());
        let n_heads = heads.len();
        self.append("SnapshotTaken", Some(*stage_id), json!({"label": label, "heads": heads, "hash": hash}))?;
        tx.commit()?;
        Ok(Snapshot { label, hash, n_heads, gate })
    }

    // ------------------------------------------------------------ Abstimmen

    fn require_assigned(&self, stage_id: &Uuid, record_id: &str) -> Result<Uuid> {
        let id = parse_record_id(record_id)?;
        if !self.store.is_assigned(stage_id, &id)? {
            return Err(not_found("Der Record gehört nicht zu dieser Stufe."));
        }
        Ok(id)
    }

    fn my_votes(&self, stage: &StageState) -> BTreeMap<String, String> {
        stage
            .votes
            .iter()
            .filter(|((_, actor), _)| *actor == self.me.actor_id)
            .map(|((record, _), v)| (record.clone(), v.decision.clone()))
            .collect()
    }

    pub fn my_stage_votes(&self, stage_id: &Uuid) -> Result<BTreeMap<String, String>> {
        Ok(self.my_votes(&self.stage_state(stage_id)?))
    }

    pub fn cast_vote(
        &self,
        stage_id: &Uuid,
        record_id: &str,
        decision: &str,
        primary_reason: Option<&str>,
        reasons: &[String],
        note: &str,
    ) -> Result<()> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &WRITE_ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if stage.closed {
            return Err(conflict("Die Stufe ist geschlossen."));
        }
        if !DECISIONS.contains(&decision) {
            return Err(invalid("Entscheidung muss include, exclude oder maybe sein."));
        }
        let record = self.require_assigned(stage_id, record_id)?.to_string();
        let previous = self.my_votes(&stage).get(&record).cloned();
        let (primary_reason, reasons) = clean_reasons(primary_reason, reasons);
        check_reasons(
            &stage_criteria(&state, &stage),
            stage.config.reason_required,
            decision,
            primary_reason,
            &reasons,
        )
        .map_err(invalid)?;
        let mut payload = json!({"record_id": record, "decision": decision, "reasons": reasons, "note": note});
        if let Some(code) = primary_reason {
            payload["primary_reason"] = json!(code);
        }
        let event_type = match previous {
            Some(previous) => {
                payload["previous"] = json!(previous);
                "VoteRevised"
            }
            None => "VoteCast",
        };
        self.append(event_type, Some(*stage_id), payload)?;
        Ok(())
    }

    pub fn flag_undecidable(&self, stage_id: &Uuid, record_id: &str, note: &str) -> Result<()> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &WRITE_ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if stage.closed {
            return Err(conflict("Die Stufe ist geschlossen."));
        }
        let record = self.require_assigned(stage_id, record_id)?.to_string();
        self.append("RecordFlaggedUndecidable", Some(*stage_id), json!({"record_id": record, "note": note}))?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_consensus(
        &self,
        stage_id: &Uuid,
        record_id: &str,
        decision: &str,
        method: &str,
        primary_reason: Option<&str>,
        reasons: &[String],
        note: &str,
    ) -> Result<()> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &WRITE_ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if stage.closed {
            return Err(conflict("Die Stufe ist geschlossen."));
        }
        if stage.blind_active() {
            return Err(forbidden("Die Stufe ist verblindet. Ein Konsens ist erst nach dem Entblinden möglich."));
        }
        if !matches!(decision, "include" | "exclude") {
            return Err(invalid("Ein Konsens muss include oder exclude sein."));
        }
        let record = self.require_assigned(stage_id, record_id)?.to_string();
        let third_person = stage.config.conflict_rule == "third_person";
        if third_person && stage.votes.contains_key(&(record.clone(), self.me.actor_id)) {
            return Err(forbidden(
                "In dieser Stufe entscheidet eine dritte Person. Du hast zu diesem Eintrag selbst abgestimmt.",
            ));
        }
        let method = match method {
            "" if third_person => "third_person",
            "" => "discussion",
            other => other,
        };
        let (primary_reason, reasons) = clean_reasons(primary_reason, reasons);
        check_reasons(
            &stage_criteria(&state, &stage),
            stage.config.reason_required,
            decision,
            primary_reason,
            &reasons,
        )
        .map_err(invalid)?;
        let mut payload =
            json!({"record_id": record, "decision": decision, "method": method, "reasons": reasons, "note": note});
        if let Some(code) = primary_reason {
            payload["primary_reason"] = json!(code);
        }
        self.append("ConsensusRecorded", Some(*stage_id), payload)?;
        Ok(())
    }

    fn record_item(&self, id: &str) -> Result<RecordItem> {
        let id = parse_record_id(id)?;
        self.store.record(&id)?.map(RecordItem::from).ok_or_else(|| not_found("Record nicht gefunden."))
    }

    /// Fortschritt der eigenen Arbeit. Bei `liberal` und `single_verified` zählt nur, was von dieser
    /// Person noch gebraucht wird; bei `unanimous` sind das alle Einträge der Stufe.
    fn progress(&self, stage: &StageState) -> Result<Progress> {
        let done = self.my_votes(stage).len();
        if stage.config.decision_rule == "unanimous" {
            return Ok(Progress { done, total: stage.config.n_records.max(0) as usize });
        }
        let open = self
            .store
            .assignments(Some(&stage.config.id))?
            .into_iter()
            .filter(|a| stage.wants_vote_from(&a.record_id.to_string(), &self.me.actor_id))
            .count();
        Ok(Progress { done, total: done + open })
    }

    pub fn stage_detail(&self, stage_id: &Uuid) -> Result<StageDetail> {
        let state = self.state()?;
        let role = require(&state, &self.me.actor_id, &ROLES)?.to_string();
        let stage = self.stage_state(stage_id)?;
        let criteria = stage_criteria(&state, &stage);
        let template = self.workflow()?.template;
        Ok(StageDetail {
            progress: self.progress(&stage)?,
            stage: stage_view(&stage),
            role,
            newer_criteria: Criteria::from_payload(state.criteria.as_ref()).version > criteria.version,
            criteria,
            criteria_schema: template.criteria_schema,
            members: self.members()?,
            terms: template.terms,
        })
    }

    /// Nächster Record, zu dem die eigene Stimme gebraucht wird, in der für alle gleichen Reihenfolge.
    pub fn next_record(&self, stage_id: &Uuid) -> Result<NextRecord> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &ROLES)?;
        let stage = self.stage_state(stage_id)?;
        let me = self.me.actor_id;
        let next = self
            .store
            .assignments(Some(stage_id))?
            .into_iter()
            .find(|a| stage.wants_vote_from(&a.record_id.to_string(), &me));
        Ok(NextRecord {
            record: next.map(|a| self.record_item(&a.record_id.to_string())).transpose()?,
            progress: self.progress(&stage)?,
        })
    }

    /// Einträge der Stufe für die Liste der Screening-Seite. Solange die Stufe verblindet ist, sind
    /// die Filter nach Konflikt, nach einer anderen Person und nach der Zahl der Stimmen gesperrt.
    pub fn stage_records(&self, stage_id: &Uuid, filter: &RecordFilter) -> Result<RecordList> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &ROLES)?;
        let stage = self.stage_state(stage_id)?;
        let blind = stage.blind_active();
        let me = self.me.actor_id;
        let person = filter.person.unwrap_or(me);
        let wanted = filter.decision.as_deref().filter(|d| !d.is_empty());
        if !matches!(wanted, None | Some("open" | "include" | "exclude" | "maybe" | "conflict")) {
            return Err(invalid("Unbekannter Filter für die Entscheidung."));
        }
        if blind {
            let locked = if wanted == Some("conflict") {
                Some("Konflikte")
            } else if person != me {
                Some("Stimmen anderer Personen")
            } else if filter.votes_min.is_some() || filter.votes_max.is_some() {
                Some("Die Zahl der Stimmen")
            } else {
                None
            };
            if let Some(what) = locked {
                return Err(forbidden(format!(
                    "Die Stufe ist verblindet. {what} sind erst nach dem Entblinden sichtbar."
                )));
            }
        }
        let finals = if blind { BTreeMap::new() } else { stage.final_decisions() };
        let mut n_votes: BTreeMap<&str, usize> = BTreeMap::new();
        for (record, _) in stage.votes.keys() {
            *n_votes.entry(record.as_str()).or_default() += 1;
        }
        let criteria = stage_criteria(&state, &stage);
        let mut keywords: Vec<KeywordCount> = Vec::new();
        for (kind, list) in [("include", &criteria.include_keywords), ("exclude", &criteria.exclude_keywords)] {
            for keyword in list {
                keywords.push(KeywordCount { keyword: keyword.clone(), kind: kind.to_string(), n: 0 });
            }
        }
        let needles: Vec<String> = keywords.iter().map(|k| k.keyword.to_lowercase()).collect();
        let keyword = filter.keyword.as_deref().map(str::trim).filter(|k| !k.is_empty()).map(str::to_lowercase);
        let query = filter.query.as_deref().map(str::trim).filter(|q| !q.is_empty()).map(str::to_lowercase);
        let limit = filter.limit.unwrap_or(LIST_LIMIT);
        let mut items = Vec::new();
        let mut total = 0;
        for r in self.store.stage_records(stage_id)? {
            let id = r.id.to_string();
            let text = format!("{}\n{}", r.title, r.abstract_text).to_lowercase();
            for (count, needle) in keywords.iter_mut().zip(&needles) {
                if text.contains(needle.as_str()) {
                    count.n += 1;
                }
            }
            let votes = n_votes.get(id.as_str()).copied().unwrap_or(0);
            let status = finals.get(&id).map(|f| f.status);
            let vote_of = |actor: Uuid| stage.votes.get(&(id.clone(), actor)).map(|v| v.decision.as_str());
            let matches = match wanted {
                None => filter.person.is_none_or(|p| vote_of(p).is_some()),
                Some("open") => vote_of(person).is_none(),
                Some("conflict") => matches!(status, Some("disagreement" | "needs_clarification")),
                Some(decision) => vote_of(person) == Some(decision),
            } && filter.votes_min.is_none_or(|n| votes >= n)
                && filter.votes_max.is_none_or(|n| votes <= n)
                && keyword.as_ref().is_none_or(|k| text.contains(k.as_str()))
                && query.as_ref().is_none_or(|q| text.contains(q.as_str()));
            if !matches {
                continue;
            }
            total += 1;
            if total > filter.offset && items.len() < limit {
                items.push(ListRow {
                    id: r.id,
                    my_decision: vote_of(me).map(str::to_string),
                    n_votes: (!blind).then_some(votes),
                    status: if blind { None } else { Some(status.unwrap_or("pending").to_string()) },
                    decision: finals.get(&id).and_then(|f| f.decision.clone()),
                    title: r.title,
                    year: r.year,
                });
            }
        }
        let locked = if blind { vec!["conflict", "person", "votes"] } else { Vec::new() };
        Ok(RecordList { items, total, keywords, locked })
    }

    /// Ein Eintrag der Stufe mit der eigenen Stimme, etwa um sie zu ändern.
    pub fn stage_record(&self, stage_id: &Uuid, record_id: &str) -> Result<RecordDetail> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &ROLES)?;
        let stage = self.stage_state(stage_id)?;
        let record = self.require_assigned(stage_id, record_id)?.to_string();
        let my_vote = stage.votes.get(&(record.clone(), self.me.actor_id)).map(|v| MyVote {
            decision: v.decision.clone(),
            primary_reason: v.primary_reason.clone(),
            reasons: v.reasons.clone(),
            note: v.note.clone(),
        });
        Ok(RecordDetail { record: self.record_item(&record)?, my_vote })
    }

    // ------------------------------------------------------------ Auswertung

    /// Records mit Widerspruch oder Markierung, samt Einzelvoten. Erst nach dem Entblinden.
    pub fn conflicts(&self, stage_id: &Uuid) -> Result<Vec<Conflict>> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if stage.blind_active() {
            return Err(forbidden("Die Stufe ist verblindet. Konflikte sind erst nach dem Entblinden sichtbar."));
        }
        let mut out = Vec::new();
        for (record, decision) in stage.final_decisions() {
            if !matches!(decision.status, "disagreement" | "needs_clarification") {
                continue;
            }
            let mut votes = Vec::new();
            for ((r, actor), v) in &stage.votes {
                if *r == record {
                    votes.push(ConflictVote {
                        name: self.name_of(actor)?,
                        decision: v.decision.clone(),
                        primary_reason: v.primary_reason.clone(),
                        reasons: v.reasons.clone(),
                        note: v.note.clone(),
                    });
                }
            }
            votes.sort_by(|a, b| a.name.cmp(&b.name));
            out.push(Conflict { record: self.record_item(&record)?, status: decision.status.to_string(), votes });
        }
        Ok(out)
    }

    pub fn summary(&self, stage_id: &Uuid) -> Result<Summary> {
        let state = self.state()?;
        let role = require(&state, &self.me.actor_id, &ROLES)?;
        let stage = self.stage_state(stage_id)?;
        let mut out = Summary {
            stage: stage_view(&stage),
            progress: self.progress(&stage)?,
            reviewers: None,
            decisions: None,
            agreement: None,
        };
        if MANAGE_ROLES.contains(&role) || !stage.blind_active() {
            let mut counts: BTreeMap<Uuid, usize> = BTreeMap::new();
            for (_, actor) in stage.votes.keys() {
                *counts.entry(*actor).or_default() += 1;
            }
            let mut reviewers = Vec::new();
            for (actor, n_votes) in counts {
                reviewers.push(ReviewerCount { name: self.name_of(&actor)?, n_votes });
            }
            reviewers.sort_by(|a, b| a.name.cmp(&b.name));
            out.reviewers = Some(reviewers);
        }
        if stage.blind_active() {
            return Ok(out);
        }
        let finals = stage.final_decisions();
        let count = |f: &dyn Fn(&FinalDecision) -> bool| finals.values().filter(|d| f(d)).count();
        let pending =
            count(&|d| d.status == "pending") + (stage.config.n_records.max(0) as usize).saturating_sub(finals.len());
        out.decisions = Some(Decisions {
            include: count(&|d| d.decision.as_deref() == Some("include")),
            exclude: count(&|d| d.decision.as_deref() == Some("exclude")),
            maybe: count(&|d| d.decision.as_deref() == Some("maybe")),
            disagreement: count(&|d| d.status == "disagreement"),
            needs_clarification: count(&|d| d.status == "needs_clarification"),
            pending,
        });
        let votes = stage.stage_votes(&MaybeMode::Stage);
        let mut pairs = agreement::cohen_pairs(&votes);
        for p in &mut pairs {
            p.rater_a = self.name_of(&Uuid::parse_str(&p.rater_a).unwrap_or_default())?;
            p.rater_b = self.name_of(&Uuid::parse_str(&p.rater_b).unwrap_or_default())?;
        }
        out.agreement = Some(AgreementView {
            percent: agreement::percent_agreement(&votes),
            percent_ci: agreement::bootstrap(&votes, &Metric::Percent, BOOTSTRAP_N, BOOTSTRAP_SEED, 0.95),
            krippendorff: agreement::krippendorff_alpha(&votes),
            alpha_ci: agreement::bootstrap(&votes, &Metric::Alpha, BOOTSTRAP_N, BOOTSTRAP_SEED, 0.95),
            cohen_pairs: pairs,
        });
        Ok(out)
    }

    pub fn final_decisions(&self, stage_id: &Uuid) -> Result<BTreeMap<String, FinalDecision>> {
        let stage = self.stage_state(stage_id)?;
        if stage.blind_active() {
            return Err(forbidden("Die Stufe ist verblindet."));
        }
        Ok(stage.final_decisions())
    }

    pub fn export_csv(&self, stage_id: &Uuid) -> Result<String> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if stage.blind_active() {
            return Err(forbidden("Die Stufe ist verblindet. Der Export ist erst nach dem Entblinden möglich."));
        }
        let finals = stage.final_decisions();
        let mut out = String::from("record_id,title,authors,year,journal,doi,status,decision,n_votes\n");
        for a in self.store.assignments(Some(stage_id))? {
            let Some(r) = self.store.record(&a.record_id)? else { continue };
            let d = finals.get(&r.id.to_string());
            let fields = [
                r.id.to_string(),
                r.title,
                r.authors,
                r.year.map(|y| y.to_string()).unwrap_or_default(),
                r.journal,
                r.doi,
                d.map_or("pending", |d| d.status).to_string(),
                d.and_then(|d| d.decision.clone()).unwrap_or_default(),
                d.map_or(0, |d| d.n_votes).to_string(),
            ];
            out.push_str(&fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
            out.push('\n');
        }
        Ok(out)
    }

    /// Prüft alle Ketten: lückenlose Folge, Vorgänger-Hash, eigener Hash, Signatur.
    pub fn verify_chain(&self) -> Result<ChainReport> {
        let events = self.store.events()?;
        let state = ReviewState::build(&events);
        let mut by_chain: BTreeMap<(Uuid, String), Vec<&Event>> = BTreeMap::new();
        for e in &events {
            by_chain.entry((e.actor_id, e.stream.clone())).or_default().push(e);
        }
        let mut problems = Vec::new();
        let mut bad = BTreeSet::new();
        for ((actor, stream), mut chain) in by_chain {
            chain.sort_by_key(|e| e.seq);
            let key = state.members.get(&actor).and_then(|m| parse_public_key(&m.public_key).ok());
            let mut prev: Option<&Event> = None;
            for e in chain {
                let expected_seq = prev.map_or(1, |p| p.seq + 1);
                let mut problem = None;
                if e.seq != expected_seq {
                    problem = Some(format!(
                        "Lücke in der Kette {stream} von {}: erwartet {expected_seq}, gefunden {}.",
                        &actor.to_string()[..8],
                        e.seq
                    ));
                } else if e.prev_hash != prev.map(|p| p.hash.clone()) {
                    problem = Some(format!("Vorgänger-Hash passt nicht bei {} ({}).", e.event_id, e.event_type));
                } else if let Some(key) = &key {
                    if let Err(err) = e.verify(key) {
                        problem = Some(err.to_string());
                    }
                } else {
                    problem = Some(format!("Kein Schlüssel für die Person {}.", &actor.to_string()[..8]));
                }
                if let Some(p) = problem {
                    bad.insert(e.event_id);
                    if problems.len() < 20 {
                        problems.push(p);
                    }
                }
                prev = Some(e);
            }
        }
        Ok(ChainReport { n_events: events.len(), n_bad: bad.len(), ok: bad.is_empty(), problems })
    }
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Kriterien, mit denen die Stufe geöffnet wurde. Fehlt die Version im Log, gelten die neuesten.
pub(crate) fn stage_criteria(state: &ReviewState, stage: &StageState) -> Criteria {
    let version = stage.config.criteria_version.as_deref().and_then(|v| v.parse::<i64>().ok());
    let payload = version.and_then(|v| state.criteria_versions.get(&v)).or(state.criteria.as_ref());
    Criteria::from_payload(payload)
}

fn stage_view(s: &StageState) -> StageView {
    let c = &s.config;
    StageView {
        id: c.id,
        review_id: c.review_id,
        name: c.name.clone(),
        kind: c.kind.clone(),
        blind: c.blind,
        min_reviewers: c.min_reviewers,
        maybe_as: c.maybe_as.clone(),
        criteria_version: c.criteria_version.clone(),
        scope: c.scope.clone(),
        sample_size: c.sample_size,
        seed: c.seed,
        source_stage: c.source_stage.clone(),
        n_records: c.n_records,
        opened_at: c.opened_at.clone(),
        unblinded: s.unblinded,
        closed: s.closed,
        blind_active: s.blind_active(),
        station: c.station.clone(),
        round: c.round,
        decision_rule: c.decision_rule.clone(),
        conflict_rule: c.conflict_rule.clone(),
        verify: c.verify.clone(),
        reason_required: c.reason_required,
    }
}
