//! Vorlagen und Ablauf: Stationen eines Reviews, ihr Zustand, Runden und Gates.
//!
//! Eine Vorlage beschreibt den Ablauf eines Review-Typs. Beim Anlegen wird sie vollständig in
//! `WorkflowVersionPublished` kopiert; danach hängt das Review nicht mehr von der App-Version ab.
//! Reviews ohne dieses Ereignis laufen mit der mitgelieferten Vorlage „Freier Ablauf“, die nichts
//! sperrt.
//!
//! Vier Arten von Stationen: `setup` (arbeitet am Review), `screening` und `form` (Stufeninstanzen)
//! und `report`. Eine Station mit Gate sperrt alle späteren Stationen, bis das Gate erfüllt oder von
//! der Leitung mit Begründung übersteuert ist. Zahlen stehen als Zähler und Nenner im Log, nie als
//! Gleitkommazahl.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::agreement;
use crate::event::Event;
use crate::projection::{order_key, MaybeMode, StageState};

pub const STATION_KINDS: [&str; 4] = ["setup", "screening", "form", "report"];
pub const DECISION_RULES: [&str; 3] = ["unanimous", "liberal", "single_verified"];
pub const CONFLICT_RULES: [&str; 2] = ["discussion", "third_person"];
const SETUP_CHECKS: [&str; 6] = [
    "criteria_published",
    "min_writers",
    "records_imported",
    "search_documented",
    "no_unresolved_duplicates",
    "all_fulltext_status",
];
const SCREENING_CHECKS: [&str; 3] = ["agreement_min", "all_decided", "no_open_conflicts"];
/// Werkzeuge für `setup`-Stationen, die diese App-Version schon hat.
const BUILT_TOOLS: [&str; 4] = ["protocol", "search", "import", "fulltext"];
pub const WORKFLOW_TYPES: [&str; 3] = ["WorkflowVersionPublished", "GateEvaluated", "GateOverridden"];

const BUILTIN: [&str; 2] = [include_str!("../templates/jbi_scoping.json"), include_str!("../templates/free.json")];
pub const FREE_TEMPLATE: &str = "free";

// ---------------------------------------------------------------- Vorlage

/// Eine Regel mit der Angabe, ob sie beim Öffnen einer Stufe geändert werden darf.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule<T> {
    pub value: T,
    #[serde(default)]
    pub editable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    pub decision_rule: Rule<String>,
    pub min_reviewers: Rule<i64>,
    pub maybe_as: Rule<Option<String>>,
    pub conflict_rule: Rule<String>,
    pub blind: Rule<bool>,
}

/// Abweichungen einer Station von den Regeln der Vorlage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuleOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_rule: Option<Rule<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_reviewers: Option<Rule<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maybe_as: Option<Rule<Option<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_rule: Option<Rule<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blind: Option<Rule<bool>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GateCheck {
    pub check: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<i64>,
}

/// Eingangsmenge einer Stufe: alle Einträge, eine Stichprobe oder die Eingeschlossenen einer Station.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Input {
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Station {
    pub key: String,
    pub kind: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gate: Vec<GateCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Input>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<RuleOverrides>,
    /// Mehrere Stufen dürfen gleichzeitig offen sein.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub free_rounds: bool,
    /// Jeder Ausschluss braucht genau einen Hauptgrund (Volltext).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reason_required: bool,
    /// Die Stufe bekommt nur Einträge mit beschafftem Volltext; Nachzügler laufen als weitere Runde.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub requires_fulltext: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchemaElement {
    pub key: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub key: String,
    pub name: String,
    pub review_type: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub terms: BTreeMap<String, String>,
    #[serde(default)]
    pub criteria_schema: Vec<SchemaElement>,
    pub rules: Rules,
    pub stations: Vec<Station>,
    #[serde(default)]
    pub forms: Vec<Value>,
    #[serde(default)]
    pub report: Value,
}

type Check = std::result::Result<(), String>;

