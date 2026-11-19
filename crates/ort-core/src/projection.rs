//! Projektionen: aus dem Log berechnete Zustände. Nichts davon wird gespeichert.
//!
//! Regeln wie in der früheren SQL-Fassung (`0002_projections.sql`):
//! - gültiges Votum = neuestes `VoteCast`/`VoteRevised` je Person, Record und Stufe
//! - es gilt das erste `StageInstanceOpened` einer Stufe (Regeln sind danach fest)
//! - `final_decision`: Konsens schlägt alles; sonst Markierung „nicht entscheidbar“ → Klärung;
//!   sonst einstimmig ab `min_reviewers`; verschiedene Voten → Widerspruch; sonst wartend.
//!
//! Ordnung über Personen hinweg: (`occurred_at`, `actor_id`, `stream`, `seq`). Innerhalb einer
//! Kette gilt immer `seq`.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::catalog;
use crate::event::Event;

pub fn order_key(e: &Event) -> (&str, Uuid, &str, i64) {
    (e.occurred_at.as_str(), e.actor_id, e.stream.as_str(), e.seq)
}

fn sorted(events: &[Event]) -> Vec<&Event> {
    let mut out: Vec<&Event> = events.iter().collect();
    out.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
    out
}

#[derive(Clone, Debug, Serialize)]
pub struct Member {
    pub actor_id: Uuid,
    pub role: String,
    pub public_key: String,
}

/// Zustand eines Reviews aus den Verwaltungsereignissen.
#[derive(Clone, Debug, Default)]
pub struct ReviewState {
    pub title: String,
    pub owner: Option<Uuid>,
    pub members: BTreeMap<Uuid, Member>,
    pub criteria: Option<Value>,
    /// Stufen in der Reihenfolge ihrer Eröffnung.
    pub stage_ids: Vec<Uuid>,
}

impl ReviewState {
    pub fn build(events: &[Event]) -> ReviewState {
        let mut state = ReviewState::default();
        let mut criteria_key: Option<(i64, &Event)> = None;
        let mut seen_stages = BTreeSet::new();
        for e in sorted(events) {
            match e.event_type.as_str() {
                "ReviewCreated" if state.owner.is_none() => {
                    state.title = e.payload_str("title").unwrap_or_default().to_string();
                    state.owner = Some(e.actor_id);
                }
                "MemberRoleSet" => {
                    let actor = e.payload_str("actor_id").and_then(|s| Uuid::parse_str(s).ok());
                    if let (Some(actor), Some(role)) = (actor, e.payload_str("role")) {
                        let public_key = e
                            .payload_str("public_key")
                            .map(str::to_string)
                            .or_else(|| state.members.get(&actor).map(|m| m.public_key.clone()))
                            .unwrap_or_default();
                        state.members.insert(actor, Member { actor_id: actor, role: role.to_string(), public_key });
                    }
                }
                "CriteriaVersionPublished" => {
                    let version = e.payload.get("version").and_then(Value::as_i64).unwrap_or(0);
                    if criteria_key.is_none_or(|(v, _)| version >= v) {
                        criteria_key = Some((version, e));
                    }
                }
                "StageInstanceOpened" => {
                    if let Some(id) = e.stage_instance_id {
                        if seen_stages.insert(id) {
                            state.stage_ids.push(id);
                        }
                    }
                }
                _ => {}
            }
        }
        state.criteria = criteria_key.map(|(_, e)| catalog::upcast(e));
        state
    }

