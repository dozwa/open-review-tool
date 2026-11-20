//! KI-Vorschlag im Screening: der Prompt aus den Daten des Reviews und das Lesen der Antwort.
//!
//! Der Kern spricht mit keiner API. Er baut die Anfrage ([`Review::ai_request`]), die App sendet sie, und
//! der Kern liest die Antwort ([`parse`]) und vermerkt den Vorschlag im Log. Ein Vorschlag ist keine
//! Stimme: Er steht in der Kette der Stufe, damit ihn während der Verblindung niemand sonst sieht, und
//! zählt in keiner Auswertung. Der Prompt selbst steht nicht im Log, nur seine Prüfsumme; seine Texte
//! dürfen deshalb der Sprache der Oberfläche folgen.

use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::catalog::MAX_RATIONALE;
use crate::criteria::Criteria;
use crate::error::{conflict, forbidden, invalid, not_found, Result};
use crate::event::sha256_hex;
use crate::projection::{ReviewState, SuggestionInfo};
use crate::registration;
use crate::service::{require, stage_criteria, RecordItem, Review, MANAGE_ROLES, WRITE_ROLES};
use crate::tr;

/// Platzhalter einer Vorlage, in der Reihenfolge der Standardvorlage.
pub const PLACEHOLDERS: [&str; 5] = ["review", "questions", "criteria", "record", "format"];

/// Was die Einstellungen zur Anfrage beitragen.
#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Vorlage mit Platzhaltern wie `{{criteria}}`; leer heißt Standardvorlage.
    pub template: String,
    /// Autoren, Jahr, Zeitschrift und DOI mitsenden.
    pub publication: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// Eine fertige Anfrage: die Nachrichten für die API und die Prüfsumme des Prompts fürs Log.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Request {
    pub messages: Vec<Message>,
    pub prompt_sha256: String,
}

impl Request {
    /// Der Prompt als ein Text, etwa zum Anzeigen.
    pub fn prompt(&self) -> String {
        self.messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n\n")
    }
}

/// Was aus der Antwort der KI gelesen wurde.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Answer {
    pub suggested: String,
    pub reason: Option<String>,
    pub rationale: String,
}

/// Die Teile, die an die Stelle der Platzhalter treten.
#[derive(Clone, Debug, Default)]
pub struct Blocks {
    pub review: String,
    pub questions: String,
    pub criteria: String,
    pub record: String,
    pub format: String,
}

/// Die Vorlage, die gilt, solange niemand eine eigene hinterlegt hat.
pub fn default_template() -> String {
    [
        tr!("Du unterstützt das Screening für ein Review. Beurteile, ob der folgende Eintrag nach den Kriterien eingeschlossen oder ausgeschlossen werden soll. Stütze dich nur auf die Angaben zum Eintrag. Reichen sie für eine Entscheidung nicht aus, antworte mit „maybe“."),
        format!("## {}\n{{{{review}}}}", tr!("Review")),
        format!("## {}\n{{{{questions}}}}", tr!("Fragestellung")),
        format!("## {}\n{{{{criteria}}}}", tr!("Kriterien")),
        format!("## {}\n{{{{record}}}}", tr!("Eintrag")),
        format!("## {}\n{{{{format}}}}", tr!("Antwort")),
    ]
    .join("\n\n")
}

/// Setzt die Teile in die Vorlage ein. Fehlen in einer eigenen Vorlage der Eintrag oder das
/// Antwortformat, kommen sie ans Ende: Ohne sie gäbe es nichts zu beurteilen und nichts zu lesen.
pub fn render(template: &str, blocks: &Blocks) -> String {
    let template = if template.trim().is_empty() { default_template() } else { template.trim().to_string() };
    let parts = [
        ("review", &blocks.review),
        ("questions", &blocks.questions),
        ("criteria", &blocks.criteria),
        ("record", &blocks.record),
        ("format", &blocks.format),
    ];
    let mut out = template.clone();
    for (name, text) in parts {
        out = out.replace(&format!("{{{{{name}}}}}"), text);
    }
    for (name, text) in [("record", &blocks.record), ("format", &blocks.format)] {
        if !template.contains(&format!("{{{{{name}}}}}")) {
            out.push_str("\n\n");
            out.push_str(text);
        }
    }
    out
}

