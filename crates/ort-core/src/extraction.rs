//! KI-Vorschlag zur Extraktion: Werte für die Felder eines Formulars, gelesen aus dem Volltext.
//!
//! Wie beim Screening ([`crate::ai`]) spricht der Kern mit keiner API. Er baut die Anfrage aus Formular
//! und Volltext ([`Review::ai_extraction_request`]), die App sendet sie, und der Kern liest die Antwort
//! ([`parse`]) gegen die Felder des Formulars. Der Prompt entsteht aus einer Vorlage mit Platzhaltern
//! ([`default_template`], [`render`]), die sich in den Einstellungen ändern lässt. Den Text des Volltexts liefert die Oberfläche Seite für
//! Seite aus dem PDF; im Log steht nur die Prüfsumme der Datei, aus der er stammt.
//!
//! Ein Vorschlag ist kein Wert: `AiExtractionReceived` hält je Feld Wert, Belegstelle und Seite fest und
//! zählt in keinem Abgleich. Erst [`Review::apply_ai_extraction`] trägt die gewählten Werte als gewöhnliche
//! `FormValueRecorded` ein und vermerkt mit `AiExtractionResolved`, welche Felder übernommen wurden.
//! Beides liegt in der Kette der Stufe und bleibt damit während der Verblindung bei der Person.

use std::collections::{BTreeMap, BTreeSet};

use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::ai::{Message, Options, Request};
use crate::catalog::MAX_EVIDENCE;
use crate::error::{conflict, forbidden, invalid, not_found, Result};
use crate::event::{sha256_hex, Event};
use crate::forms::{check_value, normal, unit_of, Field, MAX_VALUE};
use crate::registration;
use crate::service::{Review, WRITE_ROLES};
use crate::tr;

/// So viele Zeichen des Volltexts gehen höchstens an die KI; der Rest fällt weg und wird gemeldet.
pub const MAX_FULLTEXT: usize = 400_000;
/// Mit weniger lesbarem Text gilt ein PDF als Scan ohne Textebene.
const MIN_FULLTEXT: usize = 200;

/// Vorschlag zu einem Feld.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Proposed {
    pub value: Value,
    /// Stelle im Volltext, die den Wert belegt; leer, wenn die KI keine nennt.
    pub quote: String,
    pub page: Option<i64>,
}

/// Ein Vorschlag, wie ihn eine Person zu einer Quelle eingeholt hat.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExtractionInfo {
    pub id: Uuid,
    pub model: String,
    /// Prüfsumme der Datei, deren Text an die KI ging.
    pub fulltext_sha256: String,
    pub form_version: i64,
    /// Der Volltext war länger als [`MAX_FULLTEXT`]; die KI sah nur den Anfang.
    pub truncated: bool,
    /// Feld → Vorschlag; Felder ohne Vorschlag fehlen.
    pub fields: BTreeMap<String, Proposed>,
    /// Übernommene Felder; `None`, solange nichts vermerkt ist.
    pub accepted: Option<Vec<String>>,
    pub at: String,
}

impl ExtractionInfo {
    /// Liest einen Vorschlag aus `AiExtractionReceived`.
    pub(crate) fn from_event(e: &Event) -> ExtractionInfo {
        let fields = e.payload.get("fields").and_then(Value::as_object).into_iter().flatten();
        ExtractionInfo {
            id: e.event_id,
            model: e.payload_str("model").unwrap_or_default().to_string(),
            fulltext_sha256: e.payload_str("fulltext_sha256").unwrap_or_default().to_string(),
            form_version: e.payload.get("form_version").and_then(Value::as_i64).unwrap_or(0),
            truncated: e.payload.get("truncated").and_then(Value::as_bool).unwrap_or(false),
            fields: fields
                .map(|(key, p)| {
                    let proposed = Proposed {
                        value: normal(p.get("value").unwrap_or(&Value::Null)),
                        quote: p.get("quote").and_then(Value::as_str).unwrap_or_default().to_string(),
                        page: p.get("page").and_then(Value::as_i64),
                    };
                    (key.clone(), proposed)
                })
                .collect(),
            accepted: None,
            at: e.occurred_at.clone(),
        }
    }
}

