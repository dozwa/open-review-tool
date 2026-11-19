//! Register aller Ereignistypen: Kette, Schreibrecht, Bezug zur Stufe und Prüfung des Payloads.
//!
//! Jeder Typ steht genau hier. [`crate::store`] liest daraus die Kette, [`crate::exchange`] die
//! Verblindung und die Rechte beim Einlesen, [`crate::service`] prüft damit jeden Payload vor dem
//! Anhängen. Ein neuer Ereignistyp braucht einen Eintrag in [`CATALOG`] und sonst keine Liste.
//!
//! Die Payload-Prüfung verlangt nur, was die Projektionen brauchen. Zusätzliche Felder sind erlaubt,
//! damit ein Typ wachsen kann, ohne dass ältere Ereignisse ungültig werden.

use serde_json::Value;
use uuid::Uuid;

use crate::event::Event;
use crate::service::ROLES;

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

pub static CATALOG: [Spec; 12] = [
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
        version: 1,
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
        version: 1,
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
    if e.event_type == "CriteriaVersionPublished" {
        upcast_criteria(e.schema_version, &mut payload);
    }
    payload
}

/// Version 1 erlaubt fehlende Felder. Sie werden hier aufgefüllt, damit Leser nur eine Form kennen.
/// Spätere Versionen (strukturierte Kriterien) setzen ihre Umformung hier davor.
fn upcast_criteria(_from: i64, payload: &mut Value) {
    let Some(map) = payload.as_object_mut() else { return };
    for key in ["text", "change_note"] {
        map.entry(key).or_insert_with(|| Value::String(String::new()));
    }
    for key in ["include_keywords", "exclude_keywords", "exclusion_reasons"] {
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
    filled(p, "public_key")
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
    filled(p, "record_ids_sha256")
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
        e.payload = json!({"version": 1, "text": "Nur Erwachsene"});
        let current = upcast(&e);
        assert_eq!(current["text"], "Nur Erwachsene");
        assert_eq!(current["exclusion_reasons"], json!([]));
        assert_eq!(e.payload, json!({"version": 1, "text": "Nur Erwachsene"}));
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
            ("ConsensusRecorded", 1, Some(&stage), json!({"record_id": record, "decision": "maybe"}), "unzulässigen"),
            ("MemberRoleSet", 1, None, json!({"actor_id": record, "role": "chef"}), "unzulässigen Wert"),
            ("StageInstanceOpened", 1, Some(&stage), json!({"name": " ", "blind": true}), "ist leer"),
        ];
        for (event_type, version, stage, payload, expected) in cases {
            let err = validate(event_type, version, stage, &payload).unwrap_err();
            assert!(err.contains(expected), "{event_type}: {err}");
        }
    }
}