fn line(label: String, value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| format!("{label}: {value}"))
}

/// Kriterien der Stufe als Text: erst Einschluss, dann Ausschluss, jeweils mit Kürzel und Element.
pub fn criteria_block(criteria: &Criteria, element: impl Fn(&str) -> String) -> String {
    let mut out = Vec::new();
    for (kind, title) in [("include", tr!("Einschlusskriterien")), ("exclude", tr!("Ausschlusskriterien"))] {
        let items: Vec<String> = criteria
            .items
            .iter()
            .filter(|i| i.kind == kind)
            .map(|i| {
                let mut text = String::from("- ");
                if !i.code.is_empty() {
                    text.push_str(&i.code);
                    text.push(' ');
                }
                let label = element(&i.element);
                if !label.is_empty() {
                    text.push_str(&format!("({label}) "));
                }
                text.push_str(i.text.trim());
                if !i.note.trim().is_empty() {
                    text.push_str(&format!(" [{}]", tr!("Erläuterung: {}", i.note.trim())));
                }
                text
            })
            .collect();
        if !items.is_empty() {
            out.push(format!("{title}:\n{}", items.join("\n")));
        }
    }
    if !criteria.guidance.trim().is_empty() {
        out.push(format!("{}:\n{}", tr!("Hinweise zur Anwendung"), criteria.guidance.trim()));
    }
    if out.is_empty() {
        return tr!("Es sind keine Kriterien hinterlegt.");
    }
    out.join("\n\n")
}

/// Der Eintrag als Text. Publikationsangaben nur auf Wunsch, damit Autoren und Zeitschrift das Urteil
/// nicht färben.
pub fn record_block(record: &RecordItem, publication: bool) -> String {
    let mut out = Vec::new();
    out.extend(line(tr!("Titel"), &record.title));
    out.push(match record.abstract_text.trim() {
        "" => tr!("Abstract: nicht vorhanden"),
        text => format!("{}: {text}", tr!("Abstract")),
    });
    out.extend(line(tr!("Schlagwörter"), &record.keywords));
    if publication {
        out.extend(line(tr!("Autoren"), &record.authors));
        out.extend(line(tr!("Jahr"), &record.year.map(|y| y.to_string()).unwrap_or_default()));
        out.extend(line(tr!("Zeitschrift"), &record.journal));
        out.extend(line(tr!("DOI"), &record.doi));
    }
    out.join("\n")
}

/// Das verlangte Antwortformat. Die Feldnamen sind fest, weil [`parse`] sie liest.
pub fn format_block(criteria: &Criteria) -> String {
    let codes = criteria.reason_codes();
    let mut out = vec![
        tr!("Antworte nur mit einem JSON-Objekt in dieser Form, ohne weiteren Text:"),
        r#"{"decision": "include" | "exclude" | "maybe", "reason": "<code>" | null, "rationale": "<text>"}"#
            .to_string(),
    ];
    if codes.is_empty() {
        out.push(tr!("Das Feld „reason“ ist immer null."));
    } else {
        out.push(tr!(
            "Bei „exclude“ steht in „reason“ genau eines dieser Kürzel: {}. Sonst ist „reason“ null.",
            codes.join(", ")
        ));
    }
    out.push(tr!("Das Feld „rationale“ begründet die Entscheidung in zwei bis vier Sätzen auf Deutsch."));
    out.join("\n")
}

