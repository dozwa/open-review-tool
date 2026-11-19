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
use crate::forms::{self, Field, PURPOSES};
use crate::service::ROLES;
use crate::tr;
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
    /// Name für Menschen, deutsch; übersetzt über den Sprachkatalog ([`label`]).
    pub label: &'static str,
    /// Höchste Schema-Version, die diese App schreibt und versteht.
    pub version: i64,
    pub chain: Chain,
    pub writer: Writer,
    pub stage: StageRule,
    check: fn(&Value) -> Check,
}

pub static CATALOG: [Spec; 32] = [
    Spec {
        name: "ReviewCreated",
        label: "Review angelegt",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Creator,
        stage: StageRule::None,
        check: review_created,
    },
    Spec {
        name: "MemberRoleSet",
        label: "Rolle festgelegt",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: member_role_set,
    },
    Spec {
        name: "CriteriaVersionPublished",
        label: "Kriterien veröffentlicht",
        version: 2,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: criteria_version_published,
    },
    Spec {
        name: "RecordsImported",
        label: "Einträge importiert",
        version: 2,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: records_imported,
    },
    Spec {
        name: "StageInstanceOpened",
        label: "Stufe geöffnet",
        version: 2,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::Required,
        check: stage_instance_opened,
    },
    Spec {
        name: "StageUnblinded",
        label: "Stufe entblindet",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::Required,
        check: any_object,
    },
    Spec {
        name: "StageInstanceClosed",
        label: "Stufe geschlossen",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::Required,
        check: any_object,
    },
    Spec {
        name: "SnapshotTaken",
        label: "Snapshot",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::Required,
        check: snapshot_taken,
    },
    Spec {
        name: "VoteCast",
        label: "Stimme",
        version: 1,
        chain: Chain::Stage,
        writer: Writer::Write,
        stage: StageRule::Open,
        check: vote,
    },
    Spec {
        name: "VoteRevised",
        label: "Geänderte Stimme",
        version: 1,
        chain: Chain::Stage,
        writer: Writer::Write,
        stage: StageRule::Open,
        check: vote,
    },
    Spec {
        name: "RecordFlaggedUndecidable",
        label: "Markierung „Zur Klärung“",
        version: 1,
        chain: Chain::Stage,
        writer: Writer::Write,
        stage: StageRule::Open,
        check: record_flagged,
    },
    Spec {
        name: "AiSuggestionReceived",
        label: "KI-Vorschlag",
        version: 1,
        chain: Chain::Stage,
        writer: Writer::Write,
        stage: StageRule::Open,
        check: ai_suggestion_received,
    },
    Spec {
        name: "AiSuggestionResolved",
        label: "KI-Vorschlag übernommen oder abgelehnt",
        version: 1,
        chain: Chain::Stage,
        writer: Writer::Write,
        stage: StageRule::Open,
        check: ai_suggestion_resolved,
    },
    Spec {
        name: "ConsensusRecorded",
        label: "Konsens",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Write,
        stage: StageRule::OpenUnblinded,
        check: consensus_recorded,
    },
    Spec {
        name: "WorkflowVersionPublished",
        label: "Ablauf veröffentlicht",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: workflow_version_published,
    },
    Spec {
        name: "GateEvaluated",
        label: "Gate ausgewertet",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::Required,
        check: gate_evaluated,
    },
    Spec {
        name: "GateOverridden",
        label: "Gate übersteuert",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: gate_overridden,
    },
    Spec {
        name: "SearchVersionPublished",
        label: "Suchversion",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: search_version_published,
    },
    Spec {
        name: "SearchPeerReviewed",
        label: "Peer-Review der Suche",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: search_peer_reviewed,
    },
    Spec {
        name: "DuplicatesResolved",
        label: "Dubletten entschieden",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: duplicates_resolved,
    },
    Spec {
        name: "FullTextStatusSet",
        label: "Stand des Volltexts",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Write,
        stage: StageRule::None,
        check: fulltext_status_set,
    },
    Spec {
        name: "ZoteroLibraryLinked",
        label: "Zotero-Bibliothek verknüpft",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: zotero_library_linked,
    },
    Spec {
        name: "ZoteroItemMapped",
        label: "Zotero-Eintrag zugeordnet",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: zotero_item_mapped,
    },
    Spec {
        name: "RecordsLinkedToStudy",
        label: "Bündel",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: records_linked_to_study,
    },
    Spec {
        name: "SourceTypeSet",
        label: "Quellenart",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: source_type_set,
    },
    Spec {
        name: "ProtocolRegistered",
        label: "Registrierung",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: protocol_registered,
    },
    Spec {
        name: "ProtocolDrafted",
        label: "Text für die Registrierung",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: protocol_drafted,
    },
    Spec {
        name: "FormVersionPublished",
        label: "Formularversion",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: form_version_published,
    },
    Spec {
        name: "FormConfirmed",
        label: "Formular bestätigt",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Manage,
        stage: StageRule::None,
        check: form_confirmed,
    },
    Spec {
        name: "FormValueRecorded",
        label: "Formularwert",
        version: 1,
        chain: Chain::Stage,
        writer: Writer::Write,
        stage: StageRule::Open,
        check: form_value_recorded,
    },
    Spec {
        name: "FormEntryCompleted",
        label: "Formular fertig gemeldet",
        version: 1,
        chain: Chain::Stage,
        writer: Writer::Write,
        stage: StageRule::Open,
        check: form_entry_completed,
    },
    Spec {
        name: "FormConsensusRecorded",
        label: "Abgleich eines Formularwerts",
        version: 1,
        chain: Chain::Main,
        writer: Writer::Write,
        stage: StageRule::OpenUnblinded,
        check: form_value_recorded,
    },
];

