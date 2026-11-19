//! Register aller Ereignistypen: Kette, Schreibrecht, Bezug zur Stufe und Prüfung des Payloads.
//!
//! Jeder Typ steht genau hier. [`crate::store`] liest daraus die Kette, [`crate::exchange`] die
//! Verblindung und die Rechte beim Einlesen, [`crate::service`] prüft damit jeden Payload vor dem
//! Anhängen. Ein neuer Ereignistyp braucht einen Eintrag in [`CATALOG`] und sonst keine Liste.
//!
//! Die Payload-Prüfung verlangt nur, was die Projektionen brauchen. Zusätzliche Felder sind erlaubt,
//! damit ein Typ wachsen kann, ohne dass ältere Ereignisse ungültig werden.

use serde_json::{json, Value};
use uuid::Uuid;

use crate::criteria::valid_code;
use crate::event::Event;
use crate::service::ROLES;
use crate::workflow::{GateResult, Template, CONFLICT_RULES, DECISION_RULES};

/// In welcher Kette ein Ereignis liegt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chain {
    /// Verwaltung und alles, was nie verblindet wird.
    Main,
    /// Kette je Person und Stufe. Ein Paket lässt sie bei verblindeten Stufen für andere ganz weg.
    Stage,
}

/// Wer einen Typ schreiben darf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    /// Nur als erstes Ereignis des Reviews.
    Creator,
    /// Owner und Admins.
    Manage,
    /// Owner, Admins und Reviewer.
    Write,
}

/// Was für die Stufe gelten muss, auf die sich ein Ereignis bezieht.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageRule {
    /// Kein Bezug zu einer Stufe.
    None,
    /// Stufe muss angegeben sein (Ereignisse, die eine Stufe öffnen, entblinden oder schließen).
    Required,
    /// Stufe muss bekannt und offen sein.
    Open,
    /// Stufe muss bekannt, offen und entblindet sein.
    OpenUnblinded,
}

type Check = std::result::Result<(), String>;

pub struct Spec {
    pub name: &'static str,
    /// Höchste Schema-Version, die diese App schreibt und versteht.
    pub version: i64,
    pub chain: Chain,
    pub writer: Writer,
    pub stage: StageRule,
    check: fn(&Value) -> Check,
}

pub static CATALOG: [Spec; 15] = [
    Spec {
        name: "ReviewCreated",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Creator,
        stage: StageRule::None,
        check: review_created,
    },
    Spec {
        name: "MemberRoleSet",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: member_role_set,
    },
    Spec {
        name: "CriteriaVersionPublished",
        version: 2,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: criteria_version_published,
    },
    Spec {
        name: "RecordsImported",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: records_imported,
    },
    Spec {
        name: "StageInstanceOpened",
        version: 2,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::Required,
        check: stage_instance_opened,
    },
    Spec {
        name: "StageUnblinded",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::Required,
        check: any_object,
    },
    Spec {
        name: "StageInstanceClosed",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::Required,
        check: any_object,
    },
    Spec {
        name: "SnapshotTaken",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::Required,
        check: snapshot_taken,
    },
    Spec {
        name: "VoteCast",
        version: 1,
        chain: Chain::Stage,
        writer: Writer::Write,
        stage: StageRule::Open,
        check: vote,
    },
    Spec {
        name: "VoteRevised",
        version: 1,
        chain: Chain::Stage,
        writer: Writer::Write,
        stage: StageRule::Open,
        check: vote,
    },
    Spec {
        name: "RecordFlaggedUndecidable",
        version: 1,
        chain: Chain::Stage,
        writer: Writer::Write,
        stage: StageRule::Open,
        check: record_flagged,
    },
    Spec {
        name: "ConsensusRecorded",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Write,
        stage: StageRule::OpenUnblinded,
        check: consensus_recorded,
    },
    Spec {
        name: "WorkflowVersionPublished",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: workflow_version_published,
    },
    Spec {
        name: "GateEvaluated",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::Required,
        check: gate_evaluated,
    },
    Spec {
        name: "GateOverridden",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: gate_overridden,
    },
];