/// Liest die Antwort der KI. Verträgt Text um das JSON herum, Code-Zäune und Denkblöcke. Ein Grund,
/// den die Kriterien nicht kennen, fällt weg; ohne lesbare Entscheidung gibt es einen Fehler.
pub fn parse(text: &str, criteria: &Criteria) -> std::result::Result<Answer, String> {
    let thinking = Regex::new(r"(?s)<think>.*?</think>").expect("fester Ausdruck");
    let text = thinking.replace_all(text, "");
    let object = text.char_indices().filter(|(_, c)| *c == '{').find_map(|(start, _)| {
        let value = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>().next()?.ok()?;
        value.get("decision").is_some().then_some(value)
    });
    let Some(object) = object else {
        return Err(tr!("Die Antwort der KI enthält keine lesbare Entscheidung."));
    };
    let suggested = object["decision"].as_str().unwrap_or_default().trim().to_lowercase();
    if !["include", "exclude", "maybe"].contains(&suggested.as_str()) {
        return Err(tr!("Die Antwort der KI enthält keine lesbare Entscheidung."));
    }
    let named = object.get("reason").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    let reason = (suggested == "exclude")
        .then(|| criteria.reason_codes().into_iter().find(|code| code.eq_ignore_ascii_case(named)))
        .flatten()
        .map(str::to_string);
    let rationale: String = object
        .get("rationale")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .chars()
        .take(MAX_RATIONALE)
        .collect();
    Ok(Answer { suggested, reason, rationale })
}

/// KI-Vorschläge gibt es nur, solange die Leitung sie im Review nicht abgeschaltet hat.
fn require_ai(state: &ReviewState) -> Result<()> {
    if !state.ai_allowed() {
        return Err(forbidden(tr!("Die Leitung hat KI-Vorschläge für dieses Review abgeschaltet.")));
    }
    Ok(())
}

impl Review {
    /// Erlaubt KI-Vorschläge im Review oder schaltet sie ab (nur Leitung). Gilt für alle, sobald das
    /// Ereignis bei ihnen angekommen ist; schon vermerkte Vorschläge bleiben im Log.
    pub fn set_ai_policy(&self, allowed: bool) -> Result<()> {
        let state = self.state()?;
        require(&state, &self.me().actor_id, &MANAGE_ROLES)?;
        if state.ai_allowed() != allowed {
            self.append("AiPolicySet", None, json!({ "allowed": allowed }))?;
        }
        Ok(())
    }

    /// Baut die Anfrage für einen Eintrag einer Screening-Stufe. Schreibt nichts.
    pub fn ai_request(&self, stage_id: &Uuid, record_id: &str, options: &Options) -> Result<Request> {
        let state = self.state()?;
        require(&state, &self.me().actor_id, &WRITE_ROLES)?;
        require_ai(&state)?;
        let stage = self.stage_state(stage_id)?;
        if stage.config.form_id.is_some() {
            return Err(invalid(tr!("Diese Stufe arbeitet mit einem Formular, nicht mit Stimmen.")));
        }
        if stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        let record = self.require_assigned(stage_id, record_id)?.to_string();
        let record = self.record_item(&record)?;
        let criteria = stage_criteria(&state, &stage);
        let workflow = self.workflow()?;
        let template = workflow.template.localized();

        let mut review = Vec::new();
        review.extend(line(tr!("Titel"), &state.title));
        review.extend(line(tr!("Art des Reviews"), &template.review_type));
        review.extend(line(tr!("Stufe"), &stage.config.name));
        if workflow.station_of(&stage).is_some_and(|s| s.requires_fulltext) {
            review.push(tr!("Zur Beurteilung liegen nur Titel und Abstract vor, nicht der Volltext."));
        }

        let (answers, _) = registration::answers(&self.store().events_of_types(&["ProtocolDrafted"])?);
        let questions: Vec<&str> = ["primary_questions", "secondary_questions"]
            .iter()
            .filter_map(|k| answers.get(*k))
            .map(String::as_str)
            .collect();
        let questions = if questions.is_empty() { tr!("Nicht hinterlegt.") } else { questions.join("\n") };

        let element = |key: &str| {
            template.criteria_schema.iter().find(|e| e.key == key).map(|e| e.label.clone()).unwrap_or_default()
        };
        let blocks = Blocks {
            review: review.join("\n"),
            questions,
            criteria: criteria_block(&criteria, element),
            record: record_block(&record, options.publication),
            format: format_block(&criteria),
        };
        let content = render(&options.template, &blocks);
        let prompt_sha256 = sha256_hex(content.as_bytes());
        Ok(Request { messages: vec![Message { role: "user".into(), content }], prompt_sha256 })
    }