/// Woher die Treffer eines Imports stammen. Der Bericht zählt Datenbanken und Register getrennt von
/// anderen Wegen.
pub const ORIGINS: [&str; 4] = ["database", "register", "citation", "other"];
/// Höchstzahl der Paare in einem `DuplicatesResolved`.
pub const MAX_PAIRS: usize = 500;
/// Zustand eines Volltexts: beschafft, nicht beschaffbar oder (wieder) offen.
pub const FULLTEXT_STATUS: [&str; 3] = ["retrieved", "unavailable", "open"];
/// Woher ein beschaffter Volltext kommt.
pub const FULLTEXT_ORIGINS: [&str; 2] = ["folder", "zotero"];
/// Höchstzahl der Zuordnungen in einem `ZoteroItemMapped`.
pub const MAX_MAPPINGS: usize = 500;

pub fn spec(event_type: &str) -> Option<&'static Spec> {
    CATALOG.iter().find(|s| s.name == event_type)
}

/// Name eines Ereignistyps für Menschen, in der geltenden Sprache (Katalog „Ereignis: …“).
/// Unbekannte Typen erscheinen unter ihrem technischen Namen.
pub fn label(event_type: &str) -> String {
    spec(event_type).map_or_else(|| event_type.to_string(), |s| crate::i18n::text_in("Ereignis", s.label))
}

/// Ereignisse in der Kette einer Stufe. Ein Paket lässt sie weg, solange die Stufe verblindet ist.
pub fn is_blindable(event_type: &str) -> bool {
    spec(event_type).is_some_and(|s| s.chain == Chain::Stage)
}

