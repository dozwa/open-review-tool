//! Formulare: Felder mit Versionen, Werte je Person und Quelle, Abgleich je Feld.
//!
//! Ein Formular (`FormVersionPublished`) hat einen Zweck (`charting` oder `appraisal`) und Versionen
//! mit Feldern. `FormConfirmed` hält fest, dass die Leitung eine Version nach dem Pilot bestätigt hat;
//! danach braucht jede Änderung eine Begründung. Ein Feld behält seinen Typ über alle Versionen.
//!
//! Eine Formular-Stufe weist den Haupteintrag jeder eingeschlossenen Quelle zu. Jeder Wert ist ein
//! `FormValueRecorded` in der Kette der Stufe und damit verblindbar; es gilt der neueste Wert je
//! Person, Quelle und Feld. `FormEntryCompleted` meldet eine Quelle als fertig, erst dann geht sie in
//! den Abgleich. `FormConsensusRecorded` legt nach dem Entblinden den geklärten Wert eines Feldes fest.
//!
//! Zwei Arten zu arbeiten, je nach Auswahlregel der Stufe:
//! - `unanimous` (doppelt): Ab `min_reviewers` fertigen Personen wird je Feld verglichen. Verschiedene
//!   Werte sind eine Abweichung, bis ein geklärter Wert vorliegt.
//! - `single_verified` (einfach mit Prüfung): Wer zuerst abschließt, extrahiert. Eine zweite Person
//!   prüft, korrigiert über geklärte Werte und schließt als Prüfung ab.
//!
//! Gilt eine neuere Formularversion mit weiteren Pflichtfeldern, ist eine fertige Quelle
//! „nachzutragen“, bis die Felder gefüllt sind. Alles hier ist eine Projektion über das Log.

use std::collections::{BTreeMap, BTreeSet};

use rand::seq::index::sample;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::{conflict, forbidden, invalid, not_found, Result};
use crate::event::Event;
use crate::projection::{order_key, StageState};
use crate::service::{csv_field, require, stage_view, MemberView, RecordItem, Review, StageView};
use crate::service::{MANAGE_ROLES, ROLES, WRITE_ROLES};
use crate::sources::Studies;
use crate::tr;
use crate::workflow::{CheckResult, GateResult, SchemaElement, Station, Workflow};

pub const FORM_TYPES: [&str; 2] = ["FormVersionPublished", "FormConfirmed"];
pub const FIELD_TYPES: [&str; 6] = ["text", "integer", "decimal", "choice", "multi_choice", "quote"];
pub const PURPOSES: [&str; 2] = ["charting", "appraisal"];
/// Höchstzahl der Felder eines Formulars und der Auswahlwerte eines Feldes.
pub const MAX_FIELDS: usize = 200;
/// Längster Wert eines Feldes in Zeichen.
pub const MAX_VALUE: usize = 20_000;
/// Stichprobe einer Formular-Stufe je Quellenart, wenn die Vorlage nichts nennt.
pub const PILOT_PER_TYPE: i64 = 2;

type Check = std::result::Result<(), String>;

// ---------------------------------------------------------------- Felder und Werte

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    /// Einer aus [`FIELD_TYPES`].
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    /// Überschrift, unter der das Feld im Formular steht.
    #[serde(default)]
    pub group: String,
    /// Wie das Feld ausgewertet werden soll (Analyseplan).
    #[serde(default)]
    pub analysis: String,
    /// Auswahlwerte bei `choice` und `multi_choice`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

impl Field {
    fn has_options(&self) -> bool {
        matches!(self.kind.as_str(), "choice" | "multi_choice")
    }
}

/// Startformular einer Vorlage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Starter {
    pub purpose: String,
    pub name: String,
    pub fields: Vec<Field>,
}

/// Schlüssel eines Feldes: Kleinbuchstaben, Ziffern und Unterstrich, höchstens 40 Zeichen.
pub fn valid_key(key: &str) -> bool {
    (1..=40).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub fn check_fields(fields: &[Field]) -> Check {
    if fields.is_empty() || fields.len() > MAX_FIELDS {
        return Err(tr!("Ein Formular hat 1 bis {} Felder.", MAX_FIELDS));
    }
    let mut keys = BTreeSet::new();
    for f in fields {
        if !valid_key(&f.key) {
            return Err(tr!("Der Schlüssel „{}“ braucht 1 bis 40 Kleinbuchstaben, Ziffern oder Unterstriche.", f.key));
        }
        if !keys.insert(f.key.as_str()) {
            return Err(tr!("Der Schlüssel „{}“ kommt mehrfach vor.", f.key));
        }
        if f.label.trim().is_empty() {
            return Err(tr!("Dem Feld „{}“ fehlt der Name.", f.key));
        }
        if !FIELD_TYPES.contains(&f.kind.as_str()) {
            return Err(tr!("Feld „{}“: unbekannter Typ „{}“.", f.label, f.kind));
        }
        if f.has_options() {
            let distinct: BTreeSet<&str> = f.options.iter().map(|o| o.trim()).collect();
            if f.options.is_empty() || f.options.len() > MAX_FIELDS {
                return Err(tr!("Feld „{}“: Eine Auswahl braucht 1 bis {} Werte.", f.label, MAX_FIELDS));
            }
            if distinct.len() != f.options.len() || distinct.contains("") {
                return Err(tr!("Feld „{}“: Die Auswahlwerte müssen verschieden und nicht leer sein.", f.label));
            }
        }
    }
    Ok(())
}

pub fn check_starter(starter: &Starter) -> Check {
    if !PURPOSES.contains(&starter.purpose.as_str()) {
        return Err(tr!("Formular „{}“: unbekannter Zweck „{}“.", starter.name, starter.purpose));
    }
    if starter.name.trim().is_empty() {
        return Err(tr!("Einem Formular der Vorlage fehlt der Name."));
    }
    check_fields(&starter.fields)
}

/// Form eines Wertes ohne Blick aufs Formular: leer, Text, ganze Zahl oder Liste von Texten.
pub fn check_shape(value: &Value) -> Check {
    let short = |s: &str| s.chars().count() <= MAX_VALUE;
    match value {
        Value::Null => Ok(()),
        Value::String(s) if short(s) => Ok(()),
        Value::String(_) => Err(tr!("Ein Wert hat höchstens {} Zeichen.", MAX_VALUE)),
        Value::Number(n) if n.is_i64() => Ok(()),
        Value::Array(items) if items.len() <= MAX_FIELDS && items.iter().all(|i| i.as_str().is_some_and(short)) => {
            Ok(())
        }
        _ => Err(tr!("Ein Wert ist leer, ein Text, eine ganze Zahl oder eine Liste von Texten.")),
    }
}

/// Dezimalzahl als Text: Ziffern, höchstens ein Komma oder Punkt, Vorzeichen erlaubt.
fn is_decimal(text: &str) -> bool {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let mut parts = digits.splitn(2, [',', '.']);
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    parts.next().is_some_and(all_digits) && parts.next().is_none_or(all_digits)
}

/// Ob ein Wert zum Feld passt. Leer passt immer; Pflichtfelder prüft erst der Abschluss.
pub fn check_value(field: &Field, value: &Value) -> Check {
    check_shape(value)?;
    let label = &field.label;
    match (field.kind.as_str(), value) {
        (_, Value::Null) => Ok(()),
        ("text" | "quote", Value::String(_)) => Ok(()),
        ("integer", Value::Number(_)) => Ok(()),
        ("integer", _) => Err(tr!("Feld „{}“ erwartet eine ganze Zahl.", label)),
        ("decimal", Value::String(s)) if is_decimal(s) => Ok(()),
        ("decimal", _) => Err(tr!("Feld „{}“ erwartet eine Zahl wie 12,5.", label)),
        ("choice", Value::String(s)) if field.options.contains(s) => Ok(()),
        ("choice", _) => Err(tr!("Feld „{}“: Der Wert steht nicht in der Auswahl.", label)),
        ("multi_choice", Value::Array(items)) => {
            let known = items.iter().all(|i| i.as_str().is_some_and(|s| field.options.iter().any(|o| o == s)));
            match known {
                true => Ok(()),
                false => Err(tr!("Feld „{}“: Ein Wert steht nicht in der Auswahl.", label)),
            }
        }
        _ => Err(tr!("Feld „{}“: Der Wert passt nicht zum Typ des Feldes.", label)),
    }
}

/// Einheitliche Form zum Speichern und Vergleichen: Text ohne Rand, Listen sortiert und ohne
/// Doppelte, Leeres als `null`.
pub fn normal(value: &Value) -> Value {
    match value {
        Value::String(s) if s.trim().is_empty() => Value::Null,
        Value::String(s) => Value::String(s.trim().to_string()),
        Value::Array(items) => {
            let set: BTreeSet<&str> = items.iter().filter_map(Value::as_str).filter(|s| !s.is_empty()).collect();
            match set.is_empty() {
                true => Value::Null,
                false => json!(set),
            }
        }
        other => other.clone(),
    }
}

/// Wert als Text für Tabelle und Export.
pub fn show(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(items) => items.iter().map(show).collect::<Vec<_>>().join("; "),
        other => other.to_string(),
    }
}