/// Eine fertige Anfrage und was davon ins Log gehört.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ExtractionRequest {
    pub request: Request,
    pub fulltext_sha256: String,
    pub truncated: bool,
}

/// KI-Nutzung einer Formular-Stufe für den Methodentext.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AiUse {
    /// Zahl der Vorschläge: je Quelle und Person der letzte.
    pub sources_by_person: i64,
    pub models: Vec<String>,
    /// Vorgeschlagene und davon übernommene Werte.
    pub proposed: i64,
    pub accepted: i64,
    /// Zeitpunkte der Vorschläge, um solche nach einer Abschaltung zu zählen.
    pub at: Vec<String>,
}

impl AiUse {
    pub(crate) fn of(extractions: &BTreeMap<(String, Uuid), ExtractionInfo>) -> Option<AiUse> {
        if extractions.is_empty() {
            return None;
        }
        let models: BTreeSet<&str> = extractions.values().map(|x| x.model.as_str()).collect();
        Some(AiUse {
            sources_by_person: extractions.len() as i64,
            models: models.into_iter().map(str::to_string).collect(),
            proposed: extractions.values().map(|x| x.fields.len() as i64).sum(),
            accepted: extractions.values().map(|x| x.accepted.as_ref().map_or(0, |a| a.len() as i64)).sum(),
            at: extractions.values().map(|x| x.at.clone()).collect(),
        })
    }
}

/// Platzhalter einer Vorlage, in der Reihenfolge der Standardvorlage.
pub const PLACEHOLDERS: [&str; 6] = ["review", "questions", "source", "fields", "fulltext", "format"];

/// Die Teile, die an die Stelle der Platzhalter treten.
#[derive(Clone, Debug, Default)]
pub struct Blocks {
    pub review: String,
    pub questions: String,
    pub source: String,
    pub fields: String,
    pub fulltext: String,
    pub format: String,
}

/// Die Vorlage, die gilt, solange niemand eine eigene hinterlegt hat.
pub fn default_template() -> String {
    [
        tr!("Du unterstützt die Datenextraktion für ein Review. Lies den Volltext und trage zu jedem Feld des Formulars ein, was der Volltext dazu berichtet. Stütze dich nur auf den Volltext."),
        format!("## {}\n{{{{review}}}}", tr!("Review")),
        format!("## {}\n{{{{questions}}}}", tr!("Fragestellung")),
        format!("## {}\n{{{{source}}}}", tr!("Quelle")),
        format!("## {}\n{{{{fields}}}}", tr!("Formular")),
        format!("## {}\n{{{{fulltext}}}}", tr!("Volltext")),
        format!("## {}\n{{{{format}}}}", tr!("Antwort")),
    ]
    .join("\n\n")
}

/// Setzt die Teile in die Vorlage ein. Fehlen in einer eigenen Vorlage die Felder, der Volltext oder das
/// Antwortformat, kommen sie ans Ende: Ohne sie gäbe es nichts zu lesen, nichts zu füllen und keine
/// lesbare Antwort.
pub fn render(template: &str, blocks: &Blocks) -> String {
    let template = if template.trim().is_empty() { default_template() } else { template.trim().to_string() };
    let parts = [
        ("review", &blocks.review),
        ("questions", &blocks.questions),
        ("source", &blocks.source),
        ("fields", &blocks.fields),
        ("format", &blocks.format),
        // Zuletzt, damit ein Platzhalter im Text des Volltexts selbst nicht ersetzt wird.
        ("fulltext", &blocks.fulltext),
    ];
    let mut out = template.clone();
    for (name, text) in parts {
        out = out.replace(&format!("{{{{{name}}}}}"), text);
    }
    for (name, text) in [("fields", &blocks.fields), ("fulltext", &blocks.fulltext), ("format", &blocks.format)] {
        if !template.contains(&format!("{{{{{name}}}}}")) {
            out.push_str("\n\n");
            out.push_str(text);
        }
    }
    out
}