    /// Liest die Antwort der KI gegen die Kriterien der Stufe und vermerkt den Vorschlag im Log.
    pub fn record_ai_suggestion(
        &self,
        stage_id: &Uuid,
        record_id: &str,
        model: &str,
        prompt_sha256: &str,
        answer: &str,
    ) -> Result<SuggestionInfo> {
        let state = self.state()?;
        require(&state, &self.me().actor_id, &WRITE_ROLES)?;
        require_ai(&state)?;
        let stage = self.stage_state(stage_id)?;
        if stage.config.form_id.is_some() {
            return Err(invalid(tr!("Diese Stufe arbeitet mit einem Formular, nicht mit Stimmen.")));
        }
        if stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        let record = self.require_assigned(stage_id, record_id)?.to_string();
        let answer = parse(answer, &stage_criteria(&state, &stage)).map_err(invalid)?;
        let mut payload = json!({
            "record_id": record,
            "model": model,
            "prompt_sha256": prompt_sha256,
            "suggested": answer.suggested,
            "rationale": answer.rationale,
        });
        if let Some(code) = &answer.reason {
            payload["reason"] = json!(code);
        }
        let event = self.append("AiSuggestionReceived", Some(*stage_id), payload)?;
        Ok(SuggestionInfo {
            id: event.event_id,
            model: model.to_string(),
            suggested: answer.suggested,
            reason: answer.reason,
            rationale: answer.rationale,
            outcome: None,
            at: event.occurred_at,
        })
    }