fn check_rules(
    decision_rule: Option<&str>,
    min_reviewers: Option<i64>,
    maybe_as: Option<&str>,
    conflict_rule: Option<&str>,
) -> Check {
    if decision_rule.is_some_and(|r| !DECISION_RULES.contains(&r)) {
        return Err(format!("Unbekannte Auswahlregel „{}“.", decision_rule.unwrap_or_default()));
    }
    if min_reviewers.is_some_and(|n| n < 1) {
        return Err("Mindestens eine Stimme je Eintrag ist nötig.".into());
    }
    if maybe_as.is_some_and(|m| !matches!(m, "include" | "exclude")) {
        return Err("Die Maybe-Regel muss leer, include oder exclude sein.".into());
    }
    if conflict_rule.is_some_and(|r| !CONFLICT_RULES.contains(&r)) {
        return Err(format!("Unbekannte Klärungsregel „{}“.", conflict_rule.unwrap_or_default()));
    }
    Ok(())
}

impl Template {
    /// Liest eine Vorlage und prüft, dass diese App-Version jeden Teil versteht.
    pub fn parse(value: &Value) -> std::result::Result<Template, String> {
        let template: Template =
            serde_json::from_value(value.clone()).map_err(|e| format!("Die Vorlage ist nicht lesbar: {e}."))?;
        template.check()?;
        Ok(template)
    }