/// Platz eines Wertes: das Feld, bei wiederholten Gruppen mit Index.
fn slot(field: &str, group_index: i64) -> String {
    match group_index {
        0 => field.to_string(),
        n => format!("{field}#{n}"),
    }
}

// ---------------------------------------------------------------- Formulare aus dem Log

#[derive(Clone, Debug, Serialize)]
pub struct FormVersion {
    pub version: i64,
    pub name: String,
    pub fields: Vec<Field>,
    pub change_note: String,
    pub published_at: String,
    pub published_by: Uuid,
    /// Die Leitung hat diese Version bestätigt.
    pub confirmed: bool,
    /// Veröffentlicht, nachdem das Formular schon bestätigt war: eine Änderung nach dem Pilot.
    pub after_confirmation: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Form {
    pub id: Uuid,
    pub purpose: String,
    /// Aufsteigend; nie leer.
    pub versions: Vec<FormVersion>,
    pub confirmed: bool,
}

impl Form {
    pub fn current(&self) -> &FormVersion {
        self.versions.last().expect("ein Formular hat mindestens eine Version")
    }

    /// Feld mit diesem Schlüssel aus der neuesten Version, die es kennt.
    pub fn field(&self, key: &str) -> Option<&Field> {
        self.versions.iter().rev().find_map(|v| v.fields.iter().find(|f| f.key == key))
    }
}

/// Alle Formulare eines Reviews in der Reihenfolge ihrer ersten Veröffentlichung.
#[derive(Clone, Debug, Default)]
pub struct Forms {
    pub list: Vec<Form>,
}

impl Forms {
    pub fn build(events: &[Event]) -> Forms {
        let mut sorted: Vec<&Event> = events.iter().collect();
        sorted.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
        let mut forms = Forms::default();
        for e in sorted {
            let Some(id) = e.payload_str("form_id").and_then(|s| Uuid::parse_str(s).ok()) else { continue };
            let version = e.payload.get("version").and_then(Value::as_i64).unwrap_or(0);
            match e.event_type.as_str() {
                "FormVersionPublished" => {
                    let fields: Option<Vec<Field>> =
                        e.payload.get("fields").and_then(|f| serde_json::from_value(f.clone()).ok());
                    let Some(fields) = fields else { continue };
                    let index = match forms.list.iter().position(|f| f.id == id) {
                        Some(index) => index,
                        None => {
                            let purpose = e.payload_str("purpose").unwrap_or("charting").to_string();
                            forms.list.push(Form { id, purpose, versions: Vec::new(), confirmed: false });
                            forms.list.len() - 1
                        }
                    };
                    let form = &mut forms.list[index];
                    // Bei gleicher Nummer gilt die später veröffentlichte Version.
                    form.versions.retain(|v| v.version != version);
                    form.versions.push(FormVersion {
                        version,
                        name: e.payload_str("name").unwrap_or_default().to_string(),
                        fields,
                        change_note: e.payload_str("change_note").unwrap_or_default().to_string(),
                        published_at: e.occurred_at.clone(),
                        published_by: e.actor_id,
                        confirmed: false,
                        after_confirmation: form.confirmed,
                    });
                    form.versions.sort_by_key(|v| v.version);
                }
                "FormConfirmed" => {
                    let Some(form) = forms.list.iter_mut().find(|f| f.id == id) else { continue };
                    if let Some(v) = form.versions.iter_mut().find(|v| v.version == version) {
                        v.confirmed = true;
                        form.confirmed = true;
                    }
                }
                _ => {}
            }
        }
        forms
    }

    pub fn get(&self, id: &Uuid) -> Option<&Form> {
        self.list.iter().find(|f| f.id == *id)
    }

    /// Feld, wie es die genannte Version eines Formulars kennt.
    pub fn field_in(&self, form: &Uuid, version: i64, key: &str) -> Option<&Field> {
        let version = self.get(form)?.versions.iter().find(|v| v.version == version)?;
        version.fields.iter().find(|f| f.key == key)
    }