/// Prüft Typ, Schema-Version, Stufenbezug und Payload. Der Text der Ablehnung ist für Menschen.
pub fn validate(event_type: &str, schema_version: i64, stage_id: Option<&Uuid>, payload: &Value) -> Check {
    let spec = spec(event_type).ok_or_else(|| tr!("Unbekannter Ereignistyp {}.", event_type))?;
    if schema_version > spec.version {
        return Err(tr!("{} stammt aus einer neueren Version der App.", event_type));
    }
    if schema_version < 1 {
        return Err(tr!("{} hat eine ungültige Schema-Version.", event_type));
    }
    if spec.stage != StageRule::None && stage_id.is_none() {
        return Err(tr!("{} braucht eine Stufe.", event_type));
    }
    if !payload.is_object() {
        return Err(tr!("{}: Der Inhalt ist kein Objekt.", event_type));
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
        "RecordsImported" => upcast_import(&mut payload),
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

/// Importe aus Version 1 nennen weder Herkunft noch Suchversion. Die leere Herkunft heißt „ohne Angabe“.
fn upcast_import(payload: &mut Value) {
    let Some(map) = payload.as_object_mut() else { return };
    map.entry("origin").or_insert_with(|| Value::String(String::new()));
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
    p.get(key).and_then(Value::as_str).ok_or_else(|| tr!("Feld „{}“ fehlt oder ist kein Text.", key))
}

fn filled(p: &Value, key: &str) -> Check {
    if text(p, key)?.trim().is_empty() {
        return Err(tr!("Feld „{}“ ist leer.", key));
    }
    Ok(())
}

fn uuid(p: &Value, key: &str) -> Check {
    Uuid::parse_str(text(p, key)?).map(|_| ()).map_err(|_| tr!("Feld „{}“ ist keine gültige ID.", key))
}

fn integer(p: &Value, key: &str, min: i64) -> Check {
    match p.get(key).and_then(Value::as_i64) {
        Some(n) if n >= min => Ok(()),
        Some(_) => Err(tr!("Feld „{}“ ist kleiner als {}.", key, min)),
        None => Err(tr!("Feld „{}“ fehlt oder ist keine ganze Zahl.", key)),
    }
}

fn one_of(p: &Value, key: &str, allowed: &[&str]) -> Check {
    let value = text(p, key)?;
    if !allowed.contains(&value) {
        return Err(tr!("Feld „{}“ hat den unzulässigen Wert „{}“.", key, value));
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
        _ => Err(tr!("Feld „{}“ ist keine Liste von Texten.", key)),
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
        Some(_) => return Err(tr!("Feld „items“ ist keine Liste.")),
    };
    let mut codes = std::collections::BTreeSet::new();
    for item in items {
        let code = text(item, "code")?;
        if !valid_code(code) {
            return Err(tr!("Das Kürzel „{}“ braucht ein bis acht Buchstaben oder Ziffern.", code));
        }
        if !codes.insert(code) {
            return Err(tr!("Das Kürzel „{}“ kommt mehrfach vor.", code));
        }
        one_of(item, "kind", &["include", "exclude"])?;
        filled(item, "text").map_err(|e| tr!("Kriterium {}: {}", code, e))?;
        optional(item, "element", is_text)?;
        optional(item, "note", is_text)?;
    }
    Ok(())
}

fn records_imported(p: &Value) -> Check {
    uuid(p, "batch_id")?;
    filled(p, "records_sha256")?;
    integer(p, "n_records", 0)?;
    // Version 1 kennt die Herkunft nicht; leer bleibt deshalb erlaubt.
    optional(p, "origin", |p, key| if text(p, key)?.is_empty() { Ok(()) } else { one_of(p, key, &ORIGINS) })?;
    optional(p, "search_id", uuid)?;
    match (p.get("search_id").is_some_and(|v| !v.is_null()), p.get("search_version").is_some_and(|v| !v.is_null())) {
        (true, true) => integer(p, "search_version", 1),
        (false, false) => Ok(()),
        _ => Err(tr!("Suche und Suchversion gehören zusammen.")),
    }
}

/// Ob ein Text ein Datum der Form JJJJ-MM-TT ist.
pub fn is_date(value: &str) -> bool {
    let b = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| b[range].iter().all(u8::is_ascii_digit);
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' || !digits(0..4) || !digits(5..7) || !digits(8..10) {
        return false;
    }
    let (month, day) = (value[5..7].parse::<u32>().unwrap_or(0), value[8..10].parse::<u32>().unwrap_or(0));
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn date(p: &Value, key: &str) -> Check {
    if is_date(text(p, key)?) {
        return Ok(());
    }
    Err(tr!("Feld „{}“ ist kein Datum der Form JJJJ-MM-TT.", key))
}

fn search_version_published(p: &Value) -> Check {
    uuid(p, "search_id")?;
    integer(p, "version", 1)?;
    optional(p, "previous_version", |p, key| integer(p, key, 1))?;
    filled(p, "source")?;
    one_of(p, "origin", &ORIGINS)?;
    filled(p, "query")?;
    date(p, "searched_on")?;
    for key in ["platform", "limits", "change_note"] {
        optional(p, key, is_text)?;
    }
    optional(p, "hit_limit", |p, key| integer(p, key, 1))?;
    optional(p, "hits", |p, key| integer(p, key, 0))
}

fn search_peer_reviewed(p: &Value) -> Check {
    uuid(p, "search_id")?;
    integer(p, "version", 1)?;
    date(p, "reviewed_on")?;
    filled(p, "note")?;
    optional(p, "reviewer", uuid)
}

fn duplicates_resolved(p: &Value) -> Check {
    let pairs =
        p.get("pairs").and_then(Value::as_array).ok_or_else(|| tr!("Feld „pairs“ fehlt oder ist keine Liste."))?;
    if pairs.is_empty() || pairs.len() > MAX_PAIRS {
        return Err(tr!("Ein Ereignis entscheidet 1 bis {} Paare.", MAX_PAIRS));
    }
    for pair in pairs {
        uuid(pair, "a")?;
        uuid(pair, "b")?;
        one_of(pair, "decision", &["duplicate", "distinct"])?;
        let (a, b) = (text(pair, "a")?, text(pair, "b")?);
        if a == b {
            return Err(tr!("Ein Paar braucht zwei verschiedene Einträge."));
        }
        let keep = pair.get("keep").and_then(Value::as_str);
        match (text(pair, "decision")?, keep) {
            ("duplicate", Some(k)) if k == a || k == b => {}
            ("duplicate", _) => return Err(tr!("Bei einer Dublette muss „keep“ einer der beiden Einträge sein.")),
            (_, None) => {}
            (_, Some(_)) => return Err(tr!("„keep“ gehört nur zu einer Dublette.")),
        }
    }
    Ok(())
}

/// Ob ein Text ein Pfad unterhalb des verknüpften Ordners ist: relativ, mit `/` getrennt, ohne `..`.
pub fn is_relative_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains('\0')
        && value.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Schlüssel eines Zotero-Objekts: acht Zeichen aus Großbuchstaben und Ziffern.
pub fn is_zotero_key(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn zotero_key(p: &Value, key: &str) -> Check {
    if is_zotero_key(text(p, key)?) {
        return Ok(());
    }
    Err(tr!("Feld „{}“ ist kein Zotero-Schlüssel.", key))
}

fn fulltext_status_set(p: &Value) -> Check {
    uuid(p, "record_id")?;
    one_of(p, "status", &FULLTEXT_STATUS)?;
    match text(p, "status")? {
        "retrieved" => {
            one_of(p, "origin", &FULLTEXT_ORIGINS)?;
            let sum = text(p, "sha256")?;
            if sum.len() != 64 || !sum.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return Err(tr!("Feld „sha256“ ist keine Prüfsumme."));
            }
            integer(p, "size", 0)?;
            match text(p, "origin")? {
                "folder" if is_relative_path(text(p, "path")?) => Ok(()),
                "folder" => Err(tr!("Feld „path“ ist kein Pfad innerhalb des Ordners.")),
                _ => zotero_key(p, "zotero_key"),
            }
        }
        "unavailable" => filled(p, "reason"),
        _ => Ok(()),
    }
}

fn zotero_library_linked(p: &Value) -> Check {
    one_of(p, "library_type", &["user", "group"])?;
    integer(p, "library_id", 0)?;
    optional(p, "collection_key", zotero_key)?;
    filled(p, "name")
}

fn zotero_item_mapped(p: &Value) -> Check {
    let items =
        p.get("items").and_then(Value::as_array).ok_or_else(|| tr!("Feld „items“ fehlt oder ist keine Liste."))?;
    if items.is_empty() || items.len() > MAX_MAPPINGS {
        return Err(tr!("Ein Ereignis ordnet 1 bis {} Einträge zu.", MAX_MAPPINGS));
    }
    for item in items {
        uuid(item, "record_id")?;
        // Ohne Schlüssel ist die Zuordnung aufgehoben.
        optional(item, "item_key", zotero_key)?;
    }
    Ok(())
}

/// Höchstzahl der Einträge in einem Bündel.
pub const MAX_STUDY_RECORDS: usize = 50;
/// Längste Bezeichnung eines Bündels und längster Schlüssel einer Quellenart.
pub const MAX_LABEL: usize = 200;

fn id_list<'a>(p: &'a Value, key: &str) -> std::result::Result<Vec<&'a str>, String> {
    let list = p.get(key).and_then(Value::as_array).ok_or(tr!("Feld „{}“ fehlt oder ist keine Liste.", key))?;
    let mut out = Vec::new();
    for item in list {
        let id = item.as_str().filter(|s| Uuid::parse_str(s).is_ok());
        let id = id.ok_or(tr!("Feld „{}“ enthält etwas, das keine ID ist.", key))?;
        if out.contains(&id) {
            return Err(tr!("Feld „{}“ nennt einen Eintrag doppelt.", key));
        }
        out.push(id);
    }
    Ok(out)
}