    fn check(&self) -> Check {
        if self.key.trim().is_empty() || self.name.trim().is_empty() {
            return Err("Der Vorlage fehlen Schlüssel oder Name.".into());
        }
        let r = &self.rules;
        check_rules(
            Some(&r.decision_rule.value),
            Some(r.min_reviewers.value),
            r.maybe_as.value.as_deref(),
            Some(&r.conflict_rule.value),
        )?;
        if self.stations.is_empty() {
            return Err("Die Vorlage hat keine Stationen.".into());
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut screening: BTreeSet<&str> = BTreeSet::new();
        for s in &self.stations {
            let name = &s.key;
            if name.trim().is_empty() || s.label.trim().is_empty() {
                return Err("Einer Station fehlen Schlüssel oder Name.".into());
            }
            if !seen.insert(name) {
                return Err(format!("Die Station „{name}“ steht doppelt in der Vorlage."));
            }
            if !STATION_KINDS.contains(&s.kind.as_str()) {
                return Err(format!("Station „{name}“: unbekannte Art „{}“.", s.kind));
            }
            let allowed: &[&str] = match s.kind.as_str() {
                "setup" => &SETUP_CHECKS,
                "screening" => &SCREENING_CHECKS,
                _ => &[],
            };
            for g in &s.gate {
                if !allowed.contains(&g.check.as_str()) {
                    return Err(format!("Station „{name}“: unbekannte Prüfung „{}“.", g.check));
                }
                if g.check == "agreement_min" && !g.percent.is_some_and(|p| (1..=100).contains(&p)) {
                    return Err(format!("Station „{name}“: Die Schwelle muss zwischen 1 und 100 Prozent liegen."));
                }
                if g.check == "min_writers" && !g.n.is_some_and(|n| n >= 1) {
                    return Err(format!("Station „{name}“: Die Mindestzahl an Personen fehlt."));
                }
            }
            if let Some(o) = &s.rules {
                check_rules(
                    o.decision_rule.as_ref().map(|r| r.value.as_str()),
                    o.min_reviewers.as_ref().map(|r| r.value),
                    o.maybe_as.as_ref().and_then(|r| r.value.as_deref()),
                    o.conflict_rule.as_ref().map(|r| r.value.as_str()),
                )
                .map_err(|e| format!("Station „{name}“: {e}"))?;
            }
            if let Some(input) = &s.input {
                match input.scope.as_str() {
                    "all" | "sample" => {}
                    "from_station" => {
                        if !input.station.as_deref().is_some_and(|k| screening.contains(k)) {
                            return Err(format!(
                                "Station „{name}“: Die Eingangsstation muss eine frühere Screening-Station sein."
                            ));
                        }
                    }
                    other => return Err(format!("Station „{name}“: unbekannter Umfang „{other}“.")),
                }
            }
            if s.requires_fulltext && s.input.as_ref().is_none_or(|i| i.scope != "from_station") {
                return Err(format!("Station „{name}“: Volltexte setzen eine Eingangsstation voraus."));
            }
            if s.kind == "screening" {
                screening.insert(name);
            }
        }
        Ok(())
    }

    pub fn station(&self, key: &str) -> Option<&Station> {
        self.stations.iter().find(|s| s.key == key)
    }

    /// Stufen ohne Angabe einer Station zählen zur ersten Screening-Station.
    pub fn default_station(&self) -> Option<&Station> {
        self.stations.iter().find(|s| s.kind == "screening")
    }

    /// Regeln einer Station: Vorgaben der Vorlage, überschrieben durch die der Station.
    pub fn rules_for(&self, station: &Station) -> Rules {
        let mut rules = self.rules.clone();
        if let Some(o) = &station.rules {
            if let Some(r) = &o.decision_rule {
                rules.decision_rule = r.clone();
            }
            if let Some(r) = &o.min_reviewers {
                rules.min_reviewers = r.clone();
            }
            if let Some(r) = &o.maybe_as {
                rules.maybe_as = r.clone();
            }
            if let Some(r) = &o.conflict_rule {
                rules.conflict_rule = r.clone();
            }
            if let Some(r) = &o.blind {
                rules.blind = r.clone();
            }
        }
        rules
    }

    pub fn term<'a>(&'a self, key: &'a str) -> &'a str {
        self.terms.get(key).map_or(key, String::as_str)
    }
}

/// Mitgelieferte Vorlagen, in der Reihenfolge der Auswahl beim Anlegen.
pub fn builtin_templates() -> Vec<Template> {
    BUILTIN
        .iter()
        .map(|text| {
            let value: Value = serde_json::from_str(text).expect("mitgelieferte Vorlage ist gültiges JSON");
            Template::parse(&value).expect("mitgelieferte Vorlage ist gültig")
        })
        .collect()
}

pub fn builtin_template(key: &str) -> Option<Template> {
    builtin_templates().into_iter().find(|t| t.key == key)
}

/// Kurzangaben für die Auswahl beim Anlegen.
#[derive(Clone, Debug, Serialize)]
pub struct TemplateInfo {
    pub key: String,
    pub name: String,
    pub review_type: String,
    pub description: String,
    pub stations: Vec<String>,
}

pub fn template_infos() -> Vec<TemplateInfo> {
    builtin_templates()
        .into_iter()
        .map(|t| TemplateInfo {
            stations: t.stations.iter().map(|s| s.label.clone()).collect(),
            key: t.key,
            name: t.name,
            review_type: t.review_type,
            description: t.description,
        })
        .collect()
}

// ---------------------------------------------------------------- Gates

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckResult {
    pub check: String,
    pub passed: bool,
    pub numerator: i64,
    pub denominator: i64,
    /// Schwelle in Prozent, nur bei `agreement_min`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold_percent: Option<i64>,
    /// Anteil in Hundertstel Prozent, abgerundet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent_x100: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GateResult {
    pub passed: bool,
    pub checks: Vec<CheckResult>,
}

/// Wertet das Gate einer Screening-Station für eine Stufe aus. `assigned` sind die Einträge der Stufe.
pub fn evaluate_gate(station: &Station, stage: &StageState, assigned: &[String]) -> GateResult {
    let finals = stage.final_decisions();
    let total = assigned.len() as i64;
    let checks: Vec<CheckResult> = station
        .gate
        .iter()
        .map(|g| match g.check.as_str() {
            "agreement_min" => {
                let (agree, n) = agreement::agreement_counts(&stage.stage_votes(&MaybeMode::Stage));
                let threshold = g.percent.unwrap_or(100);
                CheckResult {
                    check: g.check.clone(),
                    passed: n > 0 && agree * 100 >= threshold * n,
                    numerator: agree,
                    denominator: n,
                    threshold_percent: Some(threshold),
                    percent_x100: (n > 0).then(|| agree * 10_000 / n),
                }
            }
            "all_decided" => {
                let decided = assigned.iter().filter(|r| finals.get(*r).is_some_and(|d| d.decision.is_some())).count();
                CheckResult {
                    check: g.check.clone(),
                    passed: decided as i64 == total,
                    numerator: decided as i64,
                    denominator: total,
                    threshold_percent: None,
                    percent_x100: None,
                }
            }
            _ => {
                let open =
                    finals.values().filter(|d| matches!(d.status, "disagreement" | "needs_clarification")).count();
                CheckResult {
                    check: g.check.clone(),
                    passed: open == 0,
                    numerator: open as i64,
                    denominator: total,
                    threshold_percent: None,
                    percent_x100: None,
                }
            }
        })
        .collect();
    GateResult { passed: checks.iter().all(|c| c.passed), checks }
}

/// Eine Zahl in Hundertstel als Text mit einer Nachkommastelle, z. B. 6800 → „68,0“.
fn tenths(x100: i64) -> String {
    let t = (x100 + 5) / 10;
    format!("{},{}", t / 10, t % 10)
}

// ---------------------------------------------------------------- Zustand

#[derive(Clone, Debug, Serialize)]
pub struct Override {
    pub station: String,
    pub round: i64,
    pub reason: String,
    pub actor_id: Uuid,
    pub occurred_at: String,
}

/// Vorlage, Gate-Ergebnisse und Übersteuerungen eines Reviews, aus dem Log berechnet.
#[derive(Clone, Debug)]
pub struct Workflow {
    /// 0, wenn das Review keine Vorlage veröffentlicht hat und im freien Ablauf läuft.
    pub version: i64,
    pub template: Template,
    /// Stufe → letztes Gate-Ergebnis.
    pub gates: BTreeMap<Uuid, GateResult>,
    pub overrides: Vec<Override>,
}

/// Was der Ablauf über das Review wissen muss, um `setup`-Stationen zu bewerten.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub criteria_published: bool,
    pub n_writers: i64,
    /// Einträge ohne Dubletten.
    pub n_records: i64,
    pub n_searches: i64,
    /// Mögliche Dubletten, über die noch nicht entschieden ist.
    pub open_duplicates: i64,
    /// Einträge, die einen Volltext brauchen: die Eingeschlossenen der Station vor dem Volltext.
    pub fulltext_needed: i64,
    pub fulltext_retrieved: i64,
    pub fulltext_unavailable: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckView {
    pub check: String,
    pub label: String,
    /// `None`, solange es nichts auszuwerten gibt.
    pub passed: Option<bool>,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RoundView {
    pub round: i64,
    pub stage_id: Uuid,
    pub name: String,
    pub closed: bool,
    pub gate: Option<GateResult>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StationRef {
    pub key: String,
    pub label: String,
}

/// Voreinstellungen für die nächste Stufe einer Station.
#[derive(Clone, Debug, Serialize)]
pub struct StageDefaults {
    pub name: String,
    pub round: i64,
    pub scope: String,
    pub sample_size: Option<i64>,
    pub source_stage: Option<Uuid>,
    pub rules: Rules,
    pub reason_required: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct StationView {
    pub key: String,
    pub kind: String,
    pub label: String,
    pub description: String,
    pub tool: Option<String>,
    /// `locked`, `open`, `running` oder `done`.
    pub state: &'static str,
    /// Ob diese App-Version die Station schon bearbeiten kann.
    pub available: bool,
    /// Ob die Station spätere Stationen sperrt, solange sie nicht abgeschlossen ist.
    pub blocking: bool,
    pub blocked_by: Vec<StationRef>,
    pub checks: Vec<CheckView>,
    pub rounds: Vec<RoundView>,
    pub overridden: Option<String>,
    pub free_rounds: bool,
    pub defaults: Option<StageDefaults>,
}

impl Workflow {
    pub fn build(events: &[Event]) -> Workflow {
        let mut sorted: Vec<&Event> = events.iter().collect();
        sorted.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
        let mut published: Option<(i64, Template)> = None;
        let mut gates = BTreeMap::new();
        let mut overrides = Vec::new();
        for e in sorted {
            match e.event_type.as_str() {
                "WorkflowVersionPublished" => {
                    let version = e.payload.get("version").and_then(Value::as_i64).unwrap_or(0);
                    let template = e.payload.get("template").and_then(|t| Template::parse(t).ok());
                    if let Some(template) = template {
                        if published.as_ref().is_none_or(|(v, _)| version >= *v) {
                            published = Some((version, template));
                        }
                    }
                }
                "GateEvaluated" => {
                    let result = serde_json::from_value::<GateResult>(e.payload.clone()).ok();
                    if let (Some(stage), Some(result)) = (e.stage_instance_id, result) {
                        gates.insert(stage, result);
                    }
                }
                "GateOverridden" => overrides.push(Override {
                    station: e.payload_str("station").unwrap_or_default().to_string(),
                    round: e.payload.get("round").and_then(Value::as_i64).unwrap_or(0),
                    reason: e.payload_str("reason").unwrap_or_default().to_string(),
                    actor_id: e.actor_id,
                    occurred_at: e.occurred_at.clone(),
                }),
                _ => {}
            }
        }
        let (version, template) = published.unwrap_or_else(|| {
            (0, builtin_template(FREE_TEMPLATE).expect("die Vorlage „Freier Ablauf“ wird mitgeliefert"))
        });
        Workflow { version, template, gates, overrides }
    }

    pub fn published(&self) -> bool {
        self.version > 0
    }

    /// Station einer Stufe; ohne Angabe oder mit unbekanntem Schlüssel die erste Screening-Station.
    pub fn station_of(&self, stage: &StageState) -> Option<&Station> {
        stage
            .config
            .station
            .as_deref()
            .and_then(|k| self.template.station(k))
            .filter(|s| matches!(s.kind.as_str(), "screening" | "form"))
            .or_else(|| self.template.default_station())
    }

    fn setup_check(&self, g: &GateCheck, facts: &Facts) -> CheckView {
        let (label, passed, detail) = match g.check.as_str() {
            "criteria_published" => ("Kriterien veröffentlicht".to_string(), facts.criteria_published, String::new()),
            "min_writers" => {
                let n = g.n.unwrap_or(1);
                (
                    format!("Mindestens {n} Personen mit Schreibrecht"),
                    facts.n_writers >= n,
                    format!("{} vorhanden", facts.n_writers),
                )
            }
            "search_documented" => (
                "Suche dokumentiert".to_string(),
                facts.n_searches > 0,
                match facts.n_searches {
                    1 => "1 Suche".to_string(),
                    n => format!("{n} Suchen"),
                },
            ),
            "all_fulltext_status" => (
                "Jeder Volltext beschafft oder als nicht beschaffbar vermerkt".to_string(),
                facts.fulltext_needed > 0
                    && facts.fulltext_retrieved + facts.fulltext_unavailable >= facts.fulltext_needed,
                format!(
                    "{} von {} beschafft, {} nicht beschaffbar",
                    facts.fulltext_retrieved, facts.fulltext_needed, facts.fulltext_unavailable
                ),
            ),
            "no_unresolved_duplicates" => (
                "Mögliche Dubletten entschieden".to_string(),
                facts.open_duplicates == 0,
                format!("{} offen", facts.open_duplicates),
            ),
            _ => (
                "Treffer importiert".to_string(),
                facts.n_records > 0,
                format!("{} {}", facts.n_records, self.template.term("records")),
            ),
        };
        CheckView { check: g.check.clone(), label, passed: Some(passed), detail }
    }

    fn screening_check(&self, g: &GateCheck, result: Option<&CheckResult>) -> CheckView {
        let records = self.template.term("records");
        let label = match g.check.as_str() {
            "agreement_min" => format!("Übereinstimmung mindestens {} %", g.percent.unwrap_or(100)),
            "all_decided" => format!("Alle {records} entschieden"),
            _ => "Keine offenen Konflikte".to_string(),
        };
        let detail = match (g.check.as_str(), result) {
            (_, None) => String::new(),
            ("agreement_min", Some(r)) => match r.percent_x100 {
                Some(p) => format!("{} % bei {} doppelt bewerteten {records}n", tenths(p), r.denominator),
                None => format!("keine doppelt bewerteten {records}"),
            },
            ("all_decided", Some(r)) => format!("{} von {}", r.numerator, r.denominator),
            (_, Some(r)) => format!("{} offen", r.numerator),
        };
        CheckView { check: g.check.clone(), label, passed: result.map(|r| r.passed), detail }
    }

    /// Stationen der Vorlage mit Zustand. `stages` sind alle Stufen des Reviews in der Reihenfolge
    /// ihrer Eröffnung.
    pub fn stations(&self, facts: &Facts, stages: &[StageState]) -> Vec<StationView> {
        let mut out: Vec<StationView> = Vec::new();
        let mut blockers: Vec<StationRef> = Vec::new();
        for station in &self.template.stations {
            let mut rounds: Vec<RoundView> = stages
                .iter()
                .filter(|s| self.station_of(s).is_some_and(|x| x.key == station.key))
                .map(|s| RoundView {
                    round: s.config.round,
                    stage_id: s.config.id,
                    name: s.config.name.clone(),
                    closed: s.closed,
                    gate: self.gates.get(&s.config.id).cloned(),
                })
                .collect();
            rounds.sort_by_key(|r| r.round);
            let last = rounds.last();
            let last_round = last.map_or(0, |r| r.round);
            let any_open = rounds.iter().any(|r| !r.closed);
            let overridden = self
                .overrides
                .iter()
                .rev()
                .find(|o| o.station == station.key && (station.kind != "screening" || o.round == last_round))
                .map(|o| o.reason.clone());
            let checks: Vec<CheckView> = match station.kind.as_str() {
                "setup" => station.gate.iter().map(|g| self.setup_check(g, facts)).collect(),
                "screening" => {
                    let result = last.filter(|r| r.closed).and_then(|r| r.gate.as_ref());
                    station
                        .gate
                        .iter()
                        .map(|g| {
                            self.screening_check(g, result.and_then(|r| r.checks.iter().find(|c| c.check == g.check)))
                        })
                        .collect()
                }
                _ => Vec::new(),
            };
            let blocking = !station.gate.is_empty();
            let passed = !checks.is_empty() && checks.iter().all(|c| c.passed == Some(true));
            let done = match station.kind.as_str() {
                "setup" => (blocking && passed) || overridden.is_some(),
                "screening" => match last {
                    None => overridden.is_some(),
                    Some(r) => !any_open && r.closed && (!blocking || passed || overridden.is_some()),
                },
                _ => false,
            };
            let state = if done {
                "done"
            } else if any_open {
                "running"
            } else if !blockers.is_empty() {
                "locked"
            } else if station.kind == "setup" && checks.iter().any(|c| c.passed == Some(true)) {
                "running"
            } else {
                "open"
            };
            let available = match station.kind.as_str() {
                "screening" => true,
                "setup" => station.tool.as_deref().is_some_and(|t| BUILT_TOOLS.contains(&t)),
                _ => false,
            };
            let defaults = (station.kind == "screening").then(|| {
                let input = station.input.as_ref();
                let round = last_round + 1;
                let source_stage = input.and_then(|i| i.station.as_deref()).and_then(|key| {
                    stages
                        .iter()
                        .filter(|s| self.station_of(s).is_some_and(|x| x.key == key))
                        .max_by_key(|s| s.config.round)
                        .map(|s| s.config.id)
                });
                StageDefaults {
                    name: if round > 1 && !station.free_rounds {
                        format!("{} (Runde {round})", station.label)
                    } else {
                        station.label.clone()
                    },
                    round,
                    scope: match input.map(|i| i.scope.as_str()) {
                        Some("from_station") => "from_stage".to_string(),
                        Some(scope) => scope.to_string(),
                        None => "all".to_string(),
                    },
                    sample_size: input.and_then(|i| i.sample_size),
                    source_stage,
                    rules: self.template.rules_for(station),
                    reason_required: station.reason_required,
                }
            });
            out.push(StationView {
                key: station.key.clone(),
                kind: station.kind.clone(),
                label: station.label.clone(),
                description: station.description.clone(),
                tool: station.tool.clone(),
                state,
                available,
                blocking,
                blocked_by: if done { Vec::new() } else { blockers.clone() },
                checks,
                rounds,
                overridden,
                free_rounds: station.free_rounds,
                defaults,
            });
            if blocking && !done {
                blockers.push(StationRef { key: station.key.clone(), label: station.label.clone() });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builtin_templates_are_valid_and_differ() {
        let templates = builtin_templates();
        assert_eq!(templates.iter().map(|t| t.key.as_str()).collect::<Vec<_>>(), ["jbi_scoping", "free"]);
        let jbi = &templates[0];
        assert_eq!(jbi.stations.len(), 12);
        let pilot = jbi.station("pilot_title_abstract").unwrap();
        assert_eq!(pilot.gate[0].percent, Some(75));
        // Der Pilot bleibt einstimmig, auch wenn das Review sonst eine andere Regel wählt.
        assert!(!jbi.rules_for(pilot).decision_rule.editable);
        assert!(jbi.rules_for(jbi.station("title_abstract").unwrap()).decision_rule.editable);
        assert!(templates[1].stations.iter().all(|s| s.gate.is_empty()), "der freie Ablauf sperrt nichts");
    }

    #[test]
    fn template_survives_the_log_unchanged() {
        for t in builtin_templates() {
            let value = serde_json::to_value(&t).unwrap();
            assert!(crate::canonical::to_canonical(&value).is_ok(), "keine Gleitkommazahlen in {}", t.key);
            assert_eq!(Template::parse(&value).unwrap(), t);
        }
    }

    #[test]
    fn broken_templates_are_refused() {
        let good = serde_json::to_value(builtin_template("jbi_scoping").unwrap()).unwrap();
        let cases: [(&str, Value, &str); 6] = [
            ("/stations/0/kind", json!("tanz"), "unbekannte Art"),
            ("/stations/0/gate/0/check", json!("mondphase"), "unbekannte Prüfung"),
            ("/stations/1/key", json!("protocol"), "doppelt"),
            ("/rules/decision_rule/value", json!("würfeln"), "Auswahlregel"),
            ("/stations/3/gate/0/percent", json!(140), "zwischen 1 und 100"),
            ("/stations/6/input/station", json!("report"), "frühere Screening-Station"),
        ];
        for (pointer, value, expected) in cases {
            let mut broken = good.clone();
            *broken.pointer_mut(pointer).unwrap() = value;
            let err = Template::parse(&broken).unwrap_err();
            assert!(err.contains(expected), "{pointer}: {err}");
        }
        assert!(Template::parse(&json!({"key": "x"})).unwrap_err().contains("nicht lesbar"));
    }

    #[test]
    fn tenths_rounds_to_one_decimal() {
        assert_eq!(tenths(6800), "68,0");
        assert_eq!(tenths(7496), "75,0");
        assert_eq!(tenths(10000), "100,0");
        assert_eq!(tenths(333), "3,3");
    }
}
