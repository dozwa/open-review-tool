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
use crate::duplicates::{self, Resolutions};
use crate::error::{conflict, forbidden, invalid, not_found, Result};
use crate::event::{now_timestamp, parse_public_key, sha256_hex, Event, Identity, PublicIdentity};
use crate::forms::PILOT_PER_TYPE;
use crate::fulltext::FullText;
use crate::import;
use crate::projection::{FinalDecision, MaybeMode, ReviewState, StageState, SuggestionInfo, Verify};
use crate::search::{Search, SearchVersion, Searches, SEARCH_TYPES};
use crate::sources;
use crate::store::{AssignmentRow, BatchRow, RecordRow, Store};
use crate::tr;
use crate::workflow::{self, Facts, GateResult, SchemaElement, StationView, Template, Workflow, WORKFLOW_TYPES};

pub const ROLES: [&str; 4] = ["owner", "admin", "reviewer", "viewer"];
pub const MANAGE_ROLES: [&str; 2] = ["owner", "admin"];
pub const WRITE_ROLES: [&str; 3] = ["owner", "admin", "reviewer"];
const DECISIONS: [&str; 3] = ["include", "exclude", "maybe"];
const MANAGEMENT_TYPES: [&str; 5] =
    ["ReviewCreated", "MemberRoleSet", "CriteriaVersionPublished", "StageInstanceOpened", "AiPolicySet"];
pub const BOOTSTRAP_SEED: u64 = 20261117;
pub const BOOTSTRAP_N: usize = 1000;

// ---------------------------------------------------------------- Ansichten für die Oberfläche

#[derive(Clone, Debug, Serialize)]
pub struct ReviewListItem {
    pub id: Uuid,
    pub title: String,
    pub role: Option<String>,
    /// Kennfarbe aus den Einstellungen des Reviews auf diesem Rechner; leer heißt ohne.
    pub color: String,
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
    pub form_id: Option<Uuid>,
    pub form_version: Option<i64>,
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
    pub imports: Vec<ImportView>,
    pub workflow: WorkflowView,
    /// Ob die Leitung KI-Vorschläge in diesem Review erlaubt.
    pub ai_allowed: bool,
}

/// Import mit Herkunft und Suchversion aus dem Log.
#[derive(Clone, Debug, Serialize)]
pub struct ImportView {
    #[serde(flatten)]
    pub batch: BatchRow,
    /// `database`, `register`, `citation`, `other`; leer bei Importen aus der Zeit vor der Angabe.
    pub origin: String,
    pub search_id: Option<Uuid>,
    pub search_version: Option<i64>,
}

/// Woher die Treffer eines Imports stammen. Mit Suche kommen Quelle und Herkunft von dort.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ImportSource {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub search_id: Option<Uuid>,
    /// Ohne Angabe die neueste Version der Suche.
    #[serde(default)]
    pub search_version: Option<i64>,
}

/// Was ein `RecordsImported` über seinen Import sagt.
#[derive(Clone, Debug)]
struct ImportEvent {
    batch_id: Option<Uuid>,
    source: String,
    origin: String,
    search: Option<(Uuid, i64)>,
    n_records: i64,
    n_duplicates: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchVersionView {
    #[serde(flatten)]
    pub version: SearchVersion,
    pub n_imports: i64,
    /// Importierte Einträge dieser Version, mit exakten Dubletten.
    pub n_records: i64,
    pub n_duplicates: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchView {
    pub id: Uuid,
    pub versions: Vec<SearchVersionView>,
}

/// Treffer einer Quelle, allein aus dem Log gezählt.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SourceCount {
    pub source: String,
    pub origin: String,
    pub search_id: Option<Uuid>,
    pub search_version: Option<i64>,
    pub n_imports: i64,
    pub n_records: i64,
    /// Beim Import exakt erkannte Dubletten.
    pub n_duplicates: i64,
}

/// Zahlen für den Kopf des Flussdiagramms: Treffer je Quelle und entfernte Dubletten.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImportSummary {
    pub sources: Vec<SourceCount>,
    pub n_records: i64,
    pub n_exact_duplicates: i64,
    /// Als Dublette entschiedene Paare (`DuplicatesResolved`).
    pub n_resolved_duplicates: i64,
    pub n_unique: i64,
}

/// Entscheidung über ein Paar möglicher Dubletten.
#[derive(Clone, Debug, Deserialize)]
pub struct PairDecision {
    pub a: Uuid,
    pub b: Uuid,
    /// `duplicate` oder `distinct`.
    pub decision: String,
    /// Bei `duplicate`: der Eintrag, der bleibt.
    #[serde(default)]
    pub keep: Option<Uuid>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DuplicatePair {
    pub a: RecordItem,
    pub b: RecordItem,
    /// Quelle des Imports, aus dem der Eintrag stammt.
    pub a_source: String,
    pub b_source: String,
    pub similarity: i64,
    /// Vorschlag, welcher Eintrag bleibt: der vollständigere.
    pub keep: Uuid,
}

#[derive(Clone, Debug, Serialize)]
pub struct DuplicateList {
    /// Offene Paare, höchstens [`PAIR_LIMIT`].
    pub pairs: Vec<DuplicatePair>,
    pub total: usize,
    pub n_exact: i64,
    pub n_resolved: i64,
    pub n_distinct: usize,
}

pub const PAIR_LIMIT: usize = 200;

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
    /// Ob die Leitung KI-Vorschläge in diesem Review erlaubt.
    pub ai_allowed: bool,
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
    /// Ob die Stufe von mir noch eine Stimme zu diesem Eintrag braucht (nach ihrer Auswahlregel,
    /// z. B. bei `single_verified` nur zu Ausschlüssen, die noch niemand geprüft hat).
    pub wants_my_vote: bool,
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
    /// Stand des Volltexts, falls einer beschafft oder als nicht beschaffbar vermerkt ist.
    pub fulltext: Option<FullText>,
    /// Der letzte eigene KI-Vorschlag zu diesem Eintrag.
    pub my_suggestion: Option<SuggestionInfo>,
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
    /// Was die KI dieser Person vorgeschlagen hatte, falls sie einen Vorschlag eingeholt hat.
    pub ai: Option<String>,
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
    /// Formular einer Formular-Stufe. Ohne Angabe das einzige mit dem Zweck der Station.
    pub form_id: Option<Uuid>,
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
            form_id: None,
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
    let role = state.role_of(me).ok_or_else(|| forbidden(tr!("Du bist nicht Mitglied dieses Reviews.")))?;
    if allowed.contains(&role) {
        Ok(role)
    } else {
        Err(forbidden(tr!("Dafür fehlt dir die Berechtigung.")))
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
    Uuid::parse_str(record_id).map_err(|_| invalid(tr!("Ungültige Record-ID.")))
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
            return Err(invalid(tr!("Titel fehlt.")));
        }
        if path.exists() {
            return Err(conflict(tr!("Für dieses Review gibt es schon eine Datei.")));
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
        key.map(|k| workflow::builtin_template(k).ok_or_else(|| invalid(tr!("Unbekannte Vorlage „{}“.", k))))
            .transpose()
    }