pub fn spec(event_type: &str) -> Option<&'static Spec> {
    CATALOG.iter().find(|s| s.name == event_type)
}

/// Ereignisse in der Kette einer Stufe. Ein Paket lässt sie weg, solange die Stufe verblindet ist.
pub fn is_blindable(event_type: &str) -> bool {
    spec(event_type).is_some_and(|s| s.chain == Chain::Stage)
}

/// Prüft Typ, Schema-Version, Stufenbezug und Payload. Der Text der Ablehnung ist für Menschen.
pub fn validate(event_type: &str, schema_version: i64, stage_id: Option<&Uuid>, payload: &Value) -> Check {
    let spec = spec(event_type).ok_or_else(|| format!("Unbekannter Ereignistyp {event_type}."))?;
    if schema_version > spec.version {
        return Err(format!("{event_type} stammt aus einer neueren Version der App."));
    }
    if schema_version < 1 {
        return Err(format!("{event_type} hat eine ungültige Schema-Version."));
    }
    if spec.stage != StageRule::None && stage_id.is_none() {
        return Err(format!("{event_type} braucht eine Stufe."));
    }
    if !payload.is_object() {
        return Err(format!("{event_type}: Der Inhalt ist kein Objekt."));
    }
    (spec.check)(payload).map_err(|reason| format!("{event_type}: {reason}"))
}

pub fn validate_event(e: &Event) -> Check {
    validate(&e.event_type, e.schema_version, e.stage_instance_id.as_ref(), &e.payload)
}

/// Payload in der aktuellen Form des Typs. Das Ereignis selbst bleibt unverändert, weil Prüfsumme und
/// Signatur über dem ursprünglichen Inhalt liegen. Projektionen lesen deshalb über diese Funktion.
pub fn upcast(e: &Event) -> Value {
    let mut payload = e.payload.clone();
    match e.event_type.as_str() {
        "CriteriaVersionPublished" => upcast_criteria(&mut payload),
        "StageInstanceOpened" => upcast_stage(&mut payload),
        _ => {}
    }
    payload
}

/// Stufen aus Version 1 kennen weder Station noch Auswahl- und Klärungsregel. Sie galten als
/// einstimmig mit Klärung durch Diskussion; das steht nach der Umwandlung ausdrücklich da.
fn upcast_stage(payload: &mut Value) {
    let Some(map) = payload.as_object_mut() else { return };
    map.entry("round").or_insert_with(|| Value::from(1));
    map.entry("decision_rule").or_insert_with(|| Value::from("unanimous"));
    map.entry("conflict_rule").or_insert_with(|| Value::from("discussion"));
    map.entry("reason_required").or_insert_with(|| Value::from(false));
}

/// Version 1 kannte Freitext und frei benannte Ausschlussgründe. Der Text wird zu einem Kriterium
/// ohne Kürzel; jeder alte Grund wird ein Ausschlusskriterium, dessen Kürzel der alte Wortlaut ist,
/// damit die Gründe in Stimmen alter Stufen weiter passen.
fn upcast_criteria(payload: &mut Value) {
    let Some(map) = payload.as_object_mut() else { return };
    if !map.get("items").is_some_and(Value::is_array) {
        let mut items = Vec::new();
        let text = map.get("text").and_then(Value::as_str).unwrap_or_default().trim().to_string();
        if !text.is_empty() {
            items.push(json!({"code": "", "element": "", "kind": "include", "text": text, "note": ""}));
        }
        for reason in map.get("exclusion_reasons").and_then(Value::as_array).into_iter().flatten() {
            if let Some(reason) = reason.as_str() {
                items.push(json!({"code": reason, "element": "", "kind": "exclude", "text": reason, "note": ""}));
            }
        }
        map.insert("items".into(), Value::Array(items));
    }
    for key in ["guidance", "change_note"] {
        map.entry(key).or_insert_with(|| Value::String(String::new()));
    }
    for key in ["include_keywords", "exclude_keywords"] {
        map.entry(key).or_insert_with(|| Value::Array(Vec::new()));
    }
}