fn kind_label(kind: &str) -> String {
    match kind {
        "integer" => tr!("ganze Zahl"),
        "decimal" => tr!("Dezimalzahl"),
        "choice" => tr!("Auswahl, genau ein Wert"),
        "multi_choice" => tr!("Mehrfachauswahl"),
        "quote" => tr!("wörtliches Zitat"),
        _ => tr!("Text"),
    }
}

/// Die Felder des Formulars als Text: Schlüssel, Name, Typ, Beschreibung und Auswahlwerte.
pub fn fields_block(fields: &[Field]) -> String {
    let mut out = Vec::new();
    let mut group = "";
    for f in fields {
        if f.group != group && !f.group.trim().is_empty() {
            out.push(format!("### {}", f.group.trim()));
        }
        group = &f.group;
        let mut line = format!("- {} ({}) [{}]", f.key, f.label.trim(), kind_label(&f.kind));
        if !f.description.trim().is_empty() {
            line.push_str(&format!(": {}", f.description.split_whitespace().collect::<Vec<_>>().join(" ")));
        }
        if !f.options.is_empty() {
            let options: Vec<String> = f.options.iter().map(|o| format!("„{o}“")).collect();
            line.push_str(&format!(" {}", tr!("Auswahlwerte: {}", options.join(", "))));
        }
        out.push(line);
    }
    out.join("\n")
}

/// Der Volltext mit einer Marke vor jeder Seite. Gibt auch zurück, ob gekürzt wurde.
pub fn fulltext_block(pages: &[String]) -> (String, bool) {
    let mut out = String::new();
    let mut left = MAX_FULLTEXT;
    for (index, page) in pages.iter().enumerate() {
        let text = page.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        if left == 0 {
            return (out, true);
        }
        let part: String = text.chars().take(left).collect();
        let cut = part.len() < text.len();
        left -= part.chars().count();
        out.push_str(&format!("[{}]\n{part}\n\n", tr!("Seite {}", index + 1)));
        if cut {
            return (out, true);
        }
    }
    (out, false)
}

/// Das verlangte Antwortformat. Die Feldnamen sind fest, weil [`parse`] sie liest.
pub fn format_block() -> String {
    [
        tr!("Antworte nur mit einem JSON-Objekt in dieser Form, ohne weiteren Text:"),
        r#"{"fields": {"<key>": {"value": <value> | null, "quote": "<text>", "page": <number> | null}}}"#.to_string(),
        tr!("Nenne jedes Feld des Formulars genau einmal unter seinem Schlüssel."),
        tr!("Steht eine Angabe nicht im Volltext, ist „value“ null. Rate nicht und ergänze nichts aus eigenem Wissen."),
        tr!("„value“ ist bei Text und Zitat eine Zeichenkette, bei Zahlen eine Zahl, bei einer Auswahl genau einer der genannten Werte und bei einer Mehrfachauswahl eine Liste der genannten Werte."),
        tr!("„quote“ ist die wörtliche Stelle aus dem Volltext, die den Wert belegt, höchstens zwei Sätze. „page“ ist die Seite, auf der sie steht, nach den Marken im Volltext."),
        tr!("Freie Texte schreibst du knapp und auf Deutsch; ein Zitat bleibt in der Sprache des Volltexts."),
    ]
    .join("\n")
}

/// Bringt einen Wert aus der Antwort in die Form des Feldes. Was nicht passt, fällt weg.
fn coerce(field: &Field, value: &Value) -> Option<Value> {
    let option = |text: &str| field.options.iter().find(|o| o.trim().eq_ignore_ascii_case(text.trim())).cloned();
    let fitted = match (field.kind.as_str(), value) {
        (_, Value::Null) => return None,
        ("text" | "quote", Value::String(s)) => json!(s.trim().chars().take(MAX_VALUE).collect::<String>()),
        ("text" | "quote", Value::Number(n)) => json!(n.to_string()),
        ("text" | "quote", Value::Array(items)) => {
            json!(items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; "))
        }
        ("integer", Value::Number(n)) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => json!(i),
            (None, Some(f)) if f.fract() == 0.0 && f.abs() < 9e15 => json!(f as i64),
            _ => return None,
        },
        ("integer", Value::String(s)) => json!(s.trim().parse::<i64>().ok()?),
        ("decimal", Value::Number(n)) => json!(n.to_string()),
        ("decimal", Value::String(s)) => json!(s.trim()),
        ("choice", Value::String(s)) => json!(option(s)?),
        ("multi_choice", Value::String(s)) => json!([option(s)?]),
        ("multi_choice", Value::Array(items)) => {
            json!(items.iter().filter_map(Value::as_str).filter_map(option).collect::<Vec<_>>())
        }
        _ => return None,
    };
    let fitted = normal(&fitted);
    (!fitted.is_null() && check_value(field, &fitted).is_ok()).then_some(fitted)
}

