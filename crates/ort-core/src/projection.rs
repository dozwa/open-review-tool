//! Projektionen: aus dem Log berechnete Zustände. Nichts davon wird gespeichert.
//!
//! Regeln wie in der früheren SQL-Fassung (`0002_projections.sql`):
//! - gültiges Votum = neuestes `VoteCast`/`VoteRevised` je Person, Record und Stufe
//! - es gilt das erste `StageInstanceOpened` einer Stufe (Regeln sind danach fest)
//! - `final_decision`: Konsens schlägt alles; sonst Markierung „nicht entscheidbar“ → Klärung;
//!   sonst entscheidet die Auswahlregel der Stufe:
//!   - `unanimous`: einstimmig ab `min_reviewers`; verschiedene Voten → Widerspruch; sonst wartend
//!   - `liberal`: eine Einschluss-Stimme genügt; ein Ausschluss braucht zwei gleiche Stimmen
//!   - `single_verified`: eine Stimme entscheidet; Ausschlüsse brauchen eine zweite Stimme, alle
//!     oder die aus einer Stichprobe, die sich aus Seed und Eintrags-ID nachrechnen lässt
//!
//! Ordnung über Personen hinweg: (`occurred_at`, `actor_id`, `stream`, `seq`). Innerhalb einer
//! Kette gilt immer `seq`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::catalog;
use crate::event::{sha256_hex, Event};

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
    /// Neueste Kriterienversion in der aktuellen Form.
    pub criteria: Option<Value>,
    /// Alle Kriterienversionen; eine Stufe prüft gegen die, mit der sie geöffnet wurde.
    pub criteria_versions: BTreeMap<i64, Value>,
    /// Stufen in der Reihenfolge ihrer Eröffnung.
    pub stage_ids: Vec<Uuid>,
    /// Wann die Leitung KI-Vorschläge erlaubt oder abgeschaltet hat, in der Ordnung des Logs.
    pub ai_switches: Vec<AiSwitch>,
}

/// Eine Entscheidung der Leitung über KI-Vorschläge.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AiSwitch {
    pub at: String,
    pub allowed: bool,
}

impl ReviewState {
    pub fn build(events: &[Event]) -> ReviewState {
        let mut state = ReviewState::default();
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
                    // sortiert nach Ordnung: bei gleicher Nummer gilt die später veröffentlichte
                    let version = e.payload.get("version").and_then(Value::as_i64).unwrap_or(0);
                    state.criteria_versions.insert(version, catalog::upcast(e));
                }
                "AiPolicySet" => {
                    let allowed = e.payload.get("allowed").and_then(Value::as_bool).unwrap_or(true);
                    state.ai_switches.push(AiSwitch { at: e.occurred_at.clone(), allowed });
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
        state.criteria = state.criteria_versions.values().next_back().cloned();
        state
    }

    /// Ob KI-Vorschläge im Review erlaubt sind. Ohne Entscheidung der Leitung sind sie es.
    pub fn ai_allowed(&self) -> bool {
        self.ai_switches.last().is_none_or(|s| s.allowed)
    }

    /// Ob KI-Vorschläge zu einem Zeitpunkt erlaubt waren (`occurred_at` eines Ereignisses).
    pub fn ai_allowed_at(&self, at: &str) -> bool {
        self.ai_switches.iter().rfind(|s| s.at.as_str() <= at).is_none_or(|s| s.allowed)
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
    /// Station der Vorlage; fehlt bei Stufen aus der Zeit vor den Vorlagen.
    pub station: Option<String>,
    pub round: i64,
    pub decision_rule: String,
    pub conflict_rule: String,
    pub verify: Option<Verify>,
    /// Jeder Ausschluss braucht genau einen Hauptgrund aus den Kriterien der Stufe.
    pub reason_required: bool,
    /// Formular einer Formular-Stufe; Screening-Stufen haben keines.
    pub form_id: Option<Uuid>,
    pub form_version: Option<i64>,
}

/// Zweite Stimme für Ausschlüsse bei der Regel `single_verified`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Verify {
    /// `all` oder `sample`.
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
}