// ---------------------------------------------------------------- Bausteine der Prüfung

fn text<'a>(p: &'a Value, key: &str) -> std::result::Result<&'a str, String> {
    p.get(key).and_then(Value::as_str).ok_or_else(|| format!("Feld „{key}“ fehlt oder ist kein Text."))
}

fn filled(p: &Value, key: &str) -> Check {
    if text(p, key)?.trim().is_empty() {
        return Err(format!("Feld „{key}“ ist leer."));
    }
    Ok(())
}

fn uuid(p: &Value, key: &str) -> Check {
    Uuid::parse_str(text(p, key)?).map(|_| ()).map_err(|_| format!("Feld „{key}“ ist keine gültige ID."))
}

fn integer(p: &Value, key: &str, min: i64) -> Check {
    match p.get(key).and_then(Value::as_i64) {
        Some(n) if n >= min => Ok(()),
        Some(_) => Err(format!("Feld „{key}“ ist kleiner als {min}.")),
        None => Err(format!("Feld „{key}“ fehlt oder ist keine ganze Zahl.")),
    }
}

fn one_of(p: &Value, key: &str, allowed: &[&str]) -> Check {
    let value = text(p, key)?;
    if !allowed.contains(&value) {
        return Err(format!("Feld „{key}“ hat den unzulässigen Wert „{value}“."));
    }
    Ok(())
}

/// Fehlt das Feld oder ist es `null`, ist das in Ordnung.
fn optional(p: &Value, key: &str, check: impl FnOnce(&Value, &str) -> Check) -> Check {
    match p.get(key) {
        None | Some(Value::Null) => Ok(()),
        Some(_) => check(p, key),
    }
}

fn text_list(p: &Value, key: &str) -> Check {
    match p.get(key) {
        Some(Value::Array(items)) if items.iter().all(Value::is_string) => Ok(()),
        _ => Err(format!("Feld „{key}“ ist keine Liste von Texten.")),
    }
}

fn is_text(p: &Value, key: &str) -> Check {
    text(p, key).map(|_| ())
}

// ---------------------------------------------------------------- Prüfung je Typ

fn any_object(_: &Value) -> Check {
    Ok(())
}

fn review_created(p: &Value) -> Check {
    is_text(p, "title")?;
    uuid(p, "owner")?;
    filled(p, "public_key")?;
    optional(p, "review_type", filled)
}

fn member_role_set(p: &Value) -> Check {
    uuid(p, "actor_id")?;
    one_of(p, "role", &ROLES)?;
    optional(p, "public_key", filled)
}

fn criteria_version_published(p: &Value) -> Check {
    integer(p, "version", 1)?;
    optional(p, "text", is_text)?;
    for key in ["include_keywords", "exclude_keywords", "exclusion_reasons"] {
        optional(p, key, text_list)?;
    }
    optional(p, "guidance", is_text)?;
    // Version 1 hat keine Liste; sie bleibt gültig und wird beim Lesen umgeformt.
    let items = match p.get("items") {
        None | Some(Value::Null) => return Ok(()),
        Some(Value::Array(items)) => items,
        Some(_) => return Err("Feld „items“ ist keine Liste.".into()),
    };
    let mut codes = std::collections::BTreeSet::new();
    for item in items {
        let code = text(item, "code")?;
        if !valid_code(code) {
            return Err(format!("Das Kürzel „{code}“ braucht ein bis acht Buchstaben oder Ziffern."));
        }
        if !codes.insert(code) {
            return Err(format!("Das Kürzel „{code}“ kommt mehrfach vor."));
        }
        one_of(item, "kind", &["include", "exclude"])?;
        filled(item, "text").map_err(|e| format!("Kriterium {code}: {e}"))?;
        optional(item, "element", is_text)?;
        optional(item, "note", is_text)?;
    }
    Ok(())
}

fn records_imported(p: &Value) -> Check {
    uuid(p, "batch_id")?;
    filled(p, "records_sha256")?;
    integer(p, "n_records", 0)
}