/// Ein Bündel nennt mindestens zwei Einträge und einen davon als Haupteintrag. Eine leere Liste löst
/// das Bündel auf.
fn records_linked_to_study(p: &Value) -> Check {
    uuid(p, "study_id")?;
    if text(p, "label")?.chars().count() > MAX_LABEL {
        return Err(tr!("Die Bezeichnung hat höchstens {} Zeichen.", MAX_LABEL));
    }
    let records = id_list(p, "records")?;
    let primary = p.get("primary").and_then(Value::as_str);
    match (records.len(), primary) {
        (0, None) => Ok(()),
        (0, Some(_)) => Err(tr!("Ein aufgelöstes Bündel hat keinen Haupteintrag.")),
        (1, _) => Err(tr!("Ein Bündel braucht mindestens zwei Einträge.")),
        (n, _) if n > MAX_STUDY_RECORDS => Err(tr!("Ein Bündel hat höchstens {} Einträge.", MAX_STUDY_RECORDS)),
        (_, Some(primary)) if records.contains(&primary) => Ok(()),
        _ => Err(tr!("Der Haupteintrag muss einer der Einträge des Bündels sein.")),
    }
}

/// Quellenart für einen oder mehrere Einträge; ein leerer Text nimmt sie zurück.
fn source_type_set(p: &Value) -> Check {
    let records = id_list(p, "records")?;
    if records.is_empty() || records.len() > MAX_PAIRS {
        return Err(tr!("Ein Ereignis setzt die Quellenart für 1 bis {} Einträge.", MAX_PAIRS));
    }
    if text(p, "source_type")?.chars().count() > MAX_LABEL {
        return Err(tr!("Die Quellenart hat höchstens {} Zeichen.", MAX_LABEL));
    }
    Ok(())
}