impl Verify {
    /// Ob ein Eintrag zur Prüfmenge gehört. Hängt nur an Seed und ID und ist damit für alle gleich.
    pub fn covers(&self, record_id: &str) -> bool {
        if self.scope != "sample" {
            return true;
        }
        let digest = sha256_hex(format!("{}:{record_id}", self.seed.unwrap_or(0)).as_bytes());
        let bucket = u32::from_str_radix(&digest[..8], 16).unwrap_or(0) % 100;
        i64::from(bucket) < self.percent.unwrap_or(100)
    }
}

#[derive(Clone, Debug)]
pub struct VoteInfo {
    pub decision: String,
    pub primary_reason: Option<String>,
    pub reasons: Vec<String>,
    pub note: String,
    pub seq: i64,
}

#[derive(Clone, Debug)]
pub struct ConsensusInfo {
    pub decision: String,
    pub method: String,
    pub primary_reason: Option<String>,
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
    /// (record_id, actor_id) → letzter KI-Vorschlag. Zählt nirgends als Stimme.
    pub suggestions: BTreeMap<(String, Uuid), SuggestionInfo>,
}

/// Ein KI-Vorschlag, wie ihn eine Person zu einem Eintrag eingeholt hat.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SuggestionInfo {
    pub id: Uuid,
    pub model: String,
    pub suggested: String,
    pub reason: Option<String>,
    pub rationale: String,
    /// `accepted`, `declined` oder `None`, solange nichts vermerkt ist.
    pub outcome: Option<String>,
    pub at: String,
}