    pub fn role_of(&self, actor: &Uuid) -> Option<&str> {
        self.members.get(actor).map(|m| m.role.as_str())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct StageConfig {
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
    pub record_ids_sha256: Option<String>,
    pub opened_at: String,
    pub opened_by: Uuid,
}

#[derive(Clone, Debug)]
pub struct VoteInfo {
    pub decision: String,
    pub reasons: Vec<String>,
    pub note: String,
    pub seq: i64,
}

#[derive(Clone, Debug)]
pub struct ConsensusInfo {
    pub decision: String,
    pub method: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FinalDecision {
    pub n_votes: usize,
    pub decision: Option<String>,
    pub status: &'static str,
}

/// Wie Maybe gewertet wird. `Stage` = Regel aus dem Protokoll der Stufe; `Raw` = eigene Kategorie
/// (explizite Abweichung, z. B. Sensitivitätsanalyse); `As` = explizite Zuordnung.
#[derive(Clone, Debug)]
pub enum MaybeMode {
    Stage,
    Raw,
    As(String),
}

#[derive(Clone, Debug)]
pub struct StageState {
    pub config: StageConfig,
    pub unblinded: bool,
    pub closed: bool,
    /// (record_id, actor_id) → gültiges Votum
    pub votes: BTreeMap<(String, Uuid), VoteInfo>,
    pub consensus: BTreeMap<String, ConsensusInfo>,
    /// record_id → Zahl der Markierungen
    pub flags: BTreeMap<String, usize>,
}

impl StageState {
    /// Baut den Zustand aus allen Ereignissen mit dieser `stage_instance_id`.
    pub fn build(stage_id: &Uuid, events: &[Event]) -> Option<StageState> {
        let events: Vec<&Event> =
            sorted(events).into_iter().filter(|e| e.stage_instance_id.as_ref() == Some(stage_id)).collect();
        let opened = events.iter().find(|e| e.event_type == "StageInstanceOpened")?;
        let p = &opened.payload;
        let s = |k: &str| p.get(k).and_then(Value::as_str).map(str::to_string);
        let i = |k: &str| p.get(k).and_then(Value::as_i64);
        let config = StageConfig {
            id: *stage_id,
            review_id: opened.review_id,
            name: s("name").unwrap_or_default(),
            kind: s("kind").unwrap_or_default(),
            blind: p.get("blind").and_then(Value::as_bool).unwrap_or(false),
            min_reviewers: i("min_reviewers").unwrap_or(2),
            maybe_as: s("maybe_as"),
            criteria_version: s("criteria_version"),
            scope: s("scope"),
            sample_size: i("sample_size"),
            seed: i("seed"),
            source_stage: s("source_stage"),
            n_records: i("n_records").unwrap_or(0),
            record_ids_sha256: s("record_ids_sha256"),
            opened_at: opened.occurred_at.clone(),
            opened_by: opened.actor_id,
        };
        let mut state = StageState {
            config,
            unblinded: false,
            closed: false,
            votes: BTreeMap::new(),
            consensus: BTreeMap::new(),
            flags: BTreeMap::new(),
        };
        for e in events {
            let record = e.payload_str("record_id").map(str::to_string);
            match (e.event_type.as_str(), record) {
                ("StageUnblinded", _) => state.unblinded = true,
                ("StageInstanceClosed", _) => state.closed = true,
                ("VoteCast" | "VoteRevised", Some(record)) => {
                    let info = VoteInfo {
                        decision: e.payload_str("decision").unwrap_or_default().to_string(),
                        reasons: string_list(e.payload.get("reasons")),
                        note: e.payload_str("note").unwrap_or_default().to_string(),
                        seq: e.seq,
                    };
                    let key = (record, e.actor_id);
                    if state.votes.get(&key).is_none_or(|old| old.seq < info.seq) {
                        state.votes.insert(key, info);
                    }
                }
                ("ConsensusRecorded", Some(record)) => {
                    // sortiert nach Ordnung, also gewinnt der zuletzt gesehene
                    state.consensus.insert(
                        record,
                        ConsensusInfo {
                            decision: e.payload_str("decision").unwrap_or_default().to_string(),
                            method: e.payload_str("method").unwrap_or_default().to_string(),
                        },
                    );
                }
                ("RecordFlaggedUndecidable", Some(record)) => *state.flags.entry(record).or_default() += 1,
                _ => {}
            }
        }
        Some(state)
    }

    pub fn blind_active(&self) -> bool {
        self.config.blind && !self.unblinded
    }

    /// Voten als (record_id, actor_id, decision) mit der gewählten Maybe-Regel.
    pub fn stage_votes(&self, mode: &MaybeMode) -> Vec<(String, String, String)> {
        let rule = match mode {
            MaybeMode::Stage => self.config.maybe_as.clone(),
            MaybeMode::Raw => None,
            MaybeMode::As(to) => Some(to.clone()),
        };
        self.votes
            .iter()
            .map(|((record, actor), v)| {
                let decision = match (&rule, v.decision.as_str()) {
                    (Some(to), "maybe") => to.clone(),
                    _ => v.decision.clone(),
                };
                (record.clone(), actor.to_string(), decision)
            })
            .collect()
    }

    /// Entscheidung je Record, der ein Votum, einen Konsens oder eine Markierung hat.
    pub fn final_decisions(&self) -> BTreeMap<String, FinalDecision> {
        let mut by_record: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (record, _, decision) in self.stage_votes(&MaybeMode::Stage) {
            by_record.entry(record).or_default().push(decision);
        }
        let keys: BTreeSet<String> =
            by_record.keys().chain(self.consensus.keys()).chain(self.flags.keys()).cloned().collect();
        let min = self.config.min_reviewers.max(0) as usize;
        keys.into_iter()
            .map(|record| {
                let votes = by_record.get(&record).cloned().unwrap_or_default();
                let distinct: BTreeSet<&String> = votes.iter().collect();
                let n_votes = votes.len();
                let (decision, status) = if let Some(c) = self.consensus.get(&record) {
                    (Some(c.decision.clone()), "consensus")
                } else if self.flags.contains_key(&record) {
                    (None, "needs_clarification")
                } else if distinct.len() > 1 {
                    (None, "disagreement")
                } else if n_votes < min {
                    (None, "pending")
                } else {
                    (distinct.into_iter().next().cloned(), "unanimous")
                };
                (record, FinalDecision { n_votes, decision, status })
            })
            .collect()
    }
}

pub fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    }
}