fn stage_instance_opened(p: &Value) -> Check {
    filled(p, "name")?;
    if !p.get("blind").is_some_and(Value::is_boolean) {
        return Err("Feld „blind“ fehlt oder ist kein Wahrheitswert.".into());
    }
    integer(p, "min_reviewers", 1)?;
    optional(p, "maybe_as", |p, key| one_of(p, key, &["include", "exclude"]))?;
    integer(p, "n_records", 0)?;
    filled(p, "record_ids_sha256")?;
    optional(p, "station", filled)?;
    optional(p, "round", |p, key| integer(p, key, 1))?;
    optional(p, "decision_rule", |p, key| one_of(p, key, &DECISION_RULES))?;
    optional(p, "conflict_rule", |p, key| one_of(p, key, &CONFLICT_RULES))?;
    optional(p, "reason_required", |p, key| match p[key].is_boolean() {
        true => Ok(()),
        false => Err(format!("Feld „{key}“ ist kein Wahrheitswert.")),
    })?;
    optional(p, "verify", |p, key| {
        let v = &p[key];
        one_of(v, "scope", &["all", "sample"]).map_err(|e| format!("Feld „{key}“: {e}"))?;
        if v["scope"] == "sample" {
            let percent = v.get("percent").and_then(Value::as_i64);
            if !percent.is_some_and(|n| (1..=100).contains(&n)) {
                return Err(format!("Feld „{key}“: Der Anteil der Stichprobe muss zwischen 1 und 100 Prozent liegen."));
            }
            integer(v, "seed", 0).map_err(|e| format!("Feld „{key}“: {e}"))?;
        }
        Ok(())
    })
}

fn workflow_version_published(p: &Value) -> Check {
    integer(p, "version", 1)?;
    let template = p.get("template").ok_or("Feld „template“ fehlt.")?;
    Template::parse(template).map(|_| ())
}

fn gate_evaluated(p: &Value) -> Check {
    filled(p, "station")?;
    integer(p, "round", 1)?;
    serde_json::from_value::<GateResult>(p.clone())
        .map(|_| ())
        .map_err(|_| "Das Ergebnis des Gates ist nicht lesbar.".to_string())
}

fn gate_overridden(p: &Value) -> Check {
    filled(p, "station")?;
    integer(p, "round", 0)?;
    filled(p, "reason")
}

fn snapshot_taken(p: &Value) -> Check {
    filled(p, "hash")?;
    if !p.get("heads").is_some_and(Value::is_array) {
        return Err("Feld „heads“ fehlt oder ist keine Liste.".into());
    }
    Ok(())
}

fn vote(p: &Value) -> Check {
    uuid(p, "record_id")?;
    one_of(p, "decision", &["include", "exclude", "maybe"])?;
    optional(p, "primary_reason", filled)?;
    optional(p, "reasons", text_list)?;
    optional(p, "note", is_text)
}

fn record_flagged(p: &Value) -> Check {
    uuid(p, "record_id")?;
    optional(p, "note", is_text)
}