/// Registrierung des Protokolls: Tag, Register und Kennung. Die Kennung darf leer sein, etwa bei
/// einem Protokoll, das nur intern vorliegt.
fn protocol_registered(p: &Value) -> Check {
    date(p, "registered_on")?;
    filled(p, "registry")?;
    for key in ["registry", "identifier"] {
        if text(p, key)?.chars().count() > MAX_LABEL {
            return Err(tr!("Feld „{}“ hat höchstens {} Zeichen.", key, MAX_LABEL));
        }
    }
    optional(p, "note", is_text)
}

/// Eigene Texte zur Vorbereitung der Registrierung: Formular und je Feld ein Text; leer nimmt zurück.
fn protocol_drafted(p: &Value) -> Check {
    one_of(p, "form", &crate::registration::FORMS)?;
    let answers = p.get("answers").and_then(Value::as_object).ok_or_else(|| tr!("Feld „answers“ fehlt."))?;
    if answers.is_empty() || answers.len() > forms::MAX_FIELDS {
        return Err(tr!("Ein Eintrag enthält 1 bis {} Felder.", forms::MAX_FIELDS));
    }
    for (key, value) in answers {
        if !forms::valid_key(key) {
            return Err(tr!("„{}“ ist kein Schlüssel eines Feldes.", key));
        }
        match value.as_str() {
            Some(text) if text.chars().count() <= crate::registration::MAX_ANSWER => {}
            Some(_) => return Err(tr!("Feld „{}“ ist zu lang.", key)),
            None => return Err(tr!("Feld „{}“ ist kein Text.", key)),
        }
    }
    Ok(())
}

/// Eine Formularversion: Zweck, Name und Felder. Ob ein Feld seinen Typ behält, prüft der Dienst.
fn form_version_published(p: &Value) -> Check {
    uuid(p, "form_id")?;
    integer(p, "version", 1)?;
    one_of(p, "purpose", &PURPOSES)?;
    filled(p, "name")?;
    optional(p, "change_note", is_text)?;
    let fields = p.get("fields").cloned().ok_or_else(|| tr!("Feld „fields“ fehlt."))?;
    let fields: Vec<Field> =
        serde_json::from_value(fields).map_err(|_| tr!("Die Felder des Formulars sind nicht lesbar."))?;
    forms::check_fields(&fields)
}

fn form_confirmed(p: &Value) -> Check {
    uuid(p, "form_id")?;
    integer(p, "version", 1)
}

/// Wert eines Feldes, von einer Person oder als geklärter Wert. Ob er zum Feld passt, prüfen Dienst
/// und Einlesen gegen das Formular der Stufe.
fn form_value_recorded(p: &Value) -> Check {
    uuid(p, "record_id")?;
    if !forms::valid_key(text(p, "field")?) {
        return Err(tr!("Feld „field“ ist kein Schlüssel eines Formularfeldes."));
    }
    integer(p, "group_index", 0)?;
    integer(p, "form_version", 1)?;
    forms::check_shape(p.get("value").unwrap_or(&Value::Null))?;
    optional(p, "note", is_text)
}

fn form_entry_completed(p: &Value) -> Check {
    uuid(p, "record_id")?;
    integer(p, "form_version", 1)?;
    optional(p, "seconds", |p, key| integer(p, key, 0))
}