    fn init(store: Store, me: Identity, title: &str, template: Option<Template>) -> Result<Review> {
        let title = title.trim();
        if title.is_empty() {
            return Err(invalid(tr!("Titel fehlt.")));
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
            return Err(not_found(tr!("Review-Datei nicht gefunden.")));
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
    pub(crate) fn append(&self, event_type: &str, stage_id: Option<Uuid>, payload: Value) -> Result<Event> {
        let version = catalog::spec(event_type).map_or(1, |s| s.version);
        catalog::validate(event_type, version, stage_id.as_ref(), &payload).map_err(invalid)?;
        self.store.append(&self.me, event_type, stage_id, payload)
    }

    pub(crate) fn stage_state(&self, stage_id: &Uuid) -> Result<StageState> {
        StageState::build(stage_id, &self.store.stage_events(stage_id)?)
            .ok_or_else(|| not_found(tr!("Stufe nicht gefunden.")))
    }

    // ------------------------------------------------------------ Ablauf

    /// Vorlage, Gate-Ergebnisse und Übersteuerungen. Ohne veröffentlichte Vorlage der freie Ablauf.
    pub fn workflow(&self) -> Result<Workflow> {
        Ok(Workflow::build(&self.store.events_of_types(&WORKFLOW_TYPES)?))
    }

    pub(crate) fn stage_states(&self, state: &ReviewState) -> Result<Vec<StageState>> {
        state.stage_ids.iter().map(|id| self.stage_state(id)).collect()
    }

    fn facts(&self, state: &ReviewState, workflow: &Workflow, stages: &[StageState]) -> Result<Facts> {
        let needed = self.fulltext_scope(workflow, stages)?;
        let (fulltext_retrieved, fulltext_unavailable) = self.fulltexts()?.counts(&needed);
        let resolutions = self.resolutions()?;
        let (total, exact, resolved) = self.duplicate_counts(&resolutions)?;
        // Der Abgleich liest alle Titel; nur rechnen, wenn die Vorlage danach fragt.
        let asks =
            workflow.template.stations.iter().any(|s| s.gate.iter().any(|g| g.check == "no_unresolved_duplicates"));
        let open_duplicates = match asks {
            true => duplicates::candidates(&self.store.record_keys()?, &resolutions).len() as i64,
            false => 0,
        };
        let asks = |check: &str| workflow.template.stations.iter().any(|s| s.gate.iter().any(|g| g.check == check));
        let n_citation_searches = match asks("citation_search_documented") {
            true => {
                let searches = self.search_state()?;
                let named = searches.list.iter().filter(|s| s.versions.last().is_some_and(|v| v.origin == "citation"));
                let loose = self.import_events()?.into_iter().filter(|e| e.origin == "citation" && e.search.is_none());
                (named.count() + loose.count()) as i64
            }
            false => 0,
        };
        let unassigned_records = match asks("all_records_assigned") {
            true => self.unassigned(workflow, stages, &resolutions)?,
            false => 0,
        };
        let (n_sources, sources_without_type) = match asks("source_types_set") {
            true => {
                let units = sources::units(&self.selection()?.included, &self.studies()?);
                (units.len() as i64, units.iter().filter(|u| u.source_type.is_empty()).count() as i64)
            }
            false => (0, 0),
        };
        let mut sources_without_round = BTreeMap::new();
        for station in &workflow.template.stations {
            if station.kind == "form" && station.gate.iter().any(|g| g.check == "all_sources_assigned") {
                sources_without_round
                    .insert(station.key.clone(), self.sources_without_round(workflow, stages, station)?);
            }
        }
        Ok(Facts {
            sources_without_round,
            forms_confirmed: self.form_defs()?.confirmed_purposes(),
            n_citation_searches,
            unassigned_records,
            n_sources,
            sources_without_type,
            criteria_published: Criteria::from_payload(state.criteria.as_ref()).version > 0,
            n_writers: state.members.values().filter(|m| WRITE_ROLES.contains(&m.role.as_str())).count() as i64,
            n_records: total - exact - resolved,
            n_searches: self.search_state()?.list.len() as i64,
            open_duplicates,
            fulltext_needed: needed.len() as i64,
            fulltext_retrieved,
            fulltext_unavailable,
        })
    }

    /// Einträge ohne Dubletten, die in einer Station mit allen Einträgen keiner Runde zugeteilt sind.
    /// Nach einem späteren Import (Zitationssuche) sind das die neuen Einträge.
    fn unassigned(&self, workflow: &Workflow, stages: &[StageState], resolutions: &Resolutions) -> Result<i64> {
        let active: BTreeSet<Uuid> = self
            .store
            .record_keys()?
            .into_iter()
            .filter(|k| k.duplicate_of.is_none() && !resolutions.is_removed(&k.id))
            .map(|k| k.id)
            .collect();
        let mut most = 0;
        for station in &workflow.template.stations {
            let whole = station.kind == "screening"
                && !station.free_rounds
                && station.input.as_ref().is_none_or(|i| i.scope == "all");
            if !whole {
                continue;
            }
            let mut left = active.clone();
            for stage in stages.iter().filter(|s| workflow.station_of(s).is_some_and(|x| x.key == station.key)) {
                for assignment in self.store.assignments(Some(&stage.config.id))? {
                    left.remove(&assignment.record_id);
                }
            }
            most = most.max(left.len() as i64);
        }
        Ok(most)
    }

    /// Stationen der Vorlage mit Zustand, Runden und Gates.
    pub fn stations(&self) -> Result<Vec<StationView>> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &ROLES)?;
        let workflow = self.workflow()?;
        let stages = self.stage_states(&state)?;
        Ok(workflow.stations(&self.facts(&state, &workflow, &stages)?, &stages))
    }

    pub fn station(&self, key: &str) -> Result<StationView> {
        self.stations()?
            .into_iter()
            .find(|s| s.key == key)
            .ok_or_else(|| not_found(tr!("Diese Station gibt es nicht.")))
    }