/// Liest die Antwort der KI gegen die Felder des Formulars. Verträgt Text um das JSON herum, Code-Zäune
/// und Denkblöcke. Ein Wert, der nicht zum Feld passt, fällt weg; unbekannte Felder auch. Ohne lesbares
/// Objekt gibt es einen Fehler, ohne einen einzigen Wert eine leere Liste.
pub fn parse(text: &str, fields: &[Field]) -> std::result::Result<BTreeMap<String, Proposed>, String> {
    let thinking = Regex::new(r"(?s)<think>.*?</think>").expect("fester Ausdruck");
    let text = thinking.replace_all(text, "");
    let known = |object: &Value| fields.iter().any(|f| object.get(&f.key).is_some());
    let object = text.char_indices().filter(|(_, c)| *c == '{').find_map(|(start, _)| {
        let value = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>().next()?.ok()?;
        // Manche Modelle lassen die Hülle „fields“ weg und nennen die Felder direkt.
        match value.get("fields") {
            Some(inner) if inner.is_object() => Some(inner.clone()),
            _ => known(&value).then_some(value),
        }
    });
    let Some(object) = object else {
        return Err(tr!("Die Antwort der KI enthält keine lesbaren Werte für das Formular."));
    };
    let mut out = BTreeMap::new();
    for field in fields {
        let Some(given) = object.get(&field.key) else { continue };
        // Ein Feld steht als Objekt mit „value“ da oder gleich als Wert.
        let (value, quote, page) = match given.as_object() {
            Some(parts) => (parts.get("value"), parts.get("quote"), parts.get("page")),
            None => (Some(given), None, None),
        };
        let Some(value) = value.and_then(|v| coerce(field, v)) else { continue };
        let quote: String = quote
            .and_then(Value::as_str)
            .map(|q| q.split_whitespace().collect::<Vec<_>>().join(" "))
            .unwrap_or_default()
            .chars()
            .take(MAX_EVIDENCE)
            .collect();
        let page = page.and_then(|p| p.as_i64().or_else(|| p.as_str()?.trim().parse().ok())).filter(|p| *p >= 1);
        out.insert(field.key.clone(), Proposed { value, quote, page });
    }
    Ok(out)
}