fn consensus_recorded(p: &Value) -> Check {
    uuid(p, "record_id")?;
    one_of(p, "decision", &["include", "exclude"])?;
    optional(p, "primary_reason", filled)?;
    optional(p, "reasons", text_list)?;
    optional(p, "note", is_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_type_is_listed_once() {
        let mut names: Vec<&str> = CATALOG.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), CATALOG.len());
    }

    #[test]
    fn upcast_fills_missing_criteria_fields_without_touching_the_event() {
        let mut e: Event = serde_json::from_value(json!({
            "event_id": Uuid::new_v4(), "review_id": Uuid::new_v4(), "actor_id": Uuid::new_v4(), "stream": "main",
            "seq": 1, "type": "CriteriaVersionPublished", "schema_version": 1, "occurred_at": "", "payload": {},
            "stage_instance_id": null, "prev_hash": null, "hash": "", "signature": "",
        }))
        .unwrap();
        e.payload = json!({"version": 1, "text": "Nur Erwachsene", "exclusion_reasons": ["Falsche Population"]});
        let before = e.payload.clone();
        let current = upcast(&e);
        // der alte Text wird ein Kriterium ohne Kürzel, alte Gründe behalten ihren Wortlaut als Kürzel
        assert_eq!(
            current["items"][0],
            json!({"code": "", "element": "", "kind": "include", "text": "Nur Erwachsene", "note": ""})
        );
        assert_eq!(
            (current["items"][1]["code"].as_str(), current["items"][1]["kind"].as_str()),
            (Some("Falsche Population"), Some("exclude"))
        );
        assert_eq!((&current["guidance"], &current["include_keywords"]), (&json!(""), &json!([])));
        assert_eq!(e.payload, before);
        // Version 2 bleibt, wie sie ist
        e.schema_version = 2;
        e.payload = json!({"version": 2, "items": [{"code": "E1", "kind": "exclude", "text": "Kinder"}]});
        assert_eq!(upcast(&e)["items"], e.payload["items"]);
    }

    #[test]
    fn criteria_items_need_unique_codes_and_text() {
        let check =
            |items: Value| validate("CriteriaVersionPublished", 2, None, &json!({"version": 1, "items": items}));
        assert!(
            check(json!([{"code": "I1", "element": "population", "kind": "include", "text": "Erwachsene"}])).is_ok()
        );
        assert!(check(json!([])).is_ok());
        let e1 = json!({"code": "E1", "kind": "exclude", "text": "Kinder"});
        assert!(check(json!([e1, e1])).unwrap_err().contains("mehrfach"));
        assert!(check(json!([{"code": "E 1", "kind": "exclude", "text": "x"}])).unwrap_err().contains("Kürzel"));
        assert!(check(json!([{"code": "", "kind": "exclude", "text": "x"}])).unwrap_err().contains("Kürzel"));
        assert!(check(json!([{"code": "E1", "kind": "vielleicht", "text": "x"}]))
            .unwrap_err()
            .contains("unzulässigen"));
        assert!(check(json!([{"code": "E1", "kind": "exclude", "text": " "}])).unwrap_err().contains("ist leer"));
        assert!(check(json!("E1")).unwrap_err().contains("keine Liste"));
        // die alte Form ohne Liste gilt weiter
        assert!(validate("CriteriaVersionPublished", 1, None, &json!({"version": 1, "text": "frei"})).is_ok());
    }

    #[test]
    fn old_stages_get_the_rules_they_always_had() {
        let mut e: Event = serde_json::from_value(json!({
            "event_id": Uuid::new_v4(), "review_id": Uuid::new_v4(), "actor_id": Uuid::new_v4(), "stream": "main",
            "seq": 1, "type": "StageInstanceOpened", "schema_version": 1, "occurred_at": "", "payload": {},
            "stage_instance_id": Uuid::new_v4(), "prev_hash": null, "hash": "", "signature": "",
        }))
        .unwrap();
        e.payload = json!({"name": "Alt", "blind": true, "min_reviewers": 2});
        let current = upcast(&e);
        assert_eq!((current["decision_rule"].as_str(), current["round"].as_i64()), (Some("unanimous"), Some(1)));
        assert_eq!(current["conflict_rule"], "discussion");
        assert!(e.payload.get("decision_rule").is_none());
    }

    #[test]
    fn stage_rules_and_verification_are_checked() {
        let stage = Uuid::new_v4();
        let base = json!({"name": "TA", "blind": false, "min_reviewers": 1, "n_records": 3, "record_ids_sha256": "x"});
        let with = |extra: Value| {
            let mut p = base.clone();
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            validate("StageInstanceOpened", 2, Some(&stage), &p)
        };
        assert!(with(json!({})).is_ok());
        assert!(with(json!({"station": "title_abstract", "round": 2, "decision_rule": "liberal"})).is_ok());
        assert!(with(json!({"verify": {"scope": "sample", "percent": 20, "seed": 7}})).is_ok());
        assert!(with(json!({"verify": {"scope": "all"}})).is_ok());
        assert!(with(json!({"reason_required": true})).is_ok());
        assert!(with(json!({"reason_required": "ja"})).unwrap_err().contains("Wahrheitswert"));
        assert!(with(json!({"decision_rule": "mehrheit"})).unwrap_err().contains("unzulässigen Wert"));
        assert!(with(json!({"conflict_rule": "münze"})).unwrap_err().contains("unzulässigen Wert"));
        assert!(with(json!({"round": 0})).unwrap_err().contains("kleiner als 1"));
        assert!(with(json!({"verify": {"scope": "sample", "percent": 0, "seed": 7}})).unwrap_err().contains("Anteil"));
        assert!(with(json!({"verify": {"scope": "sample", "percent": 20}})).unwrap_err().contains("seed"));
    }

    #[test]
    fn stage_chain_events_are_bound_to_an_open_stage() {
        for s in CATALOG.iter().filter(|s| s.chain == Chain::Stage) {
            assert_eq!(s.stage, StageRule::Open, "{}", s.name);
            assert_eq!(s.writer, Writer::Write, "{}", s.name);
        }
        assert!(is_blindable("VoteCast"));
        assert!(!is_blindable("ConsensusRecorded"));
        assert!(!is_blindable("Erfunden"));
    }

    #[test]
    fn validate_checks_type_version_stage_and_payload() {
        let stage = Uuid::new_v4();
        let record = Uuid::new_v4().to_string();
        let good = json!({"record_id": record, "decision": "include", "reasons": ["E1"], "note": ""});
        assert!(validate("VoteCast", 1, Some(&stage), &good).is_ok());
        // zusätzliche Felder stören nicht
        assert!(
            validate("VoteCast", 1, Some(&stage), &json!({"record_id": record, "decision": "maybe", "x": 1})).is_ok()
        );

        let cases = [
            ("Erfunden", 1, Some(&stage), good.clone(), "Unbekannter Ereignistyp"),
            ("VoteCast", 2, Some(&stage), good.clone(), "neueren Version"),
            ("VoteCast", 0, Some(&stage), good.clone(), "Schema-Version"),
            ("VoteCast", 1, None, good.clone(), "braucht eine Stufe"),
            ("VoteCast", 1, Some(&stage), json!([]), "kein Objekt"),
            ("VoteCast", 1, Some(&stage), json!({"record_id": "r1", "decision": "include"}), "keine gültige ID"),
            ("VoteCast", 1, Some(&stage), json!({"record_id": record, "decision": "ja"}), "unzulässigen Wert"),
            ("VoteCast", 1, Some(&stage), json!({"record_id": record, "decision": "include", "reasons": [1]}), "Liste"),
            (
                "VoteCast",
                1,
                Some(&stage),
                json!({"record_id": record, "decision": "exclude", "primary_reason": ""}),
                "leer",
            ),
            ("ConsensusRecorded", 1, Some(&stage), json!({"record_id": record, "decision": "maybe"}), "unzulässigen"),
            ("MemberRoleSet", 1, None, json!({"actor_id": record, "role": "chef"}), "unzulässigen Wert"),
            ("StageInstanceOpened", 1, Some(&stage), json!({"name": " ", "blind": true}), "ist leer"),
            ("StageInstanceOpened", 3, Some(&stage), json!({}), "neueren Version"),
            ("GateOverridden", 1, None, json!({"station": "pilot", "round": 1, "reason": " "}), "ist leer"),
            ("GateEvaluated", 1, None, json!({"station": "pilot", "round": 1}), "braucht eine Stufe"),
            ("GateEvaluated", 1, Some(&stage), json!({"station": "pilot", "round": 1}), "nicht lesbar"),
            ("WorkflowVersionPublished", 1, None, json!({"version": 1, "template": {"key": "x"}}), "nicht lesbar"),
        ];
        for (event_type, version, stage, payload, expected) in cases {
            let err = validate(event_type, version, stage, &payload).unwrap_err();
            assert!(err.contains(expected), "{event_type}: {err}");
        }
    }
}