    /// Vermerkt, ob der eigene Vorschlag zu einem Eintrag übernommen (`accepted`) oder abgelehnt
    /// (`declined`) wurde. Die Stimme selbst gibt die Person danach wie sonst ab.
    pub fn resolve_ai_suggestion(
        &self,
        stage_id: &Uuid,
        record_id: &str,
        suggestion: &Uuid,
        outcome: &str,
    ) -> Result<()> {
        let state = self.state()?;
        require(&state, &self.me().actor_id, &WRITE_ROLES)?;
        let stage = self.stage_state(stage_id)?;
        if stage.closed {
            return Err(conflict(tr!("Die Stufe ist geschlossen.")));
        }
        let record = self.require_assigned(stage_id, record_id)?.to_string();
        match stage.suggestions.get(&(record.clone(), self.me().actor_id)) {
            Some(s) if s.id == *suggestion && s.outcome.is_none() => {}
            Some(s) if s.id == *suggestion => {
                return Err(conflict(tr!("Zu diesem KI-Vorschlag ist schon etwas vermerkt.")))
            }
            _ => return Err(not_found(tr!("KI-Vorschlag nicht gefunden."))),
        }
        if !["accepted", "declined"].contains(&outcome) {
            return Err(invalid(tr!("Ein KI-Vorschlag wird übernommen oder abgelehnt.")));
        }
        self.append(
            "AiSuggestionResolved",
            Some(*stage_id),
            json!({"record_id": record, "suggestion": suggestion.to_string(), "outcome": outcome}),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::criteria::Criterion;
    use crate::i18n;

    fn criteria() -> Criteria {
        let item = |code: &str, element: &str, kind: &str, text: &str, note: &str| Criterion {
            code: code.into(),
            element: element.into(),
            kind: kind.into(),
            text: text.into(),
            note: note.into(),
        };
        Criteria {
            version: 1,
            items: vec![
                item("I1", "population", "include", "Erwachsene mit Kopfschmerz", ""),
                item("E1", "population", "exclude", "Kinder", "unter 18 Jahren"),
                item("E2", "", "exclude", "Tierstudie", ""),
            ],
            guidance: "Im Zweifel einschließen.".into(),
            ..Criteria::default()
        }
    }

    fn record() -> RecordItem {
        RecordItem {
            id: Uuid::new_v4(),
            title: "Migräne bei Erwachsenen".into(),
            abstract_text: "Eine Kohortenstudie.".into(),
            authors: "Muster, A.".into(),
            year: Some(2021),
            journal: "Kopfschmerz Journal".into(),
            doi: "10.1/x".into(),
            url: String::new(),
            keywords: "Migräne".into(),
            duplicate_of: None,
        }
    }

    #[test]
    fn blocks_carry_criteria_codes_and_record() {
        let block =
            criteria_block(&criteria(), |key| if key == "population" { "Population".into() } else { String::new() });
        assert!(block.contains("- I1 (Population) Erwachsene mit Kopfschmerz"));
        assert!(block.contains("- E1 (Population) Kinder [Erläuterung: unter 18 Jahren]"));
        assert!(block.contains("- E2 Tierstudie"));
        assert!(block.contains("Im Zweifel einschließen."));
        assert!(format_block(&criteria()).contains("E1, E2"));
        assert!(format_block(&Criteria::default()).contains("immer null"));

        let plain = record_block(&record(), false);
        assert!(plain.contains("Titel: Migräne bei Erwachsenen") && plain.contains("Schlagwörter: Migräne"));
        assert!(!plain.contains("Muster") && !plain.contains("2021") && !plain.contains("Kopfschmerz Journal"));
        let full = record_block(&record(), true);
        assert!(full.contains("Autoren: Muster, A.") && full.contains("Jahr: 2021") && full.contains("DOI: 10.1/x"));
        let mut bare = record();
        bare.abstract_text.clear();
        assert!(record_block(&bare, false).contains("Abstract: nicht vorhanden"));
    }

    #[test]
    fn templates_fill_placeholders_and_keep_record_and_format() {
        let blocks = Blocks {
            review: "R".into(),
            questions: "Q".into(),
            criteria: "C".into(),
            record: "EINTRAG".into(),
            format: "FORMAT".into(),
        };
        let standard = render("", &blocks);
        assert!(!standard.contains("{{"));
        for part in
            ["## Review\nR", "## Fragestellung\nQ", "## Kriterien\nC", "## Eintrag\nEINTRAG", "## Antwort\nFORMAT"]
        {
            assert!(standard.contains(part), "{part}");
        }
        for name in PLACEHOLDERS {
            assert!(default_template().contains(&format!("{{{{{name}}}}}")), "{name}");
        }
        assert_eq!(render("Prüfe {{record}} nach {{criteria}}. {{format}}", &blocks), "Prüfe EINTRAG nach C. FORMAT");
        assert_eq!(render("Nur {{criteria}}", &blocks), "Nur C\n\nEINTRAG\n\nFORMAT");
        let english = i18n::with("en", || render("", &blocks));
        assert!(english.contains("## Research question\nQ"));
    }

    #[test]
    fn answers_are_read_tolerantly() {
        let c = criteria();
        let plain = parse(r#"{"decision": "exclude", "reason": "E1", "rationale": "Kinder."}"#, &c).unwrap();
        assert_eq!(
            plain,
            Answer { suggested: "exclude".into(), reason: Some("E1".into()), rationale: "Kinder.".into() }
        );

        let fenced =
            "Gern:\n```json\n{\"decision\": \"Include\", \"reason\": null, \"rationale\": \"Passt.\"}\n```\nFertig.";
        assert_eq!(parse(fenced, &c).unwrap().suggested, "include");

        let thinking =
            "<think>Vielleicht {\"decision\": \"exclude\"}?</think>{\"decision\": \"maybe\", \"rationale\": \"\"}";
        assert_eq!(parse(thinking, &c).unwrap().suggested, "maybe");

        // Ein unbekannter Grund fällt weg, ein Grund ohne Ausschluss auch; Groß- und Kleinschreibung zählt nicht.
        assert_eq!(parse(r#"{"decision": "exclude", "reason": "X9"}"#, &c).unwrap().reason, None);
        assert_eq!(parse(r#"{"decision": "include", "reason": "E1"}"#, &c).unwrap().reason, None);
        assert_eq!(parse(r#"{"decision": "exclude", "reason": "e2"}"#, &c).unwrap().reason.as_deref(), Some("E2"));

        let long = format!(r#"{{"decision": "maybe", "rationale": "{}"}}"#, "ä".repeat(MAX_RATIONALE + 50));
        assert_eq!(parse(&long, &c).unwrap().rationale.chars().count(), MAX_RATIONALE);

        assert!(parse("Ich würde einschließen.", &c).unwrap_err().contains("keine lesbare Entscheidung"));
        assert!(parse(r#"{"decision": "ja"}"#, &c).is_err());
        assert!(parse(r#"{"other": 1} {"decision": "include"}"#, &c).is_ok());
    }
}
