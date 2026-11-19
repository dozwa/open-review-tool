//! Kriterien eines Reviews: eine Liste mit Kürzel, Element des Schemas, Art, Text und Erläuterung.
//!
//! Ausschlussgründe sind die Kürzel der Ausschlusskriterien. Eine Stufe prüft Gründe gegen die
//! Kriterienversion, mit der sie geöffnet wurde. [`check_reasons`] gilt lokal und beim Einlesen.

use serde::Serialize;
use serde_json::Value;

use crate::projection::string_list;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Criterion {
    /// Kürzel wie `E1`. Leer nur bei Kriterien aus der Zeit vor den Kürzeln.
    pub code: String,
    /// Schlüssel aus dem Kriterienschema der Vorlage, z. B. `population`; leer = ohne Zuordnung.
    pub element: String,
    /// `include` oder `exclude`.
    pub kind: String,
    pub text: String,
    /// Erläuterung für das Team, z. B. Grenzfälle.
    pub note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Criteria {
    pub version: i64,
    pub previous_version: Option<i64>,
    pub items: Vec<Criterion>,
    /// Erläuterungsdokument: wie die Kriterien anzuwenden sind.
    pub guidance: String,
    pub include_keywords: Vec<String>,
    pub exclude_keywords: Vec<String>,
    pub change_note: String,
}

impl Criteria {
    /// Liest einen Payload in der aktuellen Form, also nach [`crate::catalog::upcast`].
    pub fn from_payload(payload: Option<&Value>) -> Criteria {
        let Some(p) = payload else { return Criteria::default() };
        let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        let items = match p.get("items") {
            Some(Value::Array(items)) => items
                .iter()
                .map(|i| Criterion {
                    code: text(i, "code"),
                    element: text(i, "element"),
                    kind: text(i, "kind"),
                    text: text(i, "text"),
                    note: text(i, "note"),
                })
                .collect(),
            _ => Vec::new(),
        };
        Criteria {
            version: p.get("version").and_then(Value::as_i64).unwrap_or(0),
            previous_version: p.get("previous_version").and_then(Value::as_i64),
            items,
            guidance: text(p, "guidance"),
            include_keywords: string_list(p.get("include_keywords")),
            exclude_keywords: string_list(p.get("exclude_keywords")),
            change_note: text(p, "change_note"),
        }
    }

    /// Kürzel, die als Ausschlussgrund gelten.
    pub fn reason_codes(&self) -> Vec<&str> {
        self.items.iter().filter(|i| i.kind == "exclude" && !i.code.is_empty()).map(|i| i.code.as_str()).collect()
    }
}

/// Ob ein Kürzel die erlaubte Form hat: ein bis acht Buchstaben oder Ziffern.
pub fn valid_code(code: &str) -> bool {
    (1..=8).contains(&code.chars().count()) && code.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Prüft die Gründe einer Stimme oder eines Konsenses gegen die Kriterien der Stufe. Verlangt die
/// Stufe einen Grund, braucht jeder Ausschluss genau einen Hauptgrund.
pub fn check_reasons(
    criteria: &Criteria,
    reason_required: bool,
    decision: &str,
    primary_reason: Option<&str>,
    reasons: &[String],
) -> std::result::Result<(), String> {
    let codes = criteria.reason_codes();
    let unknown = |code: &str| {
        format!("„{code}“ ist kein Ausschlusskriterium der Kriterienversion {} dieser Stufe.", criteria.version)
    };
    match primary_reason {
        Some(_) if decision != "exclude" => return Err("Ein Hauptgrund gehört nur zu einem Ausschluss.".into()),
        Some(code) if !codes.contains(&code) => return Err(unknown(code)),
        None if reason_required && decision == "exclude" => {
            return Err("In dieser Stufe braucht jeder Ausschluss genau einen Hauptgrund.".into());
        }
        _ => {}
    }
    match reasons.iter().find(|r| !codes.contains(&r.as_str())) {
        Some(code) => Err(unknown(code)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn criteria() -> Criteria {
        Criteria::from_payload(Some(&json!({
            "version": 3,
            "items": [
                {"code": "I1", "element": "population", "kind": "include", "text": "Erwachsene"},
                {"code": "E1", "element": "population", "kind": "exclude", "text": "Kinder"},
                {"code": "E2", "kind": "exclude", "text": "Kein Originalbeitrag", "note": "auch Editorials"},
            ],
        })))
    }

    #[test]
    fn reasons_are_the_codes_of_exclusion_criteria() {
        let c = criteria();
        assert_eq!(c.reason_codes(), ["E1", "E2"]);
        assert_eq!(c.items[2].note, "auch Editorials");
        assert_eq!(check_reasons(&c, false, "exclude", None, &["E2".into()]), Ok(()));
        assert!(check_reasons(&c, false, "exclude", None, &["I1".into()]).unwrap_err().contains("Kriterienversion 3"));
        assert!(check_reasons(&c, false, "exclude", Some("X9"), &[]).is_err());
        assert!(check_reasons(&c, false, "include", Some("E1"), &[]).unwrap_err().contains("nur zu einem Ausschluss"));
    }

    #[test]
    fn required_reason_applies_to_exclusions_only() {
        let c = criteria();
        assert!(check_reasons(&c, true, "exclude", None, &["E1".into()]).unwrap_err().contains("genau einen"));
        assert_eq!(check_reasons(&c, true, "exclude", Some("E1"), &["E2".into()]), Ok(()));
        assert_eq!(check_reasons(&c, true, "include", None, &[]), Ok(()));
        assert_eq!(check_reasons(&c, true, "maybe", None, &[]), Ok(()));
    }

    #[test]
    fn codes_are_short_and_alphanumeric() {
        assert!(valid_code("E1") && valid_code("Pop2"));
        assert!(!valid_code("") && !valid_code("E 1") && !valid_code("zulangeskuerzel"));
    }
}