fn stage_instance_opened(p: &Value) -> Check {
    filled(p, "name")?;
    if !p.get("blind").is_some_and(Value::is_boolean) {
        return Err(tr!("Feld „blind“ fehlt oder ist kein Wahrheitswert."));
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
        false => Err(tr!("Feld „{}“ ist kein Wahrheitswert.", key)),
    })?;
    optional(p, "form_id", uuid)?;
    optional(p, "form_version", |p, key| integer(p, key, 1))?;
    optional(p, "verify", |p, key| {
        let v = &p[key];
        one_of(v, "scope", &["all", "sample"]).map_err(|e| tr!("Feld „{}“: {}", key, e))?;
        if v["scope"] == "sample" {
            let percent = v.get("percent").and_then(Value::as_i64);
            if !percent.is_some_and(|n| (1..=100).contains(&n)) {
                return Err(tr!("Feld „{}“: Der Anteil der Stichprobe muss zwischen 1 und 100 Prozent liegen.", key));
            }
            integer(v, "seed", 0).map_err(|e| tr!("Feld „{}“: {}", key, e))?;
        }
        Ok(())
    })
}

fn workflow_version_published(p: &Value) -> Check {
    integer(p, "version", 1)?;
    let template = p.get("template").ok_or_else(|| tr!("Feld „template“ fehlt."))?;
    Template::parse(template).map(|_| ())
}

fn gate_evaluated(p: &Value) -> Check {
    filled(p, "station")?;
    integer(p, "round", 1)?;
    serde_json::from_value::<GateResult>(p.clone())
        .map(|_| ())
        .map_err(|_| tr!("Das Ergebnis des Gates ist nicht lesbar."))
}

fn gate_overridden(p: &Value) -> Check {
    filled(p, "station")?;
    integer(p, "round", 0)?;
    filled(p, "reason")
}

