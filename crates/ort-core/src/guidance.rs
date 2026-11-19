//! Leitlinien zum Nachschlagen.
//!
//! Zu jeder mitgelieferten Vorlage liegt eine Zusammenfassung ihrer Leitlinie in der App, in eigenen
//! Worten und mit Verweis auf die Originalquelle. Ein Eintrag nennt die Stationen und Bereiche, zu
//! denen er gehört; so zeigt die Oberfläche auf jedem Bildschirm die passende Empfehlung. Die Texte
//! sind fester Bestandteil der App-Version und stehen nicht im Log.
//!
//! Jede Sprache hat ihren Ordner `guidance/<code>/` mit denselben Dateien: gleiche Schlüssel, Quellen,
//! Abschnitte, Stationen und Bereiche, nur Titel und Texte in der jeweiligen Sprache. Eine weitere
//! Sprache braucht einen weiteren Ordner und einen Eintrag in `BUILTIN`.

use serde::{Deserialize, Serialize};

use crate::i18n;
use crate::tr;
use crate::workflow::Template;

/// Die Dateien einer Sprache, in der Reihenfolge der Vorlagen.
macro_rules! language {
    ($code:literal) => {
        (
            $code,
            [
                include_str!(concat!("../guidance/", $code, "/jbi_scoping.json")),
                include_str!(concat!("../guidance/", $code, "/rapid_review.json")),
                include_str!(concat!("../guidance/", $code, "/state_of_the_art.json")),
            ],
        )
    };
}

/// Leitlinien je Sprache. Die erste Fassung ist die deutsche; sie gilt auch für Sprachen ohne eigene.
const BUILTIN: [(&str, [&str; 3]); 2] = [language!("de"), language!("en")];

/// Bereiche der Oberfläche, denen ein Eintrag zugeordnet sein kann.
pub const AREAS: [&str; 9] =
    ["criteria", "registration", "search", "imports", "fulltext", "sources", "forms", "report", "members"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub key: String,
    pub citation: String,
    pub url: String,
    /// Lizenz des Originals, soweit bekannt.
    #[serde(default)]
    pub license: String,
    /// Tag des Abrufs als JJJJ-MM-TT.
    pub accessed: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub title: String,
    /// Schlüssel einer Quelle der Leitlinie.
    pub source: String,
    /// Abschnitt oder Nummer der Empfehlung im Original.
    #[serde(default)]
    pub section: String,
    /// Adresse des Abschnitts im Original.
    pub url: String,
    /// Absätze der Zusammenfassung.
    pub text: Vec<String>,
    /// Einzelne Punkte als Liste.
    #[serde(default)]
    pub points: Vec<String>,
    /// Stationen der Vorlage, bei denen der Eintrag erscheint.
    #[serde(default)]
    pub stations: Vec<String>,
    /// Bereiche aus [`AREAS`], in denen der Eintrag erscheint.
    #[serde(default)]
    pub areas: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Guideline {
    /// Schlüssel der Vorlage, zu der die Leitlinie gehört.
    pub key: String,
    pub title: String,
    pub summary: String,
    pub sources: Vec<Source>,
    pub entries: Vec<Entry>,
}

impl Guideline {
    /// Prüft die Leitlinie gegen ihre Vorlage: Quellen, Bereiche und Stationen müssen existieren, und
    /// jede Station mit Stufen braucht mindestens einen Eintrag.
    pub fn check(&self, template: &Template) -> std::result::Result<(), String> {
        if self.sources.is_empty() || self.entries.is_empty() {
            return Err(tr!("Leitlinie „{}“: Quellen oder Einträge fehlen.", self.key));
        }
        for source in &self.sources {
            if !source.url.starts_with("https://") || !crate::catalog::is_date(&source.accessed) {
                return Err(tr!("Quelle „{}“: Adresse oder Abrufdatum fehlen.", source.key));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for e in &self.entries {
            let name = &e.id;
            if !seen.insert(name) {
                return Err(tr!("Eintrag „{}“ steht doppelt in der Leitlinie.", name));
            }
            if e.title.trim().is_empty() || e.text.iter().all(|t| t.trim().is_empty()) {
                return Err(tr!("Eintrag „{}“: Titel oder Text fehlen.", name));
            }
            if !self.sources.iter().any(|s| s.key == e.source) || !e.url.starts_with("https://") {
                return Err(tr!("Eintrag „{}“: Quelle oder Adresse fehlen.", name));
            }
            if let Some(area) = e.areas.iter().find(|a| !AREAS.contains(&a.as_str())) {
                return Err(tr!("Eintrag „{}“: unbekannter Bereich „{}“.", name, area));
            }
            if let Some(station) = e.stations.iter().find(|s| template.station(s).is_none()) {
                return Err(tr!("Eintrag „{}“: Die Vorlage hat keine Station „{}“.", name, station));
            }
            if e.stations.is_empty() && e.areas.is_empty() {
                return Err(tr!("Eintrag „{}“ gehört zu keiner Station und keinem Bereich.", name));
            }
        }
        for station in template.stations.iter().filter(|s| matches!(s.kind.as_str(), "screening" | "form")) {
            if self.for_station(&station.key).is_empty() {
                return Err(tr!("Zur Station „{}“ fehlt ein Eintrag.", station.label));
            }
        }
        Ok(())
    }

    pub fn for_station(&self, station: &str) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.stations.iter().any(|s| s == station)).collect()
    }

    pub fn for_area(&self, area: &str) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.areas.iter().any(|a| a == area)).collect()
    }
}