impl Review {
    /// Baut die Anfrage für eine Quelle einer Formular-Stufe aus dem Text ihres Volltexts. `fulltext`
    /// ist der Eintrag der Quelle, dessen Datei gelesen wurde, `pages` deren Text je Seite. Schreibt nichts.
    pub fn ai_extraction_request(
        &self,
        stage_id: &Uuid,
        record_id: &str,
        fulltext: &Uuid,
        pages: &[String],
        options: &Options,
    ) -> Result<ExtractionRequest> {
        let parts = self.form_parts(stage_id, &WRITE_ROLES)?;
        let state = self.state()?;
        if !state.ai_allowed() {
            return Err(forbidden(tr!("Die Leitung hat KI-Vorschläge für dieses Review abgeschaltet.")));
        }
        if parts.stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        let record = self.form_record(stage_id, record_id)?;
        if parts.verifies(&record, &self.me().actor_id) {
            return Err(conflict(tr!("Diese Quelle prüfst du nur. Einen KI-Vorschlag gibt es beim Extrahieren.")));
        }
        let id = Uuid::parse_str(&record).map_err(|_| invalid(tr!("Ungültige ID des Eintrags.")))?;
        let (_, records, _) = unit_of(&self.studies()?, &id);
        let file = self.fulltext_of(fulltext)?.filter(|f| f.status == "retrieved" && records.contains(fulltext));
        let file = file.ok_or_else(|| not_found(tr!("Zu dieser Quelle ist kein Volltext beschafft.")))?;
        let (text, truncated) = fulltext_block(pages);
        if text.chars().count() < MIN_FULLTEXT {
            return Err(invalid(tr!(
                "Aus diesem Volltext lässt sich kein Text lesen. Vermutlich ist es ein Scan ohne Textebene."
            )));
        }

        let row = self.record_item(&record)?;
        let workflow = self.workflow()?;
        let template = workflow.template.localized();
        let line = |label: String, value: &str| {
            let value = value.trim();
            (!value.is_empty()).then(|| format!("{label}: {value}"))
        };
        let mut review = Vec::new();
        review.extend(line(tr!("Titel"), &state.title));
        review.extend(line(tr!("Art des Reviews"), &template.review_type));
        review.extend(line(tr!("Stufe"), &parts.stage.config.name));
        let (answers, _) = registration::answers(&self.store().events_of_types(&["ProtocolDrafted"])?);
        let questions: Vec<&str> = ["primary_questions", "secondary_questions"]
            .iter()
            .filter_map(|k| answers.get(*k))
            .map(String::as_str)
            .collect();
        let questions = if questions.is_empty() { tr!("Nicht hinterlegt.") } else { questions.join("\n") };
        let mut source = Vec::new();
        source.extend(line(tr!("Titel"), &row.title));
        if options.publication {
            source.extend(line(tr!("Autoren"), &row.authors));
            source.extend(line(tr!("Jahr"), &row.year.map(|y| y.to_string()).unwrap_or_default()));
            source.extend(line(tr!("Zeitschrift"), &row.journal));
            source.extend(line(tr!("DOI"), &row.doi));
        }
        let mut fulltext = text.trim_end().to_string();
        if truncated {
            fulltext.push_str(&format!("\n\n{}", tr!("Der Volltext ist hier gekürzt; das Ende fehlt.")));
        }
        let blocks = Blocks {
            review: review.join("\n"),
            questions,
            source: source.join("\n"),
            fields: fields_block(parts.fields()),
            fulltext,
            format: format_block(),
        };
        let content = render(&options.template, &blocks);
        let prompt_sha256 = sha256_hex(content.as_bytes());
        Ok(ExtractionRequest {
            request: Request { messages: vec![Message { role: "user".into(), content }], prompt_sha256 },
            fulltext_sha256: file.sha256,
            truncated,
        })
    }

    /// Liest die Antwort der KI gegen das Formular der Stufe und vermerkt den Vorschlag im Log.
    pub fn record_ai_extraction(
        &self,
        stage_id: &Uuid,
        record_id: &str,
        model: &str,
        sent: &ExtractionRequest,
        answer: &str,
    ) -> Result<ExtractionInfo> {
        let parts = self.form_parts(stage_id, &WRITE_ROLES)?;
        if !self.state()?.ai_allowed() {
            return Err(forbidden(tr!("Die Leitung hat KI-Vorschläge für dieses Review abgeschaltet.")));
        }
        if parts.stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        let record = self.form_record(stage_id, record_id)?;
        let fields = parse(answer, parts.fields()).map_err(invalid)?;
        let listed: serde_json::Map<String, Value> = fields
            .iter()
            .map(|(key, p)| {
                let mut entry = json!({"value": p.value, "quote": p.quote});
                if let Some(page) = p.page {
                    entry["page"] = json!(page);
                }
                (key.clone(), entry)
            })
            .collect();
        let event = self.append(
            "AiExtractionReceived",
            Some(*stage_id),
            json!({
                "record_id": record,
                "model": model,
                "prompt_sha256": sent.request.prompt_sha256,
                "fulltext_sha256": sent.fulltext_sha256,
                "form_version": parts.form.current().version,
                "truncated": sent.truncated,
                "fields": listed,
            }),
        )?;
        Ok(ExtractionInfo::from_event(&event))
    }