fn snapshot_taken(p: &Value) -> Check {
    filled(p, "hash")?;
    if !p.get("heads").is_some_and(Value::is_array) {
        return Err(tr!("Feld „heads“ fehlt oder ist keine Liste."));
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

/// Längster Begründungstext eines KI-Vorschlags, in Zeichen.
pub const MAX_RATIONALE: usize = 4000;

// Die Entscheidung heißt hier `suggested` und nicht `decision`: Ein Vorschlag ist keine Stimme, und die
// Regeln für Ausschlussgründe gelten für ihn nicht.
fn ai_suggestion_received(p: &Value) -> Check {
    uuid(p, "record_id")?;
    filled(p, "model")?;
    filled(p, "prompt_sha256")?;
    one_of(p, "suggested", &["include", "exclude", "maybe"])?;
    optional(p, "reason", filled)?;
    if text(p, "rationale")?.chars().count() > MAX_RATIONALE {
        return Err(tr!("Feld „{}“ ist länger als {} Zeichen.", "rationale", MAX_RATIONALE));
    }
    Ok(())
}

fn ai_suggestion_resolved(p: &Value) -> Check {
    uuid(p, "record_id")?;
    uuid(p, "suggestion")?;
    one_of(p, "outcome", &["accepted", "declined"])
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
    fn ai_suggestions_are_checked() {
        let stage = Uuid::new_v4();
        let record = Uuid::new_v4().to_string();
        let received = |extra: Value| {
            let mut p = json!({"record_id": record, "model": "llama3", "prompt_sha256": "ab", "suggested": "exclude", "rationale": "Falsche Population."});
            p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            validate("AiSuggestionReceived", 1, Some(&stage), &p)
        };
        assert!(received(json!({})).is_ok());
        assert!(received(json!({"reason": "E1"})).is_ok());
        assert!(received(json!({"reason": null})).is_ok());
        assert!(received(json!({"suggested": "vielleicht"})).unwrap_err().contains("unzulässigen Wert"));
        assert!(received(json!({"model": " "})).unwrap_err().contains("leer"));
        assert!(received(json!({"rationale": "x".repeat(MAX_RATIONALE + 1)})).unwrap_err().contains("länger"));
        assert!(validate("AiSuggestionReceived", 1, None, &json!({})).is_err());

        let resolved = |outcome: &str| {
            let p = json!({"record_id": record, "suggestion": Uuid::new_v4().to_string(), "outcome": outcome});
            validate("AiSuggestionResolved", 1, Some(&stage), &p)
        };
        assert!(resolved("accepted").is_ok());
        assert!(resolved("declined").is_ok());
        assert!(resolved("ignoriert").unwrap_err().contains("unzulässigen Wert"));
        assert!(is_blindable("AiSuggestionReceived") && is_blindable("AiSuggestionResolved"));
    }

    #[test]
    fn stage_chain_events_are_bound_to_an_open_stage() {
        for s in CATALOG.iter().filter(|s| s.chain == Chain::Stage) {
            assert_eq!(s.stage, StageRule::Open, "{}", s.name);
            assert_eq!(s.writer, Writer::Write, "{}", s.name);
        }
        assert!(is_blindable("VoteCast"));
        assert_eq!(label("VoteCast"), "Stimme");
        assert_eq!(crate::i18n::with("en", || label("VoteCast")), "vote");
        for spec in &CATALOG {
            for language in &crate::i18n::LANGUAGES[1..] {
                let key = format!("Ereignis: {}", spec.label);
                assert!(crate::i18n::catalog(language.code).unwrap().contains_key(&key), "{}: {key}", language.code);
            }
        }
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

    #[test]
    fn bundle_and_source_type_payloads_are_checked() {
        let (study, a, b) = (Uuid::new_v4().to_string(), Uuid::new_v4().to_string(), Uuid::new_v4().to_string());
        let link = |records: Value, primary: Value| {
            let payload = json!({"study_id": study, "label": "Kohorte", "records": records, "primary": primary});
            validate("RecordsLinkedToStudy", 1, None, &payload)
        };
        assert_eq!(link(json!([a, b]), json!(a)), Ok(()));
        assert_eq!(link(json!([]), Value::Null), Ok(()), "eine leere Liste löst das Bündel auf");
        for (records, primary) in [
            (json!([a]), json!(a)),
            (json!([a, a]), json!(a)),
            (json!([a, b]), json!(study)),
            (json!([a, b]), Value::Null),
            (json!([]), json!(a)),
            (json!([a, "keine-id"]), json!(a)),
        ] {
            assert!(link(records.clone(), primary).is_err(), "{records}");
        }
        let many: Vec<String> = (0..=MAX_STUDY_RECORDS).map(|_| Uuid::new_v4().to_string()).collect();
        assert!(link(json!(many), json!(many[0])).is_err());

        let kind = |records: Value, source_type: &str| {
            validate("SourceTypeSet", 1, None, &json!({"records": records, "source_type": source_type}))
        };
        assert_eq!(kind(json!([a, b]), "review"), Ok(()));
        assert_eq!(kind(json!([a]), ""), Ok(()), "ein leerer Text nimmt die Quellenart zurück");
        assert!(kind(json!([]), "review").is_err());
        assert!(kind(json!([a]), &"x".repeat(MAX_LABEL + 1)).is_err());
        assert!(validate("SourceTypeSet", 1, None, &json!({"records": [a]})).is_err());
        // Registrierung des Protokolls: Datum und Register sind Pflicht, die Kennung darf leer sein.
        let protocol = json!({"registered_on": "2026-01-10", "registry": "OSF", "identifier": ""});
        assert_eq!(validate("ProtocolRegistered", 1, None, &protocol), Ok(()));
        for (key, bad) in [("registered_on", json!("10.01.2026")), ("registry", json!(" ")), ("identifier", json!(7))] {
            let mut changed = protocol.clone();
            changed[key] = bad;
            assert!(validate("ProtocolRegistered", 1, None, &changed).is_err(), "{key}");
        }
        // Vorbereitung der Registrierung: bekanntes Formular, je Feld ein Text; leer nimmt zurück.
        let draft =
            json!({"form": "osf_gsrrf", "answers": {"background": "Warum es das Review braucht", "funding": ""}});
        assert_eq!(validate("ProtocolDrafted", 1, None, &draft), Ok(()));
        for bad in [
            json!({"form": "prospero", "answers": {"background": "x"}}),
            json!({"form": "osf_gsrrf", "answers": {}}),
            json!({"form": "osf_gsrrf", "answers": {"background": 7}}),
            json!({"form": "osf_gsrrf", "answers": {"Mein Feld": "x"}}),
            json!({"form": "osf_gsrrf", "answers": {"background": "x".repeat(20_001)}}),
        ] {
            assert!(validate("ProtocolDrafted", 1, None, &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn search_and_duplicate_payloads_are_checked() {
        let id = Uuid::new_v4().to_string();
        let search = json!({
            "search_id": id, "version": 1, "source": "MEDLINE", "platform": "Ovid", "origin": "database",
            "query": "headache AND adult", "searched_on": "2026-11-19", "limits": "", "hits": 120,
        });
        assert_eq!(validate("SearchVersionPublished", 1, None, &search), Ok(()));
        for (key, value) in [("searched_on", json!("19.11.2026")), ("origin", json!("web")), ("query", json!(" "))] {
            let mut bad = search.clone();
            bad[key] = value;
            assert!(validate("SearchVersionPublished", 1, None, &bad).is_err(), "{key}");
        }
        let review = json!({"search_id": id, "version": 1, "reviewed_on": "2026-11-20", "note": "PRESS, ohne Befund"});
        assert_eq!(validate("SearchPeerReviewed", 1, None, &review), Ok(()));

        let (a, b) = (Uuid::new_v4().to_string(), Uuid::new_v4().to_string());
        let pairs = |pair: Value| json!({"pairs": [pair]});
        let ok = [
            json!({"a": a, "b": b, "decision": "duplicate", "keep": a}),
            json!({"a": a, "b": b, "decision": "distinct"}),
        ];
        for pair in ok {
            assert_eq!(validate("DuplicatesResolved", 1, None, &pairs(pair)), Ok(()));
        }
        let bad = [
            json!({"a": a, "b": a, "decision": "distinct"}),
            json!({"a": a, "b": b, "decision": "duplicate"}),
            json!({"a": a, "b": b, "decision": "duplicate", "keep": Uuid::new_v4().to_string()}),
            json!({"a": a, "b": b, "decision": "distinct", "keep": a}),
        ];
        for pair in bad {
            assert!(validate("DuplicatesResolved", 1, None, &pairs(pair)).is_err());
        }
        assert!(validate("DuplicatesResolved", 1, None, &json!({"pairs": []})).is_err());
    }

    #[test]
    fn fulltext_and_zotero_payloads_are_checked() {
        let id = Uuid::new_v4().to_string();
        let sum = "a".repeat(64);
        let ok = |t: &str, p: Value| validate(t, 1, None, &p);
        let folder = json!({"record_id": id, "status": "retrieved", "origin": "folder", "path": "a/b.pdf", "sha256": sum, "size": 10});
        assert_eq!(ok("FullTextStatusSet", folder.clone()), Ok(()));
        for path in ["/etc/passwd", "../b.pdf", "a//b.pdf", "a/../b.pdf", ""] {
            let mut bad = folder.clone();
            bad["path"] = json!(path);
            assert!(ok("FullTextStatusSet", bad).is_err(), "{path}");
        }
        let mut bad = folder.clone();
        bad["sha256"] = json!("abc");
        assert!(ok("FullTextStatusSet", bad).unwrap_err().contains("Prüfsumme"));
        let zotero = json!({"record_id": id, "status": "retrieved", "origin": "zotero", "zotero_key": "ABCD2345", "sha256": sum, "size": 1});
        assert_eq!(ok("FullTextStatusSet", zotero.clone()), Ok(()));
        let mut bad = zotero.clone();
        bad["zotero_key"] = json!("abc");
        assert!(ok("FullTextStatusSet", bad).is_err());
        assert_eq!(
            ok("FullTextStatusSet", json!({"record_id": id, "status": "unavailable", "reason": "Fernleihe erfolglos"})),
            Ok(())
        );
        assert!(ok("FullTextStatusSet", json!({"record_id": id, "status": "unavailable", "reason": " "})).is_err());
        assert_eq!(ok("FullTextStatusSet", json!({"record_id": id, "status": "open"})), Ok(()));
        assert!(ok("FullTextStatusSet", json!({"record_id": id, "status": "lost"})).is_err());

        let link = json!({"library_type": "group", "library_id": 12, "collection_key": "ABCD2345", "name": "Team"});
        assert_eq!(ok("ZoteroLibraryLinked", link), Ok(()));
        assert!(ok("ZoteroLibraryLinked", json!({"library_type": "web", "library_id": 0, "name": "x"})).is_err());
        assert_eq!(
            ok(
                "ZoteroItemMapped",
                json!({"items": [{"record_id": id, "item_key": "ABCD2345"}, {"record_id": id, "item_key": null}]})
            ),
            Ok(())
        );
        assert!(ok("ZoteroItemMapped", json!({"items": []})).is_err());
        assert!(ok("ZoteroItemMapped", json!({"items": [{"record_id": id, "item_key": "kurz"}]})).is_err());
    }

    #[test]
    fn imports_name_origin_and_search_together() {
        let base = json!({"batch_id": Uuid::new_v4().to_string(), "records_sha256": "x", "n_records": 3});
        assert_eq!(validate("RecordsImported", 1, None, &base), Ok(()));
        let mut with = base.clone();
        with["origin"] = json!("register");
        with["search_id"] = json!(Uuid::new_v4().to_string());
        assert!(validate("RecordsImported", 2, None, &with).unwrap_err().contains("gehören zusammen"));
        with["search_version"] = json!(1);
        assert_eq!(validate("RecordsImported", 2, None, &with), Ok(()));
        with["origin"] = json!("web");
        assert!(validate("RecordsImported", 2, None, &with).is_err());
    }
}