/// Mitgelieferte Leitlinien in der geltenden Sprache, in der Reihenfolge der Vorlagen.
pub fn guidelines() -> Vec<Guideline> {
    guidelines_in(i18n::current())
}

/// Mitgelieferte Leitlinien in der Sprache `code`; ohne eigene Fassung die deutschen.
pub fn guidelines_in(code: &str) -> Vec<Guideline> {
    let (_, files) = BUILTIN.iter().find(|(language, _)| *language == code).unwrap_or(&BUILTIN[0]);
    files.iter().map(|text| serde_json::from_str(text).expect("mitgelieferte Leitlinie ist gültiges JSON")).collect()
}

/// Leitlinie zu einer Vorlage; der freie Ablauf hat keine.
pub fn guideline(template_key: &str) -> Option<Guideline> {
    guidelines().into_iter().find(|g| g.key == template_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::{builtin_template, builtin_templates, FREE_TEMPLATE};

    #[test]
    fn every_guideline_fits_its_template() {
        let all = guidelines();
        for g in &all {
            let template = builtin_template(&g.key).unwrap_or_else(|| panic!("keine Vorlage zu {}", g.key));
            assert_eq!(g.check(&template), Ok(()), "{}", g.key);
        }
        // Jede Vorlage außer dem freien Ablauf bringt ihre Leitlinie mit.
        for t in builtin_templates().iter().filter(|t| t.key != FREE_TEMPLATE) {
            assert!(all.iter().any(|g| g.key == t.key), "Leitlinie zu {} fehlt", t.key);
        }
        assert!(guideline(FREE_TEMPLATE).is_none());
    }

    /// Jede Sprache der App hat ihre Leitlinien, und alle Fassungen haben denselben Aufbau: Nur
    /// Titel, Zusammenfassung, Absätze und Punkte unterscheiden sich.
    #[test]
    fn language_versions_have_the_same_structure() {
        assert_eq!(BUILTIN[0].0, i18n::SOURCE);
        let german = guidelines_in(i18n::SOURCE);
        for language in &i18n::LANGUAGES[1..] {
            let code = language.code;
            assert!(BUILTIN.iter().any(|(c, _)| *c == code), "Leitlinien auf {code} fehlen");
            let other = i18n::with(code, guidelines);
            assert_eq!(other.len(), german.len(), "{code}");
            for (g, o) in german.iter().zip(&other) {
                let template = builtin_template(&o.key).unwrap();
                assert_eq!(o.check(&template), Ok(()), "{code}: {}", o.key);
                assert!(!o.title.trim().is_empty() && !o.summary.trim().is_empty(), "{code}: {}", o.key);
                assert_ne!(o.summary, g.summary, "{code}: {} ist nicht übersetzt", o.key);
                // Schlüssel, Quellen, Abschnitte, Adressen, Stationen und Bereiche sind gleich.
                let bare = |guideline: &Guideline| {
                    let mut bare = guideline.clone();
                    bare.title.clear();
                    bare.summary.clear();
                    // Eine Quellenangabe darf je Sprache anders lauten („Kapitel 10“, „chapter 10“).
                    bare.sources.iter_mut().for_each(|s| s.citation.clear());
                    for e in &mut bare.entries {
                        e.title.clear();
                        e.section.retain(|c| c.is_ascii_digit());
                        e.text.iter_mut().chain(&mut e.points).for_each(String::clear);
                    }
                    bare
                };
                assert_eq!(bare(o), bare(g), "{code}: {}", o.key);
                for e in &o.entries {
                    let filled = e.text.iter().chain(&e.points).all(|t| !t.trim().is_empty());
                    assert!(filled, "{code}: {} hat leere Absätze", e.id);
                }
            }
        }
    }

    #[test]
    fn entries_are_found_by_station_and_area() {
        let jbi = guideline("jbi_scoping").unwrap();
        let pilot: Vec<&str> = jbi.for_station("pilot_title_abstract").iter().map(|e| e.id.as_str()).collect();
        assert_eq!(pilot, ["pilot", "disagreement"]);
        assert!(jbi.for_area("search").iter().any(|e| e.section == "10.2.6"));
        assert!(jbi.for_area("exchange").is_empty());
    }

    #[test]
    fn broken_guidelines_are_refused() {
        let template = builtin_template("jbi_scoping").unwrap();
        let good = guideline("jbi_scoping").unwrap();
        let mut unknown_station = good.clone();
        unknown_station.entries[0].stations.push("tanz".into());
        assert!(unknown_station.check(&template).unwrap_err().contains("keine Station"));
        let mut unknown_area = good.clone();
        unknown_area.entries[0].areas.push("keller".into());
        assert!(unknown_area.check(&template).unwrap_err().contains("unbekannter Bereich"));
        let mut gap = good.clone();
        gap.entries.retain(|e| !e.stations.iter().any(|s| s == "charting_pilot"));
        assert!(gap.check(&template).unwrap_err().contains("fehlt ein Eintrag"));
        let mut no_link = good;
        no_link.entries[0].url = "siehe Handbuch".into();
        assert!(no_link.check(&template).unwrap_err().contains("Adresse"));
    }
}