    /// Trägt die gewählten Werte eines eigenen Vorschlags ins Formular ein und vermerkt, welche Felder
    /// übernommen wurden; eine leere Liste verwirft den Vorschlag. Gibt die Zahl der geänderten Werte zurück.
    pub fn apply_ai_extraction(
        &self,
        stage_id: &Uuid,
        record_id: &str,
        suggestion: &Uuid,
        accepted: &[String],
    ) -> Result<usize> {
        let parts = self.form_parts(stage_id, &WRITE_ROLES)?;
        if parts.stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        let record = self.form_record(stage_id, record_id)?;
        let me = self.me().actor_id;
        if parts.verifies(&record, &me) {
            return Err(conflict(tr!("Diese Quelle prüfst du nur. Einen KI-Vorschlag gibt es beim Extrahieren.")));
        }
        let info = match parts.values.extractions.get(&(record.clone(), me)) {
            Some(x) if x.id == *suggestion && x.accepted.is_none() => x,
            Some(x) if x.id == *suggestion => {
                return Err(conflict(tr!("Zu diesem KI-Vorschlag ist schon etwas vermerkt.")))
            }
            _ => return Err(not_found(tr!("KI-Vorschlag nicht gefunden."))),
        };
        let chosen: BTreeSet<&str> = accepted.iter().map(String::as_str).collect();
        let mut values = Vec::new();
        for key in &chosen {
            let proposed = info.fields.get(*key);
            let proposed = proposed.ok_or_else(|| invalid(tr!("Zu diesem Feld gibt es keinen Vorschlag der KI.")))?;
            // Das Formular kann seit dem Vorschlag eine neue Version bekommen haben.
            let def = parts.fields().iter().find(|f| f.key == *key);
            let def = def.ok_or_else(|| invalid(tr!("Dieses Feld gibt es im Formular nicht.")))?;
            check_value(def, &proposed.value).map_err(invalid)?;
            values.push((*key, &proposed.value));
        }
        let mut changed = 0;
        for (key, value) in values {
            if parts.values.value_of(&record, &me, key) != *value {
                self.append(
                    "FormValueRecorded",
                    Some(*stage_id),
                    json!({
                        "record_id": record, "field": key, "group_index": 0, "value": value,
                        "form_version": parts.form.current().version,
                    }),
                )?;
                changed += 1;
            }
        }
        self.append(
            "AiExtractionResolved",
            Some(*stage_id),
            json!({"record_id": record, "suggestion": suggestion.to_string(), "accepted": chosen}),
        )?;
        Ok(changed)
    }
}

/// Was die Eingabeseite über die KI wissen muss.
#[derive(Clone, Debug, Serialize)]
pub struct EntryAi {
    /// Die Leitung erlaubt KI-Vorschläge in diesem Review.
    pub allowed: bool,
    /// Der letzte eigene Vorschlag zu dieser Quelle.
    pub suggestion: Option<ExtractionInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(key: &str, kind: &str, options: &[&str]) -> Field {
        Field {
            key: key.into(),
            label: key.to_uppercase(),
            description: if key == "design" { "Wie die\nStudie angelegt ist".into() } else { String::new() },
            kind: kind.into(),
            required: false,
            group: if key == "n" { "Stichprobe".into() } else { String::new() },
            analysis: String::new(),
            options: options.iter().map(|o| o.to_string()).collect(),
        }
    }

    fn fields() -> Vec<Field> {
        vec![
            field("design", "choice", &["RCT", "Kohorte"]),
            field("n", "integer", &[]),
            field("age", "decimal", &[]),
            field("outcomes", "multi_choice", &["Schmerz", "Lebensqualität"]),
            field("aim", "text", &[]),
            field("passage", "quote", &[]),
        ]
    }