impl StageState {
    /// Baut den Zustand aus allen Ereignissen mit dieser `stage_instance_id`.
    pub fn build(stage_id: &Uuid, events: &[Event]) -> Option<StageState> {
        let events: Vec<&Event> =
            sorted(events).into_iter().filter(|e| e.stage_instance_id.as_ref() == Some(stage_id)).collect();
        let opened = events.iter().find(|e| e.event_type == "StageInstanceOpened")?;
        let p = &catalog::upcast(opened);
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
            station: s("station"),
            round: i("round").unwrap_or(1),
            decision_rule: s("decision_rule").unwrap_or_else(|| "unanimous".into()),
            conflict_rule: s("conflict_rule").unwrap_or_else(|| "discussion".into()),
            verify: p.get("verify").and_then(|v| serde_json::from_value(v.clone()).ok()),
            reason_required: p.get("reason_required").and_then(Value::as_bool).unwrap_or(false),
            form_id: s("form_id").and_then(|id| Uuid::parse_str(&id).ok()),
            form_version: i("form_version"),
        };
        let mut state = StageState {
            config,
            unblinded: false,
            closed: false,
            votes: BTreeMap::new(),
            consensus: BTreeMap::new(),
            flags: BTreeMap::new(),
            suggestions: BTreeMap::new(),
        };
        for e in events {
            let record = e.payload_str("record_id").map(str::to_string);
            match (e.event_type.as_str(), record) {
                ("StageUnblinded", _) => state.unblinded = true,
                ("StageInstanceClosed", _) => state.closed = true,
                ("VoteCast" | "VoteRevised", Some(record)) => {
                    let info = VoteInfo {
                        decision: e.payload_str("decision").unwrap_or_default().to_string(),
                        primary_reason: e.payload_str("primary_reason").map(str::to_string),
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
                            primary_reason: e.payload_str("primary_reason").map(str::to_string),
                        },
                    );
                }
                ("RecordFlaggedUndecidable", Some(record)) => *state.flags.entry(record).or_default() += 1,
                // Innerhalb einer Kette in der Reihenfolge von `seq`: Der spätere Vorschlag ersetzt den früheren.
                ("AiSuggestionReceived", Some(record)) => {
                    let info = SuggestionInfo {
                        id: e.event_id,
                        model: e.payload_str("model").unwrap_or_default().to_string(),
                        suggested: e.payload_str("suggested").unwrap_or_default().to_string(),
                        reason: e.payload_str("reason").map(str::to_string),
                        rationale: e.payload_str("rationale").unwrap_or_default().to_string(),
                        outcome: None,
                        at: e.occurred_at.clone(),
                    };
                    state.suggestions.insert((record, e.actor_id), info);
                }
                ("AiSuggestionResolved", Some(record)) => {
                    let id = e.payload_str("suggestion").and_then(|id| Uuid::parse_str(id).ok());
                    if let Some(s) = state.suggestions.get_mut(&(record, e.actor_id)).filter(|s| Some(s.id) == id) {
                        s.outcome = e.payload_str("outcome").map(str::to_string);
                    }
                }
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

    /// Hauptgrund eines Ausschlusses: der Grund der Klärung, sonst der gemeinsame Grund der Stimmen.
    /// Nennen die Stimmen verschiedene Gründe, gilt der, der in `order` (Reihenfolge der Kriterien) zuerst steht.
    pub fn exclusion_reason(&self, record: &str, order: &[String]) -> String {
        if let Some(consensus) = self.consensus.get(record) {
            return consensus.primary_reason.clone().unwrap_or_default();
        }
        let named: BTreeSet<&str> = self
            .votes
            .range((record.to_string(), Uuid::nil())..=(record.to_string(), Uuid::max()))
            .filter(|(_, vote)| vote.decision == "exclude")
            .filter_map(|(_, vote)| vote.primary_reason.as_deref())
            .collect();
        order
            .iter()
            .map(String::as_str)
            .find(|code| named.contains(code))
            .or_else(|| named.iter().next().copied())
            .unwrap_or_default()
            .to_string()
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
        let rule = self.config.decision_rule.as_str();
        keys.into_iter()
            .map(|record| {
                let votes = by_record.get(&record).cloned().unwrap_or_default();
                let distinct: BTreeSet<&String> = votes.iter().collect();
                let n_votes = votes.len();
                let (decision, status) = if let Some(c) = self.consensus.get(&record) {
                    (Some(c.decision.clone()), "consensus")
                } else if self.flags.contains_key(&record) {
                    (None, "needs_clarification")
                } else if rule == "liberal" && votes.iter().any(|v| v == "include") {
                    (Some("include".to_string()), "liberal")
                } else if distinct.len() > 1 {
                    (None, "disagreement")
                } else {
                    let value = distinct.into_iter().next().cloned();
                    let excluded = value.as_deref() == Some("exclude");
                    let needed = match rule {
                        "liberal" => min.max(2),
                        "single_verified" if excluded && self.needs_second_vote(&record) => 2,
                        "single_verified" => 1,
                        _ => min,
                    };
                    if n_votes < needed {
                        (None, "pending")
                    } else if rule == "single_verified" && n_votes == 1 {
                        (value, "single")
                    } else {
                        (value, "unanimous")
                    }
                };
                (record, FinalDecision { n_votes, decision, status })
            })
            .collect()
    }

    /// Bei `single_verified`: ob ein Ausschluss dieses Eintrags eine zweite Stimme braucht.
    pub fn needs_second_vote(&self, record_id: &str) -> bool {
        self.config.verify.as_ref().is_none_or(|v| v.covers(record_id))
    }

    /// Ob die Person zu diesem Eintrag noch abstimmen soll. Grundlage sind die Stimmen, die auf
    /// diesem Rechner vorliegen; in einer verblindeten Stufe sind das nur die eigenen.
    pub fn wants_vote_from(&self, record_id: &str, actor: &Uuid) -> bool {
        let votes: Vec<(&Uuid, &VoteInfo)> =
            self.votes.iter().filter(|((r, _), _)| r == record_id).map(|((_, a), v)| (a, v)).collect();
        if votes.iter().any(|(a, _)| *a == actor) {
            return false;
        }
        let mapped = |v: &VoteInfo| match (&self.config.maybe_as, v.decision.as_str()) {
            (Some(to), "maybe") => to.clone(),
            _ => v.decision.clone(),
        };
        match self.config.decision_rule.as_str() {
            "liberal" => !votes.iter().any(|(_, v)| mapped(v) == "include"),
            "single_verified" => {
                votes.is_empty()
                    || (votes.len() == 1 && mapped(votes[0].1) == "exclude" && self.needs_second_vote(record_id))
            }
            _ => true,
        }
    }
}

pub fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    }
}