    /// Zwecke, für die ein bestätigtes Formular vorliegt.
    pub fn confirmed_purposes(&self) -> BTreeSet<String> {
        self.list.iter().filter(|f| f.confirmed).map(|f| f.purpose.clone()).collect()
    }
}

// ---------------------------------------------------------------- Werte einer Stufe

#[derive(Clone, Debug)]
pub struct Completion {
    pub form_version: i64,
    /// Zeit am Eintrag in Sekunden, von der Oberfläche gemessen.
    pub seconds: Option<i64>,
    /// Abschluss einer Prüfung statt einer Extraktion.
    pub verify: bool,
    at: String,
}

/// Werte, Abschlüsse und geklärte Werte einer Formular-Stufe.
#[derive(Clone, Debug, Default)]
pub struct FormState {
    /// (Eintrag, Person) → Platz → Wert; Leeres fehlt.
    pub values: BTreeMap<(String, Uuid), BTreeMap<String, Value>>,
    pub completed: BTreeMap<(String, Uuid), Completion>,
    /// (Eintrag, Platz) → geklärter Wert; `null` heißt „geklärt: leer“.
    pub consensus: BTreeMap<(String, String), Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Given {
    pub actor_id: Uuid,
    pub name: String,
    pub value: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct Cell {
    pub field: String,
    /// `agreed`, `consensus`, `differs`, `empty` oder `mine` (nur eigene Werte, Stufe verblindet).
    pub status: &'static str,
    /// Gültiger Wert; `null` bei einer Abweichung und bei leeren Feldern.
    pub value: Value,
    /// Werte der Personen, die abgeschlossen haben.
    pub values: Vec<Given>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Compared {
    /// `open`, `extracted` (wartet auf Prüfung), `conflict`, `incomplete` (Pflichtfeld leer) oder `done`.
    pub status: &'static str,
    pub cells: Vec<Cell>,
    pub n_completed: usize,
    /// Wer extrahiert hat; bei `single_verified` höchstens eine Person.
    pub extractors: Vec<Uuid>,
}

impl FormState {
    /// Baut den Zustand aus den Ereignissen einer Stufe.
    pub fn build(events: &[Event]) -> FormState {
        let mut sorted: Vec<&Event> = events.iter().collect();
        sorted.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
        let mut state = FormState::default();
        for e in sorted {
            let Some(record) = e.payload_str("record_id") else { continue };
            let place = || {
                let index = e.payload.get("group_index").and_then(Value::as_i64).unwrap_or(0);
                e.payload_str("field").map(|f| slot(f, index))
            };
            let value = || normal(e.payload.get("value").unwrap_or(&Value::Null));
            let key = (record.to_string(), e.actor_id);
            match e.event_type.as_str() {
                "FormValueRecorded" => {
                    let Some(place) = place() else { continue };
                    let own = state.values.entry(key).or_default();
                    match value() {
                        Value::Null => drop(own.remove(&place)),
                        value => drop(own.insert(place, value)),
                    }
                }
                "FormEntryCompleted" => {
                    let completion = Completion {
                        form_version: e.payload.get("form_version").and_then(Value::as_i64).unwrap_or(0),
                        seconds: e.payload.get("seconds").and_then(Value::as_i64),
                        verify: e.payload.get("verify").and_then(Value::as_bool).unwrap_or(false),
                        at: e.occurred_at.clone(),
                    };
                    state.completed.insert(key, completion);
                }
                "FormConsensusRecorded" => {
                    if let Some(place) = place() {
                        state.consensus.insert((record.to_string(), place), value());
                    }
                }
                _ => {}
            }
        }
        state
    }

    fn value_of(&self, record: &str, actor: &Uuid, field: &str) -> Value {
        self.values.get(&(record.to_string(), *actor)).and_then(|m| m.get(field)).cloned().unwrap_or(Value::Null)
    }

    /// Wer bei `single_verified` extrahiert hat: die Person mit dem ersten Abschluss, der keine
    /// Prüfung ist.
    fn extractor(&self, record: &str) -> Option<Uuid> {
        self.completed
            .iter()
            .filter(|((r, _), c)| r == record && !c.verify)
            .min_by(|a, b| (&a.1.at, a.0 .1).cmp(&(&b.1.at, b.0 .1)))
            .map(|((_, actor), _)| *actor)
    }

    /// Abgleich einer Quelle über die Felder der geltenden Formularversion.
    pub fn compare(&self, record: &str, fields: &[Field], rule: &str, min_reviewers: i64) -> Compared {
        let single = rule == "single_verified";
        let done: Vec<(&Uuid, &Completion)> =
            self.completed.iter().filter(|((r, _), _)| r == record).map(|((_, a), c)| (a, c)).collect();
        let extractors: Vec<Uuid> = match single {
            true => self.extractor(record).into_iter().collect(),
            false => done.iter().filter(|(_, c)| !c.verify).map(|(a, _)| **a).collect(),
        };
        let verified = extractors.first().is_some_and(|x| done.iter().any(|(a, c)| c.verify && *a != x));
        let enough = extractors.len() >= if single { 1 } else { min_reviewers.max(1) as usize };
        let mut missing = false;
        let cells: Vec<Cell> = fields
            .iter()
            .map(|f| {
                let values: Vec<Given> = extractors
                    .iter()
                    .map(|a| Given { actor_id: *a, name: String::new(), value: self.value_of(record, a, &f.key) })
                    .collect();
                let distinct: BTreeSet<String> = values.iter().map(|g| g.value.to_string()).collect();
                let (status, value) = match self.consensus.get(&(record.to_string(), f.key.clone())) {
                    Some(value) => ("consensus", value.clone()),
                    None if distinct.len() > 1 => ("differs", Value::Null),
                    None => match values.first().map(|g| g.value.clone()) {
                        Some(Value::Null) | None => ("empty", Value::Null),
                        Some(value) => ("agreed", value),
                    },
                };
                missing |= f.required && status != "differs" && value.is_null();
                Cell { field: f.key.clone(), status, value, values }
            })
            .collect();
        let status = if !enough {
            "open"
        } else if cells.iter().any(|c| c.status == "differs") {
            "conflict"
        } else if missing {
            "incomplete"
        } else if single && !verified {
            "extracted"
        } else {
            "done"
        };
        Compared { status, cells, n_completed: done.len(), extractors }
    }

    /// Pflichtfelder der geltenden Version, die einer Person bei einer Quelle fehlen.
    fn missing<'a>(&self, record: &str, actor: &Uuid, fields: &'a [Field]) -> Vec<&'a Field> {
        fields.iter().filter(|f| f.required && self.value_of(record, actor, &f.key).is_null()).collect()
    }
}

// ---------------------------------------------------------------- Ansichten

#[derive(Clone, Debug, Serialize)]
pub struct FormsView {
    pub forms: Vec<Form>,
    /// Startformulare der Vorlage für Zwecke, zu denen es noch kein Formular gibt.
    pub starters: Vec<Starter>,
    /// Begriff der Vorlage für das Extrahieren, z. B. „Charting“.
    pub term: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FormRef {
    pub id: Uuid,
    pub purpose: String,
    pub name: String,
    pub version: i64,
    pub fields: Vec<Field>,
    pub confirmed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FormRow {
    pub record_id: Uuid,
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    /// Bezeichnung des Bündels, sonst leer.
    pub label: String,
    pub source_type: String,
    /// Einträge der Quelle; mehr als einer bei einem Bündel.
    pub n_records: usize,
    /// Eigener Stand: `open`, `draft`, `completed`, `todo` (nachzutragen), `to_verify` oder `locked`
    /// (jemand anderes extrahiert).
    pub mine: &'static str,
    /// Stand des Abgleichs; fehlt, solange die Stufe verblindet ist.
    pub status: Option<&'static str>,
    pub n_completed: Option<usize>,
    /// Je Feld der geltenden Version; verblindet nur die eigenen Werte.
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct FormCounts {
    pub open: usize,
    pub extracted: usize,
    pub conflict: usize,
    pub incomplete: usize,
    pub done: usize,
}

/// Mittlere Zeit je Quelle, nach Quellenart.
#[derive(Clone, Debug, Serialize)]
pub struct TimeRow {
    pub source_type: String,
    pub n: i64,
    pub seconds: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct FormStageView {
    pub stage: StageView,
    pub role: String,
    pub form: FormRef,
    pub rows: Vec<FormRow>,
    /// Quellen, bei denen von dieser Person nichts mehr gebraucht wird.
    pub mine_done: usize,
    /// Fertige Quellen dieser Person, denen Pflichtfelder der geltenden Version fehlen.
    pub todo: usize,
    /// Fehlt, solange die Stufe verblindet ist.
    pub counts: Option<FormCounts>,
    pub times: Vec<TimeRow>,
    pub members: Vec<MemberView>,
    pub source_types: Vec<SchemaElement>,
    pub terms: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EntryCompletion {
    pub form_version: i64,
    pub seconds: Option<i64>,
    pub verify: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FormEntryView {
    pub stage: StageView,
    pub role: String,
    pub form: FormRef,
    pub record: RecordItem,
    /// Weitere Einträge der Quelle.
    pub companions: Vec<RecordItem>,
    /// Einträge der Quelle mit beschafftem Volltext.
    pub fulltexts: Vec<Uuid>,
    pub label: String,
    pub source_type: String,
    /// `extract`, `verify` oder `read`.
    pub mode: &'static str,
    /// Eigene Werte je Feld.
    pub values: BTreeMap<String, Value>,
    pub completed: Option<EntryCompletion>,
    /// Namen der Pflichtfelder, die noch leer sind.
    pub missing: Vec<String>,
    /// Abgleich; fehlt, solange die Stufe verblindet ist.
    pub compared: Option<Compared>,
    pub position: usize,
    pub total: usize,
    pub previous: Option<Uuid>,
    pub next: Option<Uuid>,
    /// Nächste Quelle, an der diese Person noch etwas zu tun hat.
    pub next_open: Option<Uuid>,
    pub source_types: Vec<SchemaElement>,
    pub terms: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OptionCount {
    pub option: String,
    pub n: i64,
}

/// Häufigkeiten eines Auswahlfeldes über die gültigen Werte einer Stufe.
#[derive(Clone, Debug, Serialize)]
pub struct Frequency {
    pub field: String,
    pub label: String,
    pub counts: Vec<OptionCount>,
    pub n_empty: i64,
    /// Quellen mit offener Abweichung in diesem Feld.
    pub n_open: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct FormFile {
    pub filename: String,
    pub content: String,
}

/// Eine Quelle einer Formular-Stufe im Bericht: Stand und gültige Werte je Feld. Solange die Stufe
/// verblindet ist, lautet der Stand `blind` und die Werte fehlen.
pub(crate) struct SummaryRow {
    pub record_id: Uuid,
    pub status: &'static str,
    pub values: Vec<String>,
}

/// Stand einer Formular-Stufe für Methodentext und Tabelle des Berichts.
pub(crate) struct StageSummary {
    pub form_name: String,
    pub form_version: i64,
    pub confirmed: bool,
    /// Felder der geltenden Version.
    pub fields: Vec<Field>,
    pub n_sources: i64,
    /// Fehlt wie die Zahl der Personen, solange die Stufe verblindet ist.
    pub counts: Option<FormCounts>,
    pub n_people: Option<i64>,
    /// Geklärte Felder.
    pub n_resolved: i64,
    pub rows: Vec<SummaryRow>,
}

/// Wert einer Zelle für Tabellen; eine offene Abweichung zeigt alle abgegebenen Werte.
fn cell_text(cell: &Cell) -> String {
    match cell.status {
        "differs" => {
            let given: Vec<String> = cell.values.iter().map(|g| show(&g.value)).collect();
            tr!("[Abweichung] {}", given.join(" | "))
        }
        _ => show(&cell.value),
    }
}

/// Was eine Formular-Stufe für ihre Ansichten braucht.
struct Parts {
    role: String,
    stage: StageState,
    form: Form,
    values: FormState,
}

impl Parts {
    fn fields(&self) -> &[Field] {
        &self.form.current().fields
    }

    fn compare(&self, record: &str) -> Compared {
        let c = &self.stage.config;
        self.values.compare(record, self.fields(), &c.decision_rule, c.min_reviewers)
    }

    fn single(&self) -> bool {
        self.stage.config.decision_rule == "single_verified"
    }

    /// Ob bei dieser Quelle jemand anderes extrahiert und diese Person nur prüft.
    fn verifies(&self, record: &str, me: &Uuid) -> bool {
        self.single() && self.values.extractor(record).is_some_and(|x| x != *me)
    }

    fn mine(&self, record: &str, me: &Uuid) -> &'static str {
        let key = (record.to_string(), *me);
        let completed = self.values.completed.get(&key);
        if self.verifies(record, me) {
            return match completed.is_some_and(|c| c.verify) {
                true => "completed",
                false => "to_verify",
            };
        }
        match completed {
            Some(_) if !self.values.missing(record, me, self.fields()).is_empty() => "todo",
            Some(_) => "completed",
            None if self.values.values.get(&key).is_some_and(|v| !v.is_empty()) => "draft",
            None => "open",
        }
    }

    fn form_ref(&self) -> FormRef {
        let current = self.form.current();
        FormRef {
            id: self.form.id,
            purpose: self.form.purpose.clone(),
            name: current.name.clone(),
            version: current.version,
            fields: current.fields.clone(),
            confirmed: self.form.confirmed,
        }
    }
}

/// Bezeichnung, Einträge (Haupteintrag zuerst) und Quellenart der Quelle, zu der ein Eintrag gehört.
fn unit_of(studies: &Studies, record: &Uuid) -> (String, Vec<Uuid>, String) {
    let (label, records) = match studies.study_of(record) {
        Some(study) => {
            let mut records = vec![*record];
            records.extend(study.records.iter().copied().filter(|r| r != record));
            (study.label.clone(), records)
        }
        None => (String::new(), vec![*record]),
    };
    let source_type = records.iter().find_map(|r| studies.types.get(r)).cloned().unwrap_or_default();
    (label, records, source_type)
}

impl Review {
    pub(crate) fn form_defs(&self) -> Result<Forms> {
        Ok(Forms::build(&self.store.events_of_types(&FORM_TYPES)?))
    }

    /// Formulare des Reviews mit allen Versionen.
    pub fn forms(&self) -> Result<FormsView> {
        require(&self.state()?, &self.me.actor_id, &ROLES)?;
        let forms = self.form_defs()?.list;
        let template = self.workflow()?.template;
        // Reviews von früher haben eine Vorlage ohne Startformulare im Log; dann die mitgelieferten.
        let mut starters = template.forms.clone();
        if starters.is_empty() {
            starters = crate::workflow::builtin_template(&template.key).map(|t| t.forms).unwrap_or_default();
        }
        starters.retain(|s| !forms.iter().any(|f| f.purpose == s.purpose));
        // Angeboten wird das Startformular in der geltenden Sprache; die Leitung veröffentlicht es so.
        let starters = starters.iter().map(crate::workflow::localized_starter).collect();
        Ok(FormsView { forms, starters, term: template.term("extraction").to_string() })
    }

    /// Veröffentlicht ein neues Formular oder eine neue Version. `data` nennt `purpose`, `name`,
    /// `fields`, `change_note` und für eine neue Version `form_id`.
    pub fn publish_form(&self, data: &Value) -> Result<Form> {
        require(&self.state()?, &self.me.actor_id, &MANAGE_ROLES)?;
        let text = |key: &str| data.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_string();
        let fields: Vec<Field> = serde_json::from_value(data.get("fields").cloned().unwrap_or(Value::Null))
            .map_err(|_| invalid(tr!("Die Felder des Formulars sind nicht lesbar.")))?;
        let fields: Vec<Field> = fields
            .into_iter()
            .map(|mut f| {
                f.key = f.key.trim().to_string();
                f.label = f.label.trim().to_string();
                f.options = match f.has_options() {
                    true => f.options.iter().map(|o| o.trim().to_string()).collect(),
                    false => Vec::new(),
                };
                f
            })
            .collect();
        check_fields(&fields).map_err(invalid)?;
        let name = text("name");
        if name.is_empty() {
            return Err(invalid(tr!("Dem Formular fehlt der Name.")));
        }
        let note = text("change_note");
        let forms = self.form_defs()?;
        let existing = match data.get("form_id").and_then(Value::as_str) {
            Some(id) => {
                let id = Uuid::parse_str(id).map_err(|_| invalid(tr!("Ungültige ID des Formulars.")))?;
                Some(forms.get(&id).ok_or_else(|| not_found(tr!("Dieses Formular gibt es nicht.")))?)
            }
            None => None,
        };
        let (id, purpose, version) = match existing {
            Some(form) => {
                let current = form.current();
                if current.name == name && current.fields == fields {
                    return Err(conflict(tr!("Am Formular hat sich nichts geändert.")));
                }
                for f in &fields {
                    if let Some(old) = form.field(&f.key).filter(|old| old.kind != f.kind) {
                        return Err(invalid(tr!(
                            "Das Feld „{}“ hatte bisher einen anderen Typ. Lege für den neuen Typ ein neues Feld mit eigenem Schlüssel an.",
                            old.label
                        )));
                    }
                }
                if form.confirmed && note.is_empty() {
                    return Err(invalid(tr!("Das Formular ist bestätigt. Eine Änderung braucht eine Begründung.")));
                }
                (form.id, form.purpose.clone(), current.version + 1)
            }
            None => {
                let purpose = text("purpose");
                if !PURPOSES.contains(&purpose.as_str()) {
                    return Err(invalid(tr!("Unbekannter Zweck des Formulars.")));
                }
                (Uuid::new_v4(), purpose, 1)
            }
        };
        self.append(
            "FormVersionPublished",
            None,
            json!({
                "form_id": id, "version": version, "purpose": purpose, "name": name, "fields": fields,
                "change_note": note,
            }),
        )?;
        let form = self.form_defs()?.get(&id).cloned();
        form.ok_or_else(|| not_found(tr!("Dieses Formular gibt es nicht.")))
    }

    /// Die Leitung bestätigt die geltende Version, in der Regel nach dem Pilot.
    pub fn confirm_form(&self, form_id: &Uuid) -> Result<()> {
        require(&self.state()?, &self.me.actor_id, &MANAGE_ROLES)?;
        let forms = self.form_defs()?;
        let form = forms.get(form_id).ok_or_else(|| not_found(tr!("Dieses Formular gibt es nicht.")))?;
        if form.current().confirmed {
            return Err(conflict(tr!("Diese Version ist bereits bestätigt.")));
        }
        self.append("FormConfirmed", None, json!({"form_id": form.id, "version": form.current().version}))?;
        Ok(())
    }

    /// Formular für eine Stufe der Station: das genannte, sonst das einzige mit dem Zweck der Station.
    pub(crate) fn form_for(&self, station: &Station, wanted: Option<&Uuid>) -> Result<Form> {
        let purpose = station.form.as_deref().unwrap_or("charting");
        let forms = self.form_defs()?;
        let fitting: Vec<&Form> = forms.list.iter().filter(|f| f.purpose == purpose).collect();
        let form = match wanted {
            Some(id) => fitting.into_iter().find(|f| f.id == *id),
            None if fitting.len() > 1 => return Err(invalid(tr!("Mit welchem Formular arbeitet die Stufe?"))),
            None => fitting.into_iter().next(),
        };
        form.cloned()
            .ok_or_else(|| invalid(tr!("Für die Station „{}“ ist noch kein Formular veröffentlicht.", station.label)))
    }

    /// Haupteinträge der eingeschlossenen Quellen für eine Formular-Stufe. Eine Stichprobe zieht je
    /// Quellenart `per_type` Quellen; mit Seed lässt sie sich nachrechnen.
    pub(crate) fn form_stage_ids(&self, per_type: Option<i64>, seed: &mut Option<i64>) -> Result<Vec<String>> {
        let studies = self.studies()?;
        let units = crate::sources::units(&self.selection()?.included, &studies);
        let Some(per_type) = per_type else {
            let mut ids: Vec<String> = units.iter().map(|u| u.primary.to_string()).collect();
            ids.sort();
            return Ok(ids);
        };
        let s = seed.unwrap_or_else(|| rand::thread_rng().gen_range(1..2_147_483_647));
        *seed = Some(s);
        let mut by_type: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for unit in &units {
            by_type.entry(unit.source_type.as_str()).or_default().push(unit.primary.to_string());
        }
        let mut rng = ChaCha8Rng::seed_from_u64(s as u64);
        let mut picked = Vec::new();
        for ids in by_type.values_mut() {
            ids.sort();
            let amount = (per_type.max(1) as usize).min(ids.len());
            picked.extend(sample(&mut rng, ids.len(), amount).into_iter().map(|i| ids[i].clone()));
        }
        picked.sort();
        Ok(picked)
    }

    fn form_parts(&self, stage_id: &Uuid, allowed: &[&str]) -> Result<Parts> {
        let state = self.state()?;
        let role = require(&state, &self.me.actor_id, allowed)?.to_string();
        let events = self.store.stage_events(stage_id)?;
        let stage = StageState::build(stage_id, &events).ok_or_else(|| not_found(tr!("Stufe nicht gefunden.")))?;
        let form_id =
            stage.config.form_id.ok_or_else(|| invalid(tr!("Diese Stufe arbeitet nicht mit einem Formular.")))?;
        let form = self.form_defs()?.get(&form_id).cloned();
        let form = form.ok_or_else(|| not_found(tr!("Das Formular dieser Stufe ist hier nicht bekannt.")))?;
        Ok(Parts { role, stage, form, values: FormState::build(&events) })
    }

    fn form_record(&self, stage_id: &Uuid, record_id: &str) -> Result<String> {
        let id = Uuid::parse_str(record_id).map_err(|_| invalid(tr!("Ungültige ID des Eintrags.")))?;
        if !self.store.is_assigned(stage_id, &id)? {
            return Err(not_found(tr!("Diese Quelle gehört nicht zu dieser Stufe.")));
        }
        Ok(id.to_string())
    }

    fn with_names(&self, mut compared: Compared) -> Result<Compared> {
        for cell in &mut compared.cells {
            for given in &mut cell.values {
                given.name = self.name_of(&given.actor_id)?;
            }
        }
        Ok(compared)
    }

    /// Tabelle einer Formular-Stufe: Zeilen sind Quellen, Spalten die Felder der geltenden Version.
    /// Solange die Stufe verblindet ist, stehen darin nur die eigenen Werte.
    pub fn form_stage(&self, stage_id: &Uuid) -> Result<FormStageView> {
        let parts = self.form_parts(stage_id, &ROLES)?;
        let me = self.me.actor_id;
        let blind = parts.stage.blind_active();
        let studies = self.studies()?;
        let template = self.workflow()?.template.localized();
        let mut rows = Vec::new();
        let mut counts = FormCounts::default();
        let mut times: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        for assignment in self.store.assignments(Some(stage_id))? {
            let id = assignment.record_id;
            let record = id.to_string();
            let Some(row) = self.store.record(&id)? else { continue };
            let (label, records, source_type) = unit_of(&studies, &id);
            let compared = parts.compare(&record);
            match compared.status {
                "open" => counts.open += 1,
                "extracted" => counts.extracted += 1,
                "conflict" => counts.conflict += 1,
                "incomplete" => counts.incomplete += 1,
                _ => counts.done += 1,
            }
            let timed =
                parts.values.completed.iter().filter(|((r, a), c)| *r == record && !c.verify && (!blind || *a == me));
            for seconds in timed.filter_map(|(_, c)| c.seconds) {
                let entry = times.entry(source_type.clone()).or_default();
                *entry = (entry.0 + 1, entry.1 + seconds);
            }
            let cells = match blind {
                true => parts
                    .fields()
                    .iter()
                    .map(|f| Cell {
                        field: f.key.clone(),
                        status: "mine",
                        value: parts.values.value_of(&record, &me, &f.key),
                        values: Vec::new(),
                    })
                    .collect(),
                false => self.with_names(compared.clone())?.cells,
            };
            rows.push(FormRow {
                record_id: id,
                title: row.title,
                authors: row.authors,
                year: row.year,
                label,
                source_type,
                n_records: records.len(),
                mine: parts.mine(&record, &me),
                status: (!blind).then_some(compared.status),
                n_completed: (!blind).then_some(compared.n_completed),
                cells,
            });
        }
        Ok(FormStageView {
            mine_done: rows.iter().filter(|r| r.mine == "completed").count(),
            todo: rows.iter().filter(|r| r.mine == "todo").count(),
            counts: (!blind).then_some(counts),
            times: times
                .into_iter()
                .map(|(source_type, (n, sum))| TimeRow { source_type, n, seconds: sum / n.max(1) })
                .collect(),
            stage: stage_view(&parts.stage),
            form: parts.form_ref(),
            role: parts.role,
            rows,
            members: self.members()?,
            source_types: template.source_types,
            terms: template.terms,
        })
    }

    /// Eine Quelle mit den eigenen Werten; nach dem Entblinden auch mit dem Abgleich je Feld.
    pub fn form_entry(&self, stage_id: &Uuid, record_id: &str) -> Result<FormEntryView> {
        let parts = self.form_parts(stage_id, &ROLES)?;
        let record = self.form_record(stage_id, record_id)?;
        let me = self.me.actor_id;
        let id = Uuid::parse_str(&record).map_err(|_| invalid(tr!("Ungültige ID des Eintrags.")))?;
        let (label, records, source_type) = unit_of(&self.studies()?, &id);
        let fulltexts = self.fulltexts()?;
        let mut companions = Vec::new();
        for other in records.iter().filter(|r| **r != id) {
            if let Some(row) = self.store.record(other)? {
                companions.push(RecordItem::from(row));
            }
        }
        let row = self.store.record(&id)?.ok_or_else(|| not_found(tr!("Record nicht gefunden.")))?;
        let order: Vec<Uuid> = self.store.assignments(Some(stage_id))?.into_iter().map(|a| a.record_id).collect();
        let position = order.iter().position(|r| *r == id).unwrap_or(0);
        let wanted = |r: &&Uuid| matches!(parts.mine(&r.to_string(), &me), "open" | "draft" | "todo" | "to_verify");
        let next_open =
            order.iter().skip(position + 1).find(wanted).or_else(|| order.iter().take(position).find(wanted)).copied();
        let key = (record.clone(), me);
        let writes = WRITE_ROLES.contains(&parts.role.as_str()) && !parts.stage.closed;
        let mode = match (writes, parts.verifies(&record, &me)) {
            (false, _) => "read",
            (true, true) => "verify",
            (true, false) => "extract",
        };
        let template = self.workflow()?.template.localized();
        Ok(FormEntryView {
            mode,
            values: parts.values.values.get(&key).cloned().unwrap_or_default(),
            completed: parts.values.completed.get(&key).map(|c| EntryCompletion {
                form_version: c.form_version,
                seconds: c.seconds,
                verify: c.verify,
            }),
            missing: parts.values.missing(&record, &me, parts.fields()).into_iter().map(|f| f.label.clone()).collect(),
            compared: match parts.stage.blind_active() {
                true => None,
                false => Some(self.with_names(parts.compare(&record))?),
            },
            position: position + 1,
            total: order.len(),
            previous: position.checked_sub(1).and_then(|i| order.get(i)).copied(),
            next: order.get(position + 1).copied(),
            next_open,
            stage: stage_view(&parts.stage),
            form: parts.form_ref(),
            role: parts.role,
            record: row.into(),
            companions,
            fulltexts: records.into_iter().filter(|r| fulltexts.is_retrieved(r)).collect(),
            label,
            source_type,
            source_types: template.source_types,
            terms: template.terms,
        })
    }

    /// Hält den Wert eines Feldes fest. Gibt `false` zurück, wenn er schon so dastand.
    pub fn record_form_value(&self, stage_id: &Uuid, record_id: &str, field: &str, value: &Value) -> Result<bool> {
        let parts = self.form_parts(stage_id, &WRITE_ROLES)?;
        if parts.stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        let record = self.form_record(stage_id, record_id)?;
        let me = self.me.actor_id;
        if parts.verifies(&record, &me) {
            return Err(conflict(tr!(
                "Diese Quelle hat schon jemand extrahiert. Beim Prüfen trägst du Korrekturen als geklärten Wert ein."
            )));
        }
        let def = parts.fields().iter().find(|f| f.key == field);
        let def = def.ok_or_else(|| invalid(tr!("Dieses Feld gibt es im Formular nicht.")))?;
        let value = normal(value);
        check_value(def, &value).map_err(invalid)?;
        if parts.values.value_of(&record, &me, field) == value {
            return Ok(false);
        }
        self.append(
            "FormValueRecorded",
            Some(*stage_id),
            json!({
                "record_id": record, "field": field, "group_index": 0, "value": value,
                "form_version": parts.form.current().version,
            }),
        )?;
        Ok(true)
    }

    /// Meldet eine Quelle als fertig. Dafür müssen alle Pflichtfelder der geltenden Version gefüllt
    /// sein. Hat bei „einfach mit Prüfung“ schon jemand anderes extrahiert, ist das der Abschluss der
    /// Prüfung.
    pub fn complete_form_entry(&self, stage_id: &Uuid, record_id: &str, seconds: Option<i64>) -> Result<()> {
        let parts = self.form_parts(stage_id, &WRITE_ROLES)?;
        if parts.stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        let record = self.form_record(stage_id, record_id)?;
        let me = self.me.actor_id;
        let verify = parts.verifies(&record, &me);
        if !verify {
            let missing = parts.values.missing(&record, &me, parts.fields());
            if !missing.is_empty() {
                let names: Vec<&str> = missing.iter().map(|f| f.label.as_str()).collect();
                return Err(invalid(tr!("Es fehlen Pflichtfelder: {}.", names.join(", "))));
            }
        }
        let mut payload = json!({"record_id": record, "form_version": parts.form.current().version, "verify": verify});
        if let Some(seconds) = seconds.filter(|s| *s >= 0) {
            payload["seconds"] = json!(seconds);
        }
        self.append("FormEntryCompleted", Some(*stage_id), payload)?;
        Ok(())
    }

    /// Legt den gültigen Wert eines Feldes fest: nach einer Abweichung oder als Korrektur beim Prüfen.
    pub fn record_form_consensus(
        &self,
        stage_id: &Uuid,
        record_id: &str,
        field: &str,
        value: &Value,
        note: &str,
    ) -> Result<()> {
        let parts = self.form_parts(stage_id, &WRITE_ROLES)?;
        if parts.stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        if parts.stage.blind_active() {
            return Err(forbidden(tr!("Die Stufe ist verblindet. Geklärt wird erst nach dem Entblinden.")));
        }
        let record = self.form_record(stage_id, record_id)?;
        let def = parts.fields().iter().find(|f| f.key == field);
        let def = def.ok_or_else(|| invalid(tr!("Dieses Feld gibt es im Formular nicht.")))?;
        let value = normal(value);
        check_value(def, &value).map_err(invalid)?;
        self.append(
            "FormConsensusRecorded",
            Some(*stage_id),
            json!({
                "record_id": record, "field": field, "group_index": 0, "value": value,
                "form_version": parts.form.current().version, "note": note.trim(),
            }),
        )?;
        Ok(())
    }

    /// Eingeschlossene Quellen, von denen kein Eintrag in einer Runde dieser Station liegt. Nach einem
    /// Update oder einem spät beschafften Volltext sind das die neuen Quellen.
    pub(crate) fn sources_without_round(
        &self,
        workflow: &Workflow,
        stages: &[StageState],
        station: &Station,
    ) -> Result<i64> {
        let mut assigned: BTreeSet<Uuid> = BTreeSet::new();
        for stage in stages.iter().filter(|s| workflow.station_of(s).is_some_and(|x| x.key == station.key)) {
            assigned.extend(self.store.assignments(Some(&stage.config.id))?.into_iter().map(|a| a.record_id));
        }
        let units = crate::sources::units(&self.selection()?.included, &self.studies()?);
        Ok(units.iter().filter(|u| !u.records.iter().any(|r| assigned.contains(r))).count() as i64)
    }

    /// Stand einer Formular-Stufe für den Bericht. Solange die Stufe verblindet ist, ohne Werte.
    pub(crate) fn form_summary(&self, stage: &StageState) -> Result<StageSummary> {
        let parts = self.form_parts(&stage.config.id, &ROLES)?;
        let blind = parts.stage.blind_active();
        let mut counts = FormCounts::default();
        let mut rows = Vec::new();
        let assignments = self.store.assignments(Some(&stage.config.id))?;
        for assignment in &assignments {
            let compared = parts.compare(&assignment.record_id.to_string());
            match compared.status {
                "open" => counts.open += 1,
                "extracted" => counts.extracted += 1,
                "conflict" => counts.conflict += 1,
                "incomplete" => counts.incomplete += 1,
                _ => counts.done += 1,
            }
            rows.push(match blind {
                true => SummaryRow { record_id: assignment.record_id, status: "blind", values: Vec::new() },
                false => SummaryRow {
                    record_id: assignment.record_id,
                    status: compared.status,
                    values: compared.cells.iter().map(cell_text).collect(),
                },
            });
        }
        let people: BTreeSet<Uuid> = parts.values.completed.keys().map(|(_, actor)| *actor).collect();
        let current = parts.form.current();
        Ok(StageSummary {
            form_name: current.name.clone(),
            form_version: current.version,
            confirmed: parts.form.confirmed,
            fields: current.fields.clone(),
            n_sources: assignments.len() as i64,
            counts: (!blind).then_some(counts),
            n_people: if blind { None } else { Some(people.len() as i64) },
            n_resolved: parts.values.consensus.len() as i64,
            rows,
        })
    }

    /// Gate einer Formular-Station für eine Stufe.
    pub(crate) fn form_gate(&self, station: &Station, stage: &StageState) -> Result<GateResult> {
        let parts = self.form_parts(&stage.config.id, &ROLES)?;
        let unassigned = match station.gate.iter().any(|g| g.check == "all_sources_assigned") {
            true => {
                let stages = self.stage_states(&self.state()?)?;
                self.sources_without_round(&self.workflow()?, &stages, station)?
            }
            false => 0,
        };
        let statuses: Vec<&str> = self
            .store
            .assignments(Some(&stage.config.id))?
            .into_iter()
            .map(|a| parts.compare(&a.record_id.to_string()).status)
            .collect();
        let total = statuses.len() as i64;
        let count = |wanted: &[&str]| statuses.iter().filter(|s| wanted.contains(s)).count() as i64;
        let checks: Vec<CheckResult> = station
            .gate
            .iter()
            .map(|g| {
                let (passed, numerator, denominator) = match g.check.as_str() {
                    "form_confirmed" => (parts.form.confirmed, i64::from(parts.form.confirmed), 1),
                    "all_extracted" => {
                        let n = count(&["done", "conflict"]);
                        (n == total, n, total)
                    }
                    "all_sources_assigned" => (unassigned == 0, unassigned, total + unassigned),
                    _ => {
                        let n = count(&["conflict"]);
                        (n == 0, n, total)
                    }
                };
                CheckResult {
                    check: g.check.clone(),
                    passed,
                    numerator,
                    denominator,
                    threshold_percent: None,
                    percent_x100: None,
                }
            })
            .collect();
        Ok(GateResult { passed: checks.iter().all(|c| c.passed), checks })
    }

    /// Häufigkeiten je Auswahlfeld über die gültigen Werte. Erst nach dem Entblinden.
    pub fn form_frequencies(&self, stage_id: &Uuid) -> Result<Vec<Frequency>> {
        let parts = self.form_parts(stage_id, &ROLES)?;
        if parts.stage.blind_active() {
            return Err(forbidden(tr!("Die Stufe ist verblindet. Ausgewertet wird erst nach dem Entblinden.")));
        }
        let compared: Vec<Compared> = self
            .store
            .assignments(Some(stage_id))?
            .into_iter()
            .map(|a| parts.compare(&a.record_id.to_string()))
            .collect();
        let mut out = Vec::new();
        for (index, field) in parts.fields().iter().enumerate().filter(|(_, f)| f.has_options()) {
            let mut counts: Vec<OptionCount> =
                field.options.iter().map(|o| OptionCount { option: o.clone(), n: 0 }).collect();
            let (mut n_empty, mut n_open) = (0, 0);
            for cell in compared.iter().filter_map(|c| c.cells.get(index)) {
                let chosen: Vec<&str> = match &cell.value {
                    Value::String(s) => vec![s.as_str()],
                    Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
                    _ => Vec::new(),
                };
                match (cell.status, chosen.is_empty()) {
                    ("differs", _) => n_open += 1,
                    (_, true) => n_empty += 1,
                    _ => {}
                }
                for option in chosen {
                    match counts.iter_mut().find(|c| c.option == option) {
                        Some(count) => count.n += 1,
                        // Werte aus einer früheren Version der Auswahl bleiben sichtbar.
                        None => counts.push(OptionCount { option: option.to_string(), n: 1 }),
                    }
                }
            }
            out.push(Frequency { field: field.key.clone(), label: field.label.clone(), counts, n_empty, n_open });
        }
        Ok(out)
    }

    /// Tabelle (`table`) oder Häufigkeiten (`frequencies`) einer Stufe als CSV, oder beides samt Formular
    /// als JSON (`json`). Im JSON stehen die Werte unter dem Schlüssel des Feldes und behalten ihren Typ.
    pub fn form_file(&self, stage_id: &Uuid, kind: &str) -> Result<FormFile> {
        if kind == "json" {
            return self.form_json(stage_id);
        }
        let line = |cells: Vec<String>| cells.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(",") + "\n";
        if kind == "frequencies" {
            let mut content = line(vec!["field".into(), "label".into(), "value".into(), "n".into()]);
            for f in self.form_frequencies(stage_id)? {
                for count in &f.counts {
                    content += &line(vec![f.field.clone(), f.label.clone(), count.option.clone(), count.n.to_string()]);
                }
                content += &line(vec![f.field.clone(), f.label.clone(), tr!("(leer)"), f.n_empty.to_string()]);
                content +=
                    &line(vec![f.field.clone(), f.label.clone(), tr!("(Abweichung offen)"), f.n_open.to_string()]);
            }
            return Ok(FormFile { filename: "extraktion-haeufigkeiten.csv".into(), content });
        }
        if kind != "table" {
            return Err(invalid(tr!("Unbekannte Datei.")));
        }
        let view = self.form_stage(stage_id)?;
        if view.stage.blind_active {
            return Err(forbidden(tr!("Die Stufe ist verblindet. Exportiert wird erst nach dem Entblinden.")));
        }
        let type_label =
            |key: &str| view.source_types.iter().find(|t| t.key == key).map_or(key.to_string(), |t| t.label.clone());
        let mut head: Vec<String> =
            ["nr", "record_id", "authors", "year", "title", "bundle", "source_type", "status"].map(String::from).into();
        head.extend(view.form.fields.iter().map(|f| f.label.clone()));
        let mut content = line(head);
        for (index, row) in view.rows.iter().enumerate() {
            let mut cells = vec![
                (index + 1).to_string(),
                row.record_id.to_string(),
                row.authors.clone(),
                row.year.map(|y| y.to_string()).unwrap_or_default(),
                row.title.clone(),
                row.label.clone(),
                type_label(&row.source_type),
                row.status.unwrap_or_default().to_string(),
            ];
            cells.extend(row.cells.iter().map(cell_text));
            content += &line(cells);
        }
        Ok(FormFile { filename: "extraktion-tabelle.csv".into(), content })
    }

    fn form_json(&self, stage_id: &Uuid) -> Result<FormFile> {
        let view = self.form_stage(stage_id)?;
        if view.stage.blind_active {
            return Err(forbidden(tr!("Die Stufe ist verblindet. Exportiert wird erst nach dem Entblinden.")));
        }
        let rows: Vec<Value> = view
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let mut values = serde_json::Map::new();
                let mut differences = serde_json::Map::new();
                for cell in &row.cells {
                    values.insert(cell.field.clone(), cell.value.clone());
                    if cell.status == "differs" {
                        differences.insert(cell.field.clone(), json!(cell.values));
                    }
                }
                json!({
                    "nr": index + 1, "record_id": row.record_id, "authors": row.authors, "year": row.year,
                    "title": row.title, "bundle": row.label, "source_type": row.source_type,
                    "status": row.status, "values": values, "differences": differences,
                })
            })
            .collect();
        let file = json!({
            "review": { "id": view.stage.review_id, "title": self.state()?.title },
            "stage": { "id": view.stage.id, "name": view.stage.name, "closed": view.stage.closed },
            "form": { "id": view.form.id, "name": view.form.name, "version": view.form.version, "fields": view.form.fields },
            "sources": rows,
            "frequencies": self.form_frequencies(stage_id)?,
        });
        let content = serde_json::to_string_pretty(&file).unwrap_or_default() + "\n";
        Ok(FormFile { filename: "extraktion.json".into(), content })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(key: &str, kind: &str, required: bool) -> Field {
        Field {
            key: key.into(),
            label: key.to_uppercase(),
            description: String::new(),
            kind: kind.into(),
            required,
            group: String::new(),
            analysis: String::new(),
            options: match kind {
                "choice" | "multi_choice" => vec!["a".into(), "b".into()],
                _ => Vec::new(),
            },
        }
    }

    #[test]
    fn values_must_fit_the_field_type() {
        let ok = |kind: &str, value: Value| check_value(&field("f", kind, false), &value).is_ok();
        assert!(ok("text", json!("frei")) && ok("quote", json!("„so“, S. 3")) && ok("text", Value::Null));
        assert!(ok("integer", json!(12)) && !ok("integer", json!("12")) && !ok("integer", json!(1.5)));
        assert!(ok("decimal", json!("12,5")) && ok("decimal", json!("-0.25")) && ok("decimal", json!("7")));
        assert!(!ok("decimal", json!("1,2,3")) && !ok("decimal", json!("abc")) && !ok("decimal", json!(",5")));
        assert!(ok("choice", json!("a")) && !ok("choice", json!("c")) && !ok("choice", json!(["a"])));
        assert!(ok("multi_choice", json!(["a", "b"])) && !ok("multi_choice", json!(["a", "c"])));
        assert!(!ok("text", json!(1.5)) && !ok("text", json!({"a": 1})));
    }

    #[test]
    fn normal_form_makes_values_comparable() {
        assert_eq!(normal(&json!("  Prag ")), json!("Prag"));
        assert_eq!(normal(&json!("   ")), Value::Null);
        assert_eq!(normal(&json!(["b", "a", "b"])), json!(["a", "b"]));
        assert_eq!(normal(&json!([])), Value::Null);
        assert_eq!(show(&json!(["a", "b"])), "a; b");
        assert_eq!(show(&json!(12)), "12");
    }

    #[test]
    fn broken_field_lists_are_refused() {
        assert!(check_fields(&[]).is_err());
        assert!(check_fields(&[field("Land", "text", false)]).is_err(), "Großbuchstabe im Schlüssel");
        assert!(check_fields(&[field("a", "text", false), field("a", "text", false)]).is_err());
        assert!(check_fields(&[field("a", "bild", false)]).is_err());
        let mut choice = field("a", "choice", false);
        choice.options = vec!["x".into(), " x".into()];
        assert!(check_fields(&[choice]).is_err());
        assert!(check_fields(&[field("a", "choice", true), field("b_2", "decimal", false)]).is_ok());
    }
}