    /// Eingaben für die nächste Stufe einer Station, wie die Vorlage sie vorsieht.
    pub fn stage_defaults(&self, station: &str) -> Result<OpenStage> {
        let view = self.station(station)?;
        let d = view.defaults.ok_or_else(|| invalid(tr!("Diese Station arbeitet nicht mit Stufen.")))?;
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
            form_id: None,
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
            return Err(invalid(tr!("Eine geänderte Vorlage braucht eine Begründung.")));
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
            return Err(invalid(tr!("Übersteuern braucht eine Begründung.")));
        }
        let view = self.station(station)?;
        if !view.blocking {
            return Err(invalid(tr!("Die Station „{}“ sperrt nichts.", view.label)));
        }
        if view.state == "done" {
            return Err(conflict(tr!("Die Station „{}“ ist bereits abgeschlossen.", view.label)));
        }
        if view.rounds.iter().any(|r| !r.closed) {
            return Err(conflict(tr!("Erst die laufende Stufe dieser Station schließen.")));
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
            return Err(forbidden(tr!("Die Stufe ist verblindet. Das Gate ist erst nach dem Entblinden einsehbar.")));
        }
        self.evaluate_gate(&self.workflow()?, &stage)
    }

    fn evaluate_gate(&self, workflow: &Workflow, stage: &StageState) -> Result<Option<GateResult>> {
        let Some(station) = workflow.station_of(stage).filter(|s| !s.gate.is_empty()) else { return Ok(None) };
        if stage.config.form_id.is_some() {
            return Ok(Some(self.form_gate(station, stage)?));
        }
        let assigned: Vec<String> =
            self.store.assignments(Some(&stage.config.id))?.into_iter().map(|a| a.record_id.to_string()).collect();
        Ok(Some(workflow::evaluate_gate(station, stage, &assigned)))
    }

    pub fn name_of(&self, actor: &Uuid) -> Result<String> {
        Ok(self.store.person_name(actor)?.unwrap_or_else(|| tr!("Unbekannt ({})", &actor.to_string()[..8])))
    }

    pub fn list_item(&self) -> Result<ReviewListItem> {
        let state = self.state()?;
        Ok(ReviewListItem {
            id: self.id()?,
            title: state.title.clone(),
            role: state.role_of(&self.me.actor_id).map(str::to_string),
            color: self.project_settings()?.color,
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
            return Err(invalid(tr!("Unbekannte Rolle.")));
        }
        parse_public_key(&person.public_key)?;
        if let Some(current) = state.members.get(&person.actor_id) {
            if current.public_key != person.public_key {
                return Err(conflict(tr!("Diese Person ist schon mit einem anderen Schlüssel Mitglied.")));
            }
            let owners = state.members.values().filter(|m| m.role == "owner").count();
            if current.role == "owner" && role != "owner" && owners == 1 {
                return Err(invalid(tr!("Ein Review braucht mindestens einen Owner.")));
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
        let member = state.members.get(actor_id).ok_or_else(|| not_found(tr!("Diese Person ist kein Mitglied.")))?;
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
                return Err(invalid(tr!("Das Element „{}“ gibt es im Kriterienschema der Vorlage nicht.", element)));
            }
            items.push(json!({
                "code": text(item, "code"), "element": element, "kind": text(item, "kind"),
                "text": text(item, "text"), "note": text(item, "note"),
            }));
        }
        if items.is_empty() {
            return Err(invalid(tr!("Mindestens ein Kriterium ist nötig.")));
        }
        let change_note = text(data, "change_note");
        if previous.version > 0 && change_note.is_empty() {
            return Err(invalid(tr!("Was hat sich an den Kriterien geändert? Die Angabe fehlt.")));
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

    /// Kurzform ohne Suchversion: Treffer einer Datenbank mit frei benannter Quelle.
    pub fn import_file(&self, filename: &str, text: &str, source: &str) -> Result<BatchRow> {
        let from = ImportSource { source: source.into(), origin: "database".into(), ..ImportSource::default() };
        self.import_file_with(filename, text, &from)
    }

    /// Liest eine Datei in RIS, BibTeX oder dem PubMed-Format ein. Fehler in der Datei landen im
    /// Importstatus. Mit Suchversion übernimmt der Import Quelle und Herkunft von dort.
    pub fn import_file_with(&self, filename: &str, text: &str, from: &ImportSource) -> Result<BatchRow> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        let (source, origin, search) = match from.search_id {
            Some(id) => {
                let searches = self.search_state()?;
                let search = searches.get(&id).ok_or_else(|| not_found(tr!("Diese Suche gibt es nicht.")))?;
                let version = match from.search_version {
                    Some(n) => search.version(n),
                    None => search.latest(),
                }
                .ok_or_else(|| not_found(tr!("Diese Suchversion gibt es nicht.")))?;
                (version.source.clone(), version.origin.clone(), Some((id, version.version)))
            }
            None => {
                if !catalog::ORIGINS.contains(&from.origin.as_str()) {
                    return Err(invalid(tr!(
                        "Woher stammen die Treffer? Wähle eine Suche oder die Herkunft: Datenbank, Register, Zitationssuche oder andere."
                    )));
                }
                (from.source.trim().to_string(), from.origin.clone(), None)
            }
        };
        let source = source.as_str();
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
        let mut payload = json!({
            "batch_id": batch.id.to_string(), "source": batch.source, "filename": batch.filename,
            "file_sha256": batch.sha256, "format": batch.format, "n_records": batch.n_records,
            "n_duplicates": n_duplicates, "record_ids_sha256": sha256_hex(ids.join("\n").as_bytes()),
            "records_sha256": self.store.records_digest(&batch.id)?,
            "created_at": batch.created_at, "origin": origin,
        });
        if let Some((id, version)) = search {
            payload["search_id"] = json!(id.to_string());
            payload["search_version"] = json!(version);
        }
        self.append("RecordsImported", None, payload)?;
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

    fn import_events(&self) -> Result<Vec<ImportEvent>> {
        let events = self.store.events_of_types(&["RecordsImported"])?;
        Ok(events
            .iter()
            .map(|e| {
                let p = catalog::upcast(e);
                let text = |key: &str| p.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
                let number = |key: &str| p.get(key).and_then(Value::as_i64);
                let search_id = p.get("search_id").and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok());
                ImportEvent {
                    batch_id: Uuid::parse_str(&text("batch_id")).ok(),
                    source: text("source"),
                    origin: text("origin"),
                    search: search_id.zip(number("search_version")),
                    n_records: number("n_records").unwrap_or(0),
                    n_duplicates: number("n_duplicates").unwrap_or(0),
                }
            })
            .collect())
    }

    /// Import → Herkunft, für jeden Import, den das Log nennt.
    pub(crate) fn import_origins(&self) -> Result<Vec<(Uuid, String)>> {
        Ok(self.import_events()?.into_iter().filter_map(|e| Some((e.batch_id?, e.origin))).collect())
    }

    /// Treffer je Quelle und Dublettenzahl, allein aus dem Log. Grundlage für das Flussdiagramm.
    pub fn import_summary(&self) -> Result<ImportSummary> {
        require(&self.state()?, &self.me.actor_id, &ROLES)?;
        let mut sources: Vec<SourceCount> = Vec::new();
        for e in self.import_events()? {
            let (search_id, search_version) = (e.search.map(|s| s.0), e.search.map(|s| s.1));
            let found = sources.iter().position(|s| {
                s.source == e.source
                    && s.origin == e.origin
                    && s.search_id == search_id
                    && s.search_version == search_version
            });
            let index = found.unwrap_or_else(|| {
                sources.push(SourceCount {
                    source: e.source.clone(),
                    origin: e.origin.clone(),
                    search_id,
                    search_version,
                    n_imports: 0,
                    n_records: 0,
                    n_duplicates: 0,
                });
                sources.len() - 1
            });
            sources[index].n_imports += 1;
            sources[index].n_records += e.n_records;
            sources[index].n_duplicates += e.n_duplicates;
        }
        let n_records: i64 = sources.iter().map(|s| s.n_records).sum();
        let n_exact_duplicates: i64 = sources.iter().map(|s| s.n_duplicates).sum();
        let n_resolved_duplicates = self.resolutions()?.removed.len() as i64;
        Ok(ImportSummary {
            sources,
            n_records,
            n_exact_duplicates,
            n_resolved_duplicates,
            n_unique: n_records - n_exact_duplicates - n_resolved_duplicates,
        })
    }

    // ------------------------------------------------------------ Suche

    fn search_state(&self) -> Result<Searches> {
        Ok(Searches::build(&self.store.events_of_types(&SEARCH_TYPES)?))
    }

    /// Suchen mit ihren Versionen und den Importen je Version.
    pub fn searches(&self) -> Result<Vec<SearchView>> {
        require(&self.state()?, &self.me.actor_id, &ROLES)?;
        let imports = self.import_events()?;
        Ok(self
            .search_state()?
            .list
            .into_iter()
            .map(|search| {
                let versions = search
                    .versions
                    .into_iter()
                    .map(|version| {
                        let own: Vec<&ImportEvent> =
                            imports.iter().filter(|i| i.search == Some((search.id, version.version))).collect();
                        SearchVersionView {
                            n_imports: own.len() as i64,
                            n_records: own.iter().map(|i| i.n_records).sum(),
                            n_duplicates: own.iter().map(|i| i.n_duplicates).sum(),
                            version,
                        }
                    })
                    .collect();
                SearchView { id: search.id, versions }
            })
            .collect())
    }

    /// Hält eine Suche fest. Mit `search_id` entsteht eine neue Version dieser Suche, sonst eine neue Suche.
    pub fn publish_search(&self, data: &Value) -> Result<Search> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        let text = |key: &str| data.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_string();
        let number = |key: &str| data.get(key).and_then(Value::as_i64);
        let searches = self.search_state()?;
        let (id, version, previous) = match data.get("search_id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
            Some(raw) => {
                let id = Uuid::parse_str(raw).map_err(|_| invalid(tr!("Ungültige ID der Suche.")))?;
                let search = searches.get(&id).ok_or_else(|| not_found(tr!("Diese Suche gibt es nicht.")))?;
                let last = search.latest().map_or(0, |v| v.version);
                (id, last + 1, Some(last))
            }
            None => (Uuid::new_v4(), 1, None),
        };
        if text("source").is_empty() {
            return Err(invalid(tr!("Die Quelle fehlt, z. B. MEDLINE oder ClinicalTrials.gov.")));
        }
        if text("query").is_empty() {
            return Err(invalid(tr!("Der Suchstring fehlt.")));
        }
        if !catalog::ORIGINS.contains(&text("origin").as_str()) {
            return Err(invalid(tr!("Die Herkunft fehlt: Datenbank, Register, Zitationssuche oder andere.")));
        }
        if !catalog::is_date(&text("searched_on")) {
            return Err(invalid(tr!("Das Datum der Suche fehlt oder hat nicht die Form JJJJ-MM-TT.")));
        }
        if number("hit_limit").is_some_and(|n| n < 1) || number("hits").is_some_and(|n| n < 0) {
            return Err(invalid(tr!("Trefferzahl und Trefferlimit dürfen nicht negativ sein.")));
        }
        if version > 1 && text("change_note").is_empty() {
            return Err(invalid(tr!("Was hat sich an der Suche geändert? Die Notiz zur Änderung fehlt.")));
        }
        self.append(
            "SearchVersionPublished",
            None,
            json!({
                "search_id": id.to_string(), "version": version, "previous_version": previous,
                "source": text("source"), "platform": text("platform"), "origin": text("origin"),
                "query": text("query"), "searched_on": text("searched_on"), "limits": text("limits"),
                "hit_limit": number("hit_limit"), "hits": number("hits"), "change_note": text("change_note"),
            }),
        )?;
        self.search_state()?.get(&id).cloned().ok_or_else(|| not_found(tr!("Diese Suche gibt es nicht.")))
    }

    /// Vermerkt ein Peer-Review der Suche, z. B. nach PRESS. Im Log steht kein Name: entweder ein
    /// Mitglied oder nur der Vermerk mit der Rolle der Person.
    pub fn review_search(
        &self,
        search_id: &Uuid,
        version: i64,
        reviewed_on: &str,
        note: &str,
        reviewer: Option<Uuid>,
    ) -> Result<()> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        let searches = self.search_state()?;
        if searches.get(search_id).and_then(|s| s.version(version)).is_none() {
            return Err(not_found(tr!("Diese Suchversion gibt es nicht.")));
        }
        if note.trim().is_empty() {
            return Err(invalid(tr!("Der Vermerk fehlt: Wer hat geprüft (Rolle) und mit welchem Ergebnis?")));
        }
        if !catalog::is_date(reviewed_on) {
            return Err(invalid(tr!("Das Datum der Prüfung fehlt oder hat nicht die Form JJJJ-MM-TT.")));
        }
        if reviewer.is_some_and(|r| !state.members.contains_key(&r)) {
            return Err(not_found(tr!("Diese Person ist kein Mitglied des Reviews.")));
        }
        self.append(
            "SearchPeerReviewed",
            None,
            json!({
                "search_id": search_id.to_string(), "version": version, "reviewed_on": reviewed_on,
                "note": note.trim(), "reviewer": reviewer.map(|r| r.to_string()),
            }),
        )?;
        Ok(())
    }

    // ------------------------------------------------------------ Dubletten

    pub(crate) fn resolutions(&self) -> Result<Resolutions> {
        Ok(Resolutions::build(&self.store.events_of_types(&["DuplicatesResolved"])?))
    }

    /// Einträge insgesamt, exakte Dubletten aus dem Import und als Dublette entschiedene.
    pub(crate) fn duplicate_counts(&self, resolutions: &Resolutions) -> Result<(i64, i64, i64)> {
        let count = |sql: &str| -> Result<i64> { Ok(self.store.conn().query_row(sql, [], |r| r.get(0))?) };
        let mut resolved = 0;
        for id in resolutions.removed.keys() {
            // Zählt nur, was hier vorhanden und nicht schon exakte Dublette ist.
            if self.store.record(id)?.is_some_and(|r| r.duplicate_of.is_none()) {
                resolved += 1;
            }
        }
        Ok((
            count("SELECT count(*) FROM record")?,
            count("SELECT count(*) FROM record WHERE duplicate_of IS NOT NULL")?,
            resolved,
        ))
    }

    /// Mögliche Dubletten, über die noch nicht entschieden ist, die sichersten zuerst.
    pub fn duplicate_candidates(&self) -> Result<DuplicateList> {
        require(&self.state()?, &self.me.actor_id, &ROLES)?;
        let resolutions = self.resolutions()?;
        let found = duplicates::candidates(&self.store.record_keys()?, &resolutions);
        let sources: BTreeMap<Uuid, String> = self.store.batches()?.into_iter().map(|b| (b.id, b.source)).collect();
        let source_of = |r: &RecordRow| sources.get(&r.batch_id).cloned().unwrap_or_default();
        // Vollständiger ist, wer Abstract und DOI hat; bei Gleichstand bleibt der erste.
        let weight = |r: &RecordRow| {
            2 * i64::from(!r.abstract_text.is_empty())
                + 2 * i64::from(!r.doi.is_empty())
                + i64::from(!r.pmid.is_empty())
        };
        let mut pairs = Vec::new();
        for c in found.iter().take(PAIR_LIMIT) {
            let (Some(a), Some(b)) = (self.store.record(&c.a)?, self.store.record(&c.b)?) else { continue };
            pairs.push(DuplicatePair {
                keep: if weight(&b) > weight(&a) { b.id } else { a.id },
                a_source: source_of(&a),
                b_source: source_of(&b),
                similarity: c.similarity,
                a: a.into(),
                b: b.into(),
            });
        }
        let (_, n_exact, n_resolved) = self.duplicate_counts(&resolutions)?;
        Ok(DuplicateList { pairs, total: found.len(), n_exact, n_resolved, n_distinct: resolutions.distinct.len() })
    }

    /// Entscheidet Paare: Dublette (mit dem Eintrag, der bleibt) oder keine. Eine spätere Entscheidung
    /// zum selben Paar ersetzt die frühere. Offene und geschlossene Stufen ändern sich nicht.
    pub fn resolve_duplicates(&self, pairs: &[PairDecision]) -> Result<usize> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        if pairs.is_empty() {
            return Err(invalid(tr!("Kein Paar gewählt.")));
        }
        for pair in pairs {
            for id in [&pair.a, &pair.b] {
                let record = self.store.record(id)?.ok_or_else(|| not_found(tr!("Eintrag nicht gefunden.")))?;
                if record.duplicate_of.is_some() {
                    return Err(invalid(tr!(
                        "„{}“ ist schon beim Import als Dublette erkannt.",
                        record.title.chars().take(60).collect::<String>()
                    )));
                }
            }
        }
        for chunk in pairs.chunks(catalog::MAX_PAIRS) {
            let list: Vec<Value> = chunk
                .iter()
                .map(|p| {
                    let mut pair = json!({"a": p.a.to_string(), "b": p.b.to_string(), "decision": p.decision});
                    if let Some(keep) = p.keep {
                        pair["keep"] = json!(keep.to_string());
                    }
                    pair
                })
                .collect();
            self.append("DuplicatesResolved", None, json!({ "pairs": list }))?;
        }
        Ok(pairs.len())
    }

    pub fn records(&self, query: Option<&str>, include_duplicates: bool) -> Result<Vec<RecordItem>> {
        let rows = self.store.records_where("ORDER BY title, id", &[])?;
        let q = query.map(str::to_lowercase).filter(|q| !q.is_empty());
        let resolutions = self.resolutions()?;
        Ok(rows
            .into_iter()
            .filter(|r| include_duplicates || (r.duplicate_of.is_none() && !resolutions.is_removed(&r.id)))
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
            state.role_of(&self.me.actor_id).ok_or_else(|| forbidden(tr!("Du bist nicht Mitglied dieses Reviews.")))?;
        let (n_records, n_exact, n_resolved) = self.duplicate_counts(&self.resolutions()?)?;
        let events = self.import_events()?;
        let imports = self
            .store
            .batches()?
            .into_iter()
            .map(|batch| {
                let event = events.iter().find(|e| e.batch_id == Some(batch.id));
                ImportView {
                    origin: event.map(|e| e.origin.clone()).unwrap_or_default(),
                    search_id: event.and_then(|e| e.search).map(|s| s.0),
                    search_version: event.and_then(|e| e.search).map(|s| s.1),
                    batch,
                }
            })
            .collect();
        Ok(ReviewDetail {
            id: self.id()?,
            title: state.title.clone(),
            role: role.to_string(),
            me: self.me.actor_id,
            members: self.members()?,
            n_records,
            n_duplicates: n_exact + n_resolved,
            criteria: Criteria::from_payload(state.criteria.as_ref()),
            stages: self.list_stages()?,
            imports,
            workflow: self.workflow_view(&state)?,
            ai_allowed: state.ai_allowed(),
        })
    }

    fn workflow_view(&self, state: &ReviewState) -> Result<WorkflowView> {
        let workflow = self.workflow()?;
        let stages = self.stage_states(state)?;
        let stations = workflow.stations(&self.facts(state, &workflow, &stages)?, &stages);
        // Texte der Vorlage in der geltenden Sprache; im Log bleibt sie, wie sie veröffentlicht wurde.
        let shown = workflow.template.localized();
        Ok(WorkflowView {
            published: workflow.published(),
            version: workflow.version,
            template_key: shown.key,
            template_name: shown.name,
            review_type: shown.review_type,
            terms: shown.terms,
            criteria_schema: shown.criteria_schema,
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
            return Err(invalid(tr!("maybe_as muss leer, include oder exclude sein.")));
        }
        let name = params.name.trim();
        if name.is_empty() {
            return Err(invalid(tr!("Name der Stufe fehlt.")));
        }
        if params.min_reviewers < 1 {
            return Err(invalid(tr!("Mindestens eine Stimme je Record ist nötig.")));
        }

        // Station, Sperren und Regeln aus der Vorlage
        let workflow = self.workflow()?;
        let stages = self.stage_states(&state)?;
        let views = workflow.stations(&self.facts(&state, &workflow, &stages)?, &stages);
        let key = match (&params.station, workflow.published()) {
            (Some(key), _) => key.clone(),
            (None, false) => workflow.template.default_station().map(|s| s.key.clone()).unwrap_or_default(),
            (None, true) => return Err(invalid(tr!("Zu welcher Station gehört die Stufe? Die Angabe fehlt."))),
        };
        let station = workflow.template.station(&key).ok_or_else(|| not_found(tr!("Diese Station gibt es nicht.")))?;
        let is_form = station.kind == "form";
        if !is_form && station.kind != "screening" {
            return Err(invalid(tr!("Die Station „{}“ arbeitet nicht mit Stufen.", station.label)));
        }
        // Formular-Stufen arbeiten mit der geltenden Version; die Nummer beim Öffnen steht im Log.
        let form = match is_form {
            true => Some(self.form_for(station, params.form_id.as_ref())?),
            false => None,
        };
        let view = views.iter().find(|v| v.key == key).ok_or_else(|| not_found(tr!("Diese Station gibt es nicht.")))?;
        if let Some(blocker) = view.blocked_by.first() {
            return Err(conflict(tr!(
                "Die Station „{}“ ist gesperrt: „{}“ ist noch nicht abgeschlossen. Die Leitung kann das mit einer Begründung übersteuern.",
                station.label, blocker.label
            )));
        }
        if !station.free_rounds && view.rounds.iter().any(|r| !r.closed) {
            return Err(conflict(tr!("In der Station „{}“ ist noch eine Stufe offen.", station.label)));
        }
        let round = view.rounds.last().map_or(0, |r| r.round) + 1;
        let rules = workflow.template.rules_for(station);
        let decision_rule = params.decision_rule.clone().unwrap_or_else(|| rules.decision_rule.value.clone());
        let conflict_rule = params.conflict_rule.clone().unwrap_or_else(|| rules.conflict_rule.value.clone());
        if !workflow::DECISION_RULES.contains(&decision_rule.as_str()) {
            return Err(invalid(tr!("Unbekannte Auswahlregel.")));
        }
        if is_form && decision_rule == "liberal" {
            return Err(invalid(tr!("Extrahiert wird doppelt oder einfach mit Prüfung.")));
        }
        if !workflow::CONFLICT_RULES.contains(&conflict_rule.as_str()) {
            return Err(invalid(tr!("Unbekannte Klärungsregel.")));
        }
        let label = &station.label;
        let fixed = |editable: bool, same: bool, message: &dyn Fn() -> String| -> Result<()> {
            if editable || same {
                Ok(())
            } else {
                Err(invalid(message()))
            }
        };
        fixed(rules.decision_rule.editable, decision_rule == rules.decision_rule.value, &|| {
            tr!("Die Vorlage legt die Auswahlregel für die Station „{}“ fest.", label)
        })?;
        fixed(rules.conflict_rule.editable, conflict_rule == rules.conflict_rule.value, &|| {
            tr!("Die Vorlage legt die Klärungsregel für die Station „{}“ fest.", label)
        })?;
        fixed(rules.min_reviewers.editable, params.min_reviewers == rules.min_reviewers.value, &|| {
            tr!("Die Vorlage legt die Zahl der Stimmen für die Station „{}“ fest.", label)
        })?;
        fixed(rules.maybe_as.editable, params.maybe_as == rules.maybe_as.value, &|| {
            tr!("Die Vorlage legt die Maybe-Regel für die Station „{}“ fest.", label)
        })?;
        fixed(rules.blind.editable, params.blind == rules.blind.value, &|| {
            tr!("Die Vorlage legt die Verblindung für die Station „{}“ fest.", label)
        })?;
        let verify = if decision_rule == "single_verified" {
            if params.blind {
                return Err(invalid(tr!(
                    "Bei „eine Stimme mit Prüfung“ sieht die zweite Person die Ausschlüsse. Die Stufe kann deshalb nicht verblindet sein."
                )));
            }
            let mut verify = params.verify.clone().unwrap_or(Verify { scope: "all".into(), percent: None, seed: None });
            match verify.scope.as_str() {
                "all" => (verify.percent, verify.seed) = (None, None),
                "sample" => {
                    if !verify.percent.is_some_and(|p| (1..=100).contains(&p)) {
                        return Err(invalid(tr!("Der Anteil der Stichprobe muss zwischen 1 und 100 Prozent liegen.")));
                    }
                    verify.seed = Some(verify.seed.unwrap_or_else(|| rand::thread_rng().gen_range(1..2_147_483_647)));
                }
                _ => return Err(invalid(tr!("Die Prüfung gilt für alle Ausschlüsse oder für eine Stichprobe."))),
            }
            Some(verify)
        } else {
            None
        };
        let criteria = Criteria::from_payload(state.criteria.as_ref());
        let reason_required = params.reason_required.unwrap_or(station.reason_required);
        if station.reason_required && !reason_required {
            return Err(invalid(tr!(
                "Die Vorlage verlangt in der Station „{}“ für jeden Ausschluss einen Hauptgrund.",
                station.label
            )));
        }
        if reason_required && criteria.reason_codes().is_empty() {
            return Err(invalid(tr!(
                "In dieser Stufe braucht jeder Ausschluss einen Hauptgrund. Dafür fehlen Ausschlusskriterien mit Kürzel."
            )));
        }
        let source_stage = params.source_stage.or(view.defaults.as_ref().and_then(|d| d.source_stage));

        let resolutions = self.resolutions()?;
        let sample_scope = params.scope == "sample";
        let mut seed = params.seed;
        let ids: Vec<String> = match params.scope.as_str() {
            "all" | "sample" if is_form => {
                let per_type = sample_scope.then(|| params.sample_size.unwrap_or(PILOT_PER_TYPE));
                self.form_stage_ids(per_type, &mut seed)?
            }
            _ if is_form => return Err(invalid(tr!("Eine Formular-Stufe bekommt alle Quellen oder eine Stichprobe."))),
            "from_stage" => {
                let source_id = source_stage.ok_or_else(|| invalid(tr!("Quellstufe fehlt.")))?;
                let source = self.stage_state(&source_id)?;
                // Speist eine Station mit festen Runden, zählen alle ihre Runden: Was eine frühere
                // Runde eingeschlossen hat, geht nicht verloren, wenn später eine weitere dazukam.
                let feeding = workflow.station_of(&source).filter(|s| {
                    workflow.published()
                        && !s.free_rounds
                        && station.input.as_ref().and_then(|i| i.station.as_deref()) == Some(s.key.as_str())
                });
                let mut sources: Vec<&StageState> = match feeding {
                    Some(feeding) => {
                        stages.iter().filter(|s| workflow.station_of(s).is_some_and(|x| x.key == feeding.key)).collect()
                    }
                    None => vec![&source],
                };
                sources.sort_by_key(|s| s.config.round);
                let mut decided: BTreeMap<String, bool> = BTreeMap::new();
                for stage in sources {
                    for (id, d) in stage.final_decisions() {
                        decided.insert(id, d.decision.as_deref() == Some("include"));
                    }
                }
                decided
                    .into_iter()
                    .filter(|(_, included)| *included)
                    .map(|(id, _)| id)
                    // Was inzwischen als Dublette entschieden ist, geht nicht in die Folgestufe.
                    .filter(|id| !Uuid::parse_str(id).is_ok_and(|id| resolutions.is_removed(&id)))
                    .collect()
            }
            "all" | "sample" => {
                let mut ids: Vec<String> = self
                    .store
                    .records_where("WHERE duplicate_of IS NULL", &[])?
                    .into_iter()
                    .filter(|r| !resolutions.is_removed(&r.id))
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
            _ => return Err(invalid(tr!("Unbekannter Umfang der Stufe."))),
        };
        // Stationen mit festen Runden legen nichts doppelt vor: Eine weitere Runde bekommt nur, was in
        // keiner früheren war, etwa die Treffer der Zitationssuche. Stichproben ziehen jedes Mal neu.
        let mut earlier: BTreeSet<Uuid> = BTreeSet::new();
        if workflow.published() && !station.free_rounds && !sample_scope {
            for round in &view.rounds {
                earlier.extend(self.store.assignments(Some(&round.stage_id))?.into_iter().map(|a| a.record_id));
            }
        }
        let fulltexts = match station.requires_fulltext {
            true => Some(self.fulltexts()?),
            false => None,
        };
        let ids: Vec<String> = ids
            .into_iter()
            .filter(|id| {
                Uuid::parse_str(id)
                    .is_ok_and(|id| !earlier.contains(&id) && fulltexts.as_ref().is_none_or(|f| f.is_retrieved(&id)))
            })
            .collect();
        if ids.is_empty() {
            return Err(invalid(match (station.requires_fulltext, earlier.is_empty()) {
                _ if is_form && earlier.is_empty() => tr!("Es gibt noch keine eingeschlossenen Quellen."),
                (true, true) => tr!("Für diese Stufe ist noch kein Volltext beschafft."),
                (true, false) => tr!("Seit der letzten Runde ist kein weiterer Volltext beschafft worden."),
                (false, true) => tr!("Für diese Stufe gibt es keine Records."),
                (false, false) => tr!("Seit der letzten Runde ist nichts Neues dazugekommen."),
            }));
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
        let mut payload = json!({
            "name": name, "kind": params.kind, "blind": params.blind, "min_reviewers": params.min_reviewers,
            "maybe_as": params.maybe_as, "criteria_version": criteria.version.to_string(),
            "scope": params.scope, "sample_size": if sample_scope { Some(ids.len()) } else { None },
            "seed": if sample_scope { seed } else { None },
            "source_stage": if params.scope == "from_stage" { source_stage.map(|s| s.to_string()) } else { None },
            "n_records": ids.len(), "record_ids_sha256": self.store.assignment_digest(&stage_id)?,
            "station": key, "round": round, "decision_rule": decision_rule, "conflict_rule": conflict_rule,
            "verify": verify, "reason_required": reason_required,
        });
        if let Some(form) = &form {
            payload["form_id"] = json!(form.id);
            payload["form_version"] = json!(form.current().version);
        }
        self.append("StageInstanceOpened", Some(stage_id), payload)?;
        tx.commit()?;
        self.get_stage(&stage_id)
    }

    pub fn unblind_stage(&self, stage_id: &Uuid) -> Result<()> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if !stage.blind_active() {
            return Err(conflict(tr!("Die Stufe ist nicht verblindet.")));
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
            return Err(conflict(tr!("Die Stufe ist bereits geschlossen.")));
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

    pub(crate) fn require_assigned(&self, stage_id: &Uuid, record_id: &str) -> Result<Uuid> {
        let id = parse_record_id(record_id)?;
        if !self.store.is_assigned(stage_id, &id)? {
            return Err(not_found(tr!("Der Record gehört nicht zu dieser Stufe.")));
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
        if stage.config.form_id.is_some() {
            return Err(invalid(tr!("Diese Stufe arbeitet mit einem Formular, nicht mit Stimmen.")));
        }
        if stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        if !DECISIONS.contains(&decision) {
            return Err(invalid(tr!("Entscheidung muss include, exclude oder maybe sein.")));
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
        if stage.config.form_id.is_some() {
            return Err(invalid(tr!("Diese Stufe arbeitet mit einem Formular, nicht mit Stimmen.")));
        }
        if stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
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
        if stage.config.form_id.is_some() {
            return Err(invalid(tr!("Diese Stufe arbeitet mit einem Formular, nicht mit Stimmen.")));
        }
        if stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        if stage.blind_active() {
            return Err(forbidden(tr!("Die Stufe ist verblindet. Ein Konsens ist erst nach dem Entblinden möglich.")));
        }
        if !matches!(decision, "include" | "exclude") {
            return Err(invalid(tr!("Ein Konsens muss include oder exclude sein.")));
        }
        let record = self.require_assigned(stage_id, record_id)?.to_string();
        let third_person = stage.config.conflict_rule == "third_person";
        if third_person && stage.votes.contains_key(&(record.clone(), self.me.actor_id)) {
            return Err(forbidden(tr!(
                "In dieser Stufe entscheidet eine dritte Person. Du hast zu diesem Eintrag selbst abgestimmt."
            )));
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

    pub(crate) fn record_item(&self, id: &str) -> Result<RecordItem> {
        let id = parse_record_id(id)?;
        self.store.record(&id)?.map(RecordItem::from).ok_or_else(|| not_found(tr!("Record nicht gefunden.")))
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
        let template = self.workflow()?.template.localized();
        Ok(StageDetail {
            progress: self.progress(&stage)?,
            stage: stage_view(&stage),
            role,
            newer_criteria: Criteria::from_payload(state.criteria.as_ref()).version > criteria.version,
            criteria,
            criteria_schema: template.criteria_schema,
            members: self.members()?,
            terms: template.terms,
            ai_allowed: state.ai_allowed(),
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
            return Err(invalid(tr!("Unbekannter Filter für die Entscheidung.")));
        }
        if blind {
            let locked = if wanted == Some("conflict") {
                Some(tr!("Die Stufe ist verblindet. Konflikte sind erst nach dem Entblinden sichtbar."))
            } else if person != me {
                Some(tr!("Die Stufe ist verblindet. Stimmen anderer Personen sind erst nach dem Entblinden sichtbar."))
            } else if filter.votes_min.is_some() || filter.votes_max.is_some() {
                Some(tr!("Die Stufe ist verblindet. Die Zahl der Stimmen ist erst nach dem Entblinden sichtbar."))
            } else {
                None
            };
            if let Some(message) = locked {
                return Err(forbidden(message));
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
                    wants_my_vote: stage.wants_vote_from(&id, &me),
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
        let fulltext = Uuid::parse_str(&record).ok().and_then(|id| self.fulltexts().ok()?.status.remove(&id));
        let my_suggestion = stage.suggestions.get(&(record.clone(), self.me.actor_id)).cloned();
        Ok(RecordDetail { record: self.record_item(&record)?, my_vote, fulltext, my_suggestion })
    }

    // ------------------------------------------------------------ Auswertung

    /// Records mit Widerspruch oder Markierung, samt Einzelvoten. Erst nach dem Entblinden.
    pub fn conflicts(&self, stage_id: &Uuid) -> Result<Vec<Conflict>> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if stage.blind_active() {
            return Err(forbidden(tr!("Die Stufe ist verblindet. Konflikte sind erst nach dem Entblinden sichtbar.")));
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
                        ai: stage.suggestions.get(&(r.clone(), *actor)).map(|s| s.suggested.clone()),
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
            return Err(forbidden(tr!("Die Stufe ist verblindet.")));
        }
        Ok(stage.final_decisions())
    }

    /// Tabelle einer Screening-Stufe als CSV: je Eintrag eine Zeile, je Person eine Spalte mit ihrer Stimme.
    pub fn export_csv(&self, stage_id: &Uuid) -> Result<String> {
        self.stage_file(stage_id, "csv")
    }

    /// Stand einer Screening-Stufe als Datei, `csv` oder `json`. Beide zeigen dasselbe: je Eintrag die
    /// Entscheidung, den Hauptgrund eines Ausschlusses und die Stimmen der Personen. Das JSON nennt dazu
    /// alle Gründe, die Notizen und die Klärung.
    pub fn stage_file(&self, stage_id: &Uuid, format: &str) -> Result<String> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &ROLES)?;
        if !["csv", "json"].contains(&format) {
            return Err(invalid(tr!("Unbekannte Datei.")));
        }
        let stage = self.stage_state(stage_id)?;
        if stage.config.form_id.is_some() {
            return Err(invalid(tr!("Diese Stufe arbeitet mit einem Formular, nicht mit Stimmen.")));
        }
        if stage.blind_active() {
            return Err(forbidden(tr!("Die Stufe ist verblindet. Der Export ist erst nach dem Entblinden möglich.")));
        }
        let finals = stage.final_decisions();
        let order: Vec<String> = stage_criteria(&state, &stage).reason_codes().into_iter().map(String::from).collect();
        let mut voters: Vec<(String, Uuid)> = Vec::new();
        for actor in stage.votes.keys().map(|(_, actor)| *actor).collect::<BTreeSet<_>>() {
            voters.push((self.name_of(&actor)?, actor));
        }
        voters.sort_by_key(|(name, actor)| (name.to_lowercase(), *actor));

        let mut csv: Vec<String> =
            ["record_id", "title", "authors", "year", "journal", "doi", "status", "decision", "n_votes", "reason"]
                .map(String::from)
                .into();
        csv.extend(voters.iter().map(|(name, _)| format!("vote_{name}")));
        let mut csv = csv.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",") + "\n";
        let mut rows = Vec::new();
        for a in self.store.assignments(Some(stage_id))? {
            let Some(r) = self.store.record(&a.record_id)? else { continue };
            let id = r.id.to_string();
            let d = finals.get(&id);
            let status = d.map_or("pending", |d| d.status);
            let decision = d.and_then(|d| d.decision.clone());
            let n_votes = d.map_or(0, |d| d.n_votes);
            let reason = match decision.as_deref() {
                Some("exclude") => stage.exclusion_reason(&id, &order),
                _ => String::new(),
            };
            let vote = |actor: &Uuid| stage.votes.get(&(id.clone(), *actor));
            let mut fields = vec![
                id.clone(),
                r.title.clone(),
                r.authors.clone(),
                r.year.map(|y| y.to_string()).unwrap_or_default(),
                r.journal.clone(),
                r.doi.clone(),
                status.to_string(),
                decision.clone().unwrap_or_default(),
                n_votes.to_string(),
                reason.clone(),
            ];
            fields.extend(voters.iter().map(|(_, actor)| vote(actor).map(|v| v.decision.clone()).unwrap_or_default()));
            csv.push_str(&fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
            csv.push('\n');
            if format == "json" {
                let votes: Vec<Value> = voters
                    .iter()
                    .filter_map(|(name, actor)| {
                        let v = vote(actor)?;
                        Some(json!({
                            "actor_id": actor, "name": name, "decision": v.decision,
                            "primary_reason": v.primary_reason, "reasons": v.reasons, "note": v.note,
                        }))
                    })
                    .collect();
                let consensus = stage
                    .consensus
                    .get(&id)
                    .map(|c| json!({ "decision": c.decision, "method": c.method, "primary_reason": c.primary_reason }));
                rows.push(json!({
                    "record_id": id, "title": r.title, "authors": r.authors, "year": r.year,
                    "journal": r.journal, "doi": r.doi, "status": status, "decision": decision,
                    "n_votes": n_votes, "reason": if reason.is_empty() { Value::Null } else { json!(reason) },
                    "votes": votes, "consensus": consensus,
                }));
            }
        }
        if format == "csv" {
            return Ok(csv);
        }
        let c = &stage.config;
        let file = json!({
            "review": { "id": self.id()?, "title": state.title },
            "stage": {
                "id": c.id, "name": c.name, "station": c.station, "round": c.round,
                "decision_rule": c.decision_rule, "conflict_rule": c.conflict_rule,
                "min_reviewers": c.min_reviewers, "closed": stage.closed,
            },
            "records": rows,
        });
        Ok(serde_json::to_string_pretty(&file).unwrap_or_default() + "\n")
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
                    problem = Some(tr!(
                        "Lücke in der Kette {} von {}: erwartet {}, gefunden {}.",
                        stream,
                        &actor.to_string()[..8],
                        expected_seq,
                        e.seq
                    ));
                } else if e.prev_hash != prev.map(|p| p.hash.clone()) {
                    problem = Some(tr!("Vorgänger-Hash passt nicht bei {} ({}).", e.event_id, e.event_type));
                } else if let Some(key) = &key {
                    if let Err(err) = e.verify(key) {
                        problem = Some(err.to_string());
                    }
                } else {
                    problem = Some(tr!("Kein Schlüssel für die Person {}.", &actor.to_string()[..8]));
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

pub(crate) fn csv_field(value: &str) -> String {
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

pub(crate) fn stage_view(s: &StageState) -> StageView {
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
        form_id: c.form_id,
        form_version: c.form_version,
    }
}
