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
use crate::error::{conflict, forbidden, invalid, not_found, Result};
use crate::event::{now_timestamp, parse_public_key, sha256_hex, Event, Identity, PublicIdentity};
use crate::import;
use crate::projection::{string_list, FinalDecision, MaybeMode, ReviewState, StageState};
use crate::store::{AssignmentRow, BatchRow, RecordRow, Store};

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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Criteria {
    pub version: i64,
    pub previous_version: Option<i64>,
    pub text: String,
    pub include_keywords: Vec<String>,
    pub exclude_keywords: Vec<String>,
    pub exclusion_reasons: Vec<String>,
    pub change_note: String,
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
    pub criteria: Criteria,
}

#[derive(Clone, Debug, Serialize)]
pub struct NextRecord {
    pub record: Option<RecordItem>,
    pub progress: Progress,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConflictVote {
    pub name: String,
    pub decision: String,
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

fn parse_record_id(record_id: &str) -> Result<Uuid> {
    Uuid::parse_str(record_id).map_err(|_| invalid("Ungültige Record-ID."))
}

impl Review {
    /// Legt ein neues Review in einer neuen Datei an. Wer es anlegt, ist Owner.
    pub fn create(path: &Path, me: Identity, title: &str) -> Result<Review> {
        if title.trim().is_empty() {
            return Err(invalid("Titel fehlt."));
        }
        if path.exists() {
            return Err(conflict("Für dieses Review gibt es schon eine Datei."));
        }
        Self::init(Store::open(path)?, me, title)
    }

    /// Für Tests und Werkzeuge: Review nur im Speicher.
    pub fn create_in_memory(me: Identity, title: &str) -> Result<Review> {
        Self::init(Store::in_memory()?, me, title)
    }

    fn init(store: Store, me: Identity, title: &str) -> Result<Review> {
        let title = title.trim();
        if title.is_empty() {
            return Err(invalid("Titel fehlt."));
        }
        let review = Review { store, me };
        let review_id = Uuid::new_v4();
        let tx = review.store.conn().unchecked_transaction()?;
        review.store.set_meta("review_id", &review_id.to_string())?;
        let me = &review.me;
        review.append(
            "ReviewCreated",
            None,
            json!({"title": title, "owner": me.actor_id.to_string(), "public_key": me.public_key()}),
        )?;
        review.append(
            "MemberRoleSet",
            None,
            json!({"actor_id": me.actor_id.to_string(), "role": "owner", "public_key": me.public_key()}),
        )?;
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
        Ok(criteria_from(self.state()?.criteria.as_ref()))
    }

    pub fn publish_criteria(&self, data: &Value) -> Result<Criteria> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &MANAGE_ROLES)?;
        let previous = criteria_from(state.criteria.as_ref());
        let payload = json!({
            "version": previous.version + 1,
            "previous_version": if previous.version > 0 { Some(previous.version) } else { None },
            "text": data.get("text").and_then(Value::as_str).unwrap_or_default(),
            "include_keywords": str_list(data.get("include_keywords")),
            "exclude_keywords": str_list(data.get("exclude_keywords")),
            "exclusion_reasons": str_list(data.get("exclusion_reasons")),
            "change_note": data.get("change_note").and_then(Value::as_str).unwrap_or_default(),
        });
        self.append("CriteriaVersionPublished", None, payload.clone())?;
        Ok(criteria_from(Some(&payload)))
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
            criteria: criteria_from(state.criteria.as_ref()),
            stages: self.list_stages()?,
            imports: self.store.batches()?,
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
        let mut seed = params.seed;
        let ids: Vec<String> = match params.scope.as_str() {
            "from_stage" => {
                let source_id = params.source_stage.ok_or_else(|| invalid("Quellstufe fehlt."))?;
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
        let criteria = criteria_from(state.criteria.as_ref());
        let sample_scope = params.scope == "sample";
        self.append(
            "StageInstanceOpened",
            Some(stage_id),
            json!({
                "name": name, "kind": params.kind, "blind": params.blind, "min_reviewers": params.min_reviewers,
                "maybe_as": params.maybe_as, "criteria_version": criteria.version.to_string(),
                "scope": params.scope, "sample_size": if sample_scope { Some(ids.len()) } else { None },
                "seed": if sample_scope { seed } else { None },
                "source_stage": if params.scope == "from_stage" { params.source_stage.map(|s| s.to_string()) } else { None },
                "n_records": ids.len(), "record_ids_sha256": self.store.assignment_digest(&stage_id)?,
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
        let tx = self.store.conn().unchecked_transaction()?;
        self.append("StageInstanceClosed", Some(*stage_id), json!({"label": label}))?;
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
        Ok(Snapshot { label, hash, n_heads })
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
        let reasons: Vec<String> = reasons.iter().map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect();
        let mut payload = json!({"record_id": record, "decision": decision, "reasons": reasons, "note": note});
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

    pub fn record_consensus(
        &self,
        stage_id: &Uuid,
        record_id: &str,
        decision: &str,
        method: &str,
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
        let method = if method.is_empty() { "discussion" } else { method };
        self.append(
            "ConsensusRecorded",
            Some(*stage_id),
            json!({"record_id": record, "decision": decision, "method": method, "reasons": reasons, "note": note}),
        )?;
        Ok(())
    }

    fn record_item(&self, id: &str) -> Result<RecordItem> {
        let id = parse_record_id(id)?;
        self.store.record(&id)?.map(RecordItem::from).ok_or_else(|| not_found("Record nicht gefunden."))
    }

    fn progress(&self, stage: &StageState) -> Progress {
        Progress { done: self.my_votes(stage).len(), total: stage.config.n_records.max(0) as usize }
    }

    pub fn stage_detail(&self, stage_id: &Uuid) -> Result<StageDetail> {
        let state = self.state()?;
        let role = require(&state, &self.me.actor_id, &ROLES)?.to_string();
        let stage = self.stage_state(stage_id)?;
        Ok(StageDetail {
            progress: self.progress(&stage),
            stage: stage_view(&stage),
            role,
            criteria: criteria_from(state.criteria.as_ref()),
        })
    }

    /// Nächster Record ohne eigenes Votum, in der für alle gleichen Reihenfolge der Stufe.
    pub fn next_record(&self, stage_id: &Uuid) -> Result<NextRecord> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &ROLES)?;
        let stage = self.stage_state(stage_id)?;
        let mine = self.my_votes(&stage);
        let next =
            self.store.assignments(Some(stage_id))?.into_iter().find(|a| !mine.contains_key(&a.record_id.to_string()));
        Ok(NextRecord {
            record: next.map(|a| self.record_item(&a.record_id.to_string())).transpose()?,
            progress: self.progress(&stage),
        })
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
            progress: self.progress(&stage),
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

fn criteria_from(payload: Option<&Value>) -> Criteria {
    let p = payload.cloned().unwrap_or(Value::Null);
    Criteria {
        version: p.get("version").and_then(Value::as_i64).unwrap_or(0),
        previous_version: p.get("previous_version").and_then(Value::as_i64),
        text: p.get("text").and_then(Value::as_str).unwrap_or_default().to_string(),
        include_keywords: string_list(p.get("include_keywords")),
        exclude_keywords: string_list(p.get("exclude_keywords")),
        exclusion_reasons: string_list(p.get("exclusion_reasons")),
        change_note: p.get("change_note").and_then(Value::as_str).unwrap_or_default().to_string(),
    }
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
    }
}