    #[test]
    fn blocks_name_fields_and_pages() {
        let block = fields_block(&fields());
        assert!(block.contains(
            "- design (DESIGN) [Auswahl, genau ein Wert]: Wie die Studie angelegt ist Auswahlwerte: „RCT“, „Kohorte“"
        ));
        assert!(block.contains("### Stichprobe\n- n (N) [ganze Zahl]"));
        assert!(format_block().contains(r#"{"fields": {"<key>""#));

        let (text, cut) = fulltext_block(&["Erste  Seite\nmit Text".into(), "  ".into(), "Dritte".into()]);
        assert_eq!(text, "[Seite 1]\nErste Seite mit Text\n\n[Seite 3]\nDritte\n\n");
        assert!(!cut);
        let (long, cut) = fulltext_block(&["ä".repeat(MAX_FULLTEXT - 1), "zwei".into(), "drei".into()]);
        assert!(cut && long.ends_with("[Seite 2]\nz\n\n") && !long.contains("drei"));
    }

    #[test]
    fn templates_fill_placeholders_and_keep_fields_fulltext_and_format() {
        let blocks = Blocks {
            review: "R".into(),
            questions: "Q".into(),
            source: "S".into(),
            fields: "FELDER".into(),
            fulltext: "TEXT mit {{format}}".into(),
            format: "FORMAT".into(),
        };
        let standard = render("", &blocks);
        for part in ["## Review\nR", "## Quelle\nS", "## Formular\nFELDER", "## Volltext\nTEXT mit {{format}}"] {
            assert!(standard.contains(part), "{part}");
        }
        assert!(standard.ends_with("## Antwort\nFORMAT"));
        for name in PLACEHOLDERS {
            assert!(default_template().contains(&format!("{{{{{name}}}}}")), "{name}");
        }
        assert_eq!(
            render("Lies {{fulltext}} für {{fields}}. {{format}}", &blocks),
            "Lies TEXT mit {{format}} für FELDER. FORMAT"
        );
        assert_eq!(render("Nur {{source}}", &blocks), "Nur S\n\nFELDER\n\nTEXT mit {{format}}\n\nFORMAT");
    }

    #[test]
    fn answers_are_fitted_to_the_fields() {
        let answer = r#"Gern. ```json
{"fields": {
  "design": {"value": "rct", "quote": "a  randomised\ntrial", "page": 2},
  "n": {"value": "120", "quote": "", "page": "3"},
  "age": {"value": 41.5, "page": 0},
  "outcomes": {"value": ["Schmerz", "Kosten"]},
  "aim": {"value": null},
  "passage": "We enrolled adults.",
  "unknown": {"value": "x"}
}}
```"#;
        let read = parse(answer, &fields()).unwrap();
        assert_eq!(read["design"], Proposed { value: json!("RCT"), quote: "a randomised trial".into(), page: Some(2) });
        assert_eq!((read["n"].value.clone(), read["n"].page), (json!(120), Some(3)));
        assert_eq!((read["age"].value.clone(), read["age"].page), (json!("41.5"), None));
        assert_eq!(read["outcomes"].value, json!(["Schmerz"]));
        assert_eq!(read["passage"].value, json!("We enrolled adults."));
        assert!(!read.contains_key("aim") && !read.contains_key("unknown"));

        // Was nicht zum Feld passt, fällt weg; ganze Zahlen dürfen als 12.0 kommen.
        let odd = r#"<think>{"fields": {"n": {"value": 1}}}</think>
            {"design": {"value": "Fallserie"}, "n": {"value": 12.0}, "age": {"value": "etwa 40"}, "outcomes": {"value": []}}"#;
        let read = parse(odd, &fields()).unwrap();
        assert_eq!(read.keys().collect::<Vec<_>>(), ["n"]);
        assert_eq!(read["n"].value, json!(12));

        assert!(parse(r#"{"fields": {}}"#, &fields()).unwrap().is_empty());
        assert!(parse("Ich finde nichts.", &fields()).unwrap_err().contains("keine lesbaren Werte"));
        assert!(parse(r#"{"decision": "include"}"#, &fields()).is_err());
        let long = format!(r#"{{"fields": {{"aim": {{"value": "x", "quote": "{}"}}}}}}"#, "q".repeat(MAX_EVIDENCE + 9));
        assert_eq!(parse(&long, &fields()).unwrap()["aim"].quote.chars().count(), MAX_EVIDENCE);
    }
}
