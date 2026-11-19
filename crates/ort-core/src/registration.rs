//! Registrierung vorbereiten.
//!
//! Die App führt durch die Felder eines Registrierungsformulars (mitgeliefert: das „Generalized
//! Systematic Review Registration“-Formular von OSF) und schlägt zu jedem Feld vor, was das Review
//! schon hergibt: Titel, Team, Stationen und Regeln der Vorlage, Kriterien, Suchen und Formularfelder.
//! Vorschläge sind eine Projektion und stehen nicht im Log. Was die Leitung selbst schreibt, hält
//! `ProtocolDrafted` fest; je Feld gilt der letzte Eintrag, ein leerer Text nimmt ihn zurück. Die
//! Registrierung selbst geschieht im Register; danach trägt die Leitung sie mit `ProtocolRegistered` ein.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::error::{invalid, Result};
use crate::event::Event;
use crate::forms::Field;
use crate::guidance;
use crate::i18n;
use crate::projection::order_key;
use crate::protocol::Registration;
use crate::report::percent;
use crate::search::{Searches, SEARCH_TYPES};
use crate::service::{require, Review, MANAGE_ROLES, ROLES};
use crate::tr;
use crate::workflow::{Rules, Station, Template};

/// Mitgeliefertes Formular.
pub const OSF_FORM: &str = "osf_gsrrf";
pub const FORMS: [&str; 1] = [OSF_FORM];
/// Längster Text eines Feldes in Zeichen.
pub const MAX_ANSWER: usize = 20_000;
/// Fassungen des OSF-Formulars je Sprache. Die erste ist die deutsche; sie gilt auch für Sprachen ohne
/// eigene Fassung. Eine weitere Sprache braucht ihre Datei `registration/<code>/osf_gsrrf.json` und
/// eine Zeile hier.
const OSF_JSON: [(&str, &str); 2] = [
    ("de", include_str!("../registration/de/osf_gsrrf.json")),
    ("en", include_str!("../registration/en/osf_gsrrf.json")),
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub key: String,
    pub label: String,
    /// Name der Seite im Register.
    pub original: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub number: i64,
    pub key: String,
    pub section: String,
    pub label: String,
    /// Name des Feldes im Register; dorthin wird der Text kopiert.
    pub original: String,
    pub hint: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegistrationForm {
    pub key: String,
    pub name: String,
    pub registry: String,
    pub citation: String,
    pub url: String,
    pub sections: Vec<Section>,
    pub items: Vec<Item>,
}

/// Das OSF-Formular in der geltenden Sprache.
pub fn osf_form() -> RegistrationForm {
    let language = i18n::current();
    let json = OSF_JSON.iter().find(|(code, _)| *code == language).unwrap_or(&OSF_JSON[0]).1;
    serde_json::from_str(json).expect("mitgeliefertes Registrierungsformular ist gültiges JSON")
}

#[derive(Clone, Debug, Serialize)]
pub struct DraftItem {
    #[serde(flatten)]
    pub item: Item,
    /// Was das Review zu diesem Feld schon hergibt; leer, wenn nichts.
    pub suggestion: String,
    /// Eigener Text der Leitung.
    pub answer: Option<String>,
    /// `answered` (eigener Text), `suggested` (nur Vorschlag) oder `open`.
    pub status: &'static str,
}

impl DraftItem {
    /// Der Text, der ins Register käme: der eigene, sonst der Vorschlag.
    pub fn text(&self) -> &str {
        self.answer.as_deref().unwrap_or(&self.suggestion)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RegistrationDraft {
    pub form: String,
    pub name: String,
    pub registry: String,
    pub citation: String,
    pub url: String,
    pub sections: Vec<Section>,
    pub items: Vec<DraftItem>,
    pub n_answered: i64,
    pub n_suggested: i64,
    pub n_open: i64,
    /// Zeitpunkt des letzten eigenen Eintrags.
    pub updated_at: Option<String>,
    /// Eingetragene Registrierung, falls es schon eine gibt.
    pub registration: Option<Registration>,
    /// Ob diese Person die Texte ändern darf.
    pub can_edit: bool,
}

/// Eigene Texte je Feld aus den `ProtocolDrafted`-Ereignissen; der letzte Eintrag gilt.
pub fn answers(events: &[Event]) -> (BTreeMap<String, String>, Option<String>) {
    let mut sorted: Vec<&Event> = events.iter().filter(|e| e.event_type == "ProtocolDrafted").collect();
    sorted.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
    let mut out = BTreeMap::new();
    let mut updated = None;
    for e in sorted {
        if e.payload_str("form") != Some(OSF_FORM) {
            continue;
        }
        for (key, value) in e.payload.get("answers").and_then(Value::as_object).into_iter().flatten() {
            match value.as_str().map(str::trim) {
                Some("") | None => drop(out.remove(key)),
                Some(text) => drop(out.insert(key.clone(), text.to_string())),
            }
        }
        updated = Some(e.occurred_at.clone());
    }
    (out, updated)
}

fn list(items: impl IntoIterator<Item = String>) -> String {
    items.into_iter().map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n")
}

/// Wie viele Personen bewerten oder extrahieren, in einem Satz.
fn rule_text(rules: &Rules, form: bool) -> String {
    let n = rules.min_reviewers.value.max(1);
    match (rules.decision_rule.value.as_str(), form) {
        ("single_verified", false) => tr!("eine Person bewertet jeden Eintrag, eine zweite prüft die Ausschlüsse"),
        ("single_verified", true) => tr!("eine Person extrahiert jede Quelle, eine zweite prüft die Angaben"),
        ("liberal", _) => tr!("eine Einschluss-Stimme genügt, ein Ausschluss braucht zwei gleiche Stimmen"),
        (_, false) if n > 1 => tr!("{} Personen bewerten jeden Eintrag unabhängig", n),
        (_, true) if n > 1 => tr!("{} Personen extrahieren jede Quelle unabhängig", n),
        (_, false) => tr!("eine Person bewertet jeden Eintrag"),
        (_, true) => tr!("eine Person extrahiert jede Quelle"),
    }
}

fn blind_text(rules: &Rules) -> String {
    match rules.blind.value && rules.decision_rule.value != "single_verified" {
        true => tr!("verblindet, bis die Leitung die Stufe entblindet"),
        false => tr!("nicht verblindet"),
    }
}

fn field_line(f: &Field) -> String {
    let mut line = f.label.clone();
    if !f.description.trim().is_empty() {
        line.push_str(&format!(": {}", f.description.trim()));
    }
    if !f.options.is_empty() {
        line.push_str(&format!(" ({})", f.options.join(", ")));
    }
    line
}

impl Review {
    /// Formular mit Vorschlägen und eigenen Texten auf dem Stand des Logs.
    pub fn registration_draft(&self) -> Result<RegistrationDraft> {
        let state = self.state()?;
        let role = require(&state, &self.me().actor_id, &ROLES)?;
        let can_edit = MANAGE_ROLES.contains(&role);
        let form = osf_form();
        let (answers, updated_at) = answers(&self.store().events_of_types(&["ProtocolDrafted"])?);
        let mut suggestions = self.registration_suggestions()?;
        let items: Vec<DraftItem> = form
            .items
            .into_iter()
            .map(|item| {
                let suggestion = suggestions.remove(item.key.as_str()).unwrap_or_default().trim().to_string();
                let answer = answers.get(&item.key).cloned();
                let status = match (&answer, suggestion.is_empty()) {
                    (Some(_), _) => "answered",
                    (None, false) => "suggested",
                    (None, true) => "open",
                };
                DraftItem { item, suggestion, answer, status }
            })
            .collect();
        let count = |status: &str| items.iter().filter(|i| i.status == status).count() as i64;
        Ok(RegistrationDraft {
            form: form.key,
            name: form.name,
            registry: form.registry,
            citation: form.citation,
            url: form.url,
            sections: form.sections,
            n_answered: count("answered"),
            n_suggested: count("suggested"),
            n_open: count("open"),
            items,
            updated_at,
            registration: self.protocol()?.registration,
            can_edit,
        })
    }

    /// Hält den eigenen Text zu einem Feld fest. Ein leerer Text nimmt ihn zurück; dann gilt wieder
    /// der Vorschlag.
    pub fn draft_registration(&self, key: &str, text: &str) -> Result<RegistrationDraft> {
        require(&self.state()?, &self.me().actor_id, &MANAGE_ROLES)?;
        if !osf_form().items.iter().any(|i| i.key == key) {
            return Err(invalid(tr!("Dieses Feld kennt das Registrierungsformular nicht.")));
        }
        let text = text.trim();
        if text.chars().count() > MAX_ANSWER {
            return Err(invalid(tr!(
                "Ein Feld fasst höchstens {} Zeichen. Längeres gehört in einen Anhang.",
                MAX_ANSWER
            )));
        }
        let (current, _) = answers(&self.store().events_of_types(&["ProtocolDrafted"])?);
        if current.get(key).map(String::as_str).unwrap_or_default() != text {
            self.append("ProtocolDrafted", None::<Uuid>, json!({"form": OSF_FORM, "answers": {key: text}}))?;
        }
        self.registration_draft()
    }

    /// Die Vorbereitung als Markdown: je Feld der Name im Register und der Text zum Kopieren.
    pub fn registration_markdown(&self) -> Result<String> {
        let draft = self.registration_draft()?;
        let mut out = tr!(
            "# Registrierung vorbereiten: {} ({})\n\nReview: {}\n\n",
            draft.name,
            draft.registry,
            self.state()?.title
        );
        out.push_str(&tr!(
            "Stand: {} von {} Feldern mit eigenem Text, {} mit einem Vorschlag aus dem Review, {} offen. Vorschläge sind als solche gekennzeichnet und vor dem Einreichen zu prüfen.\n\nFormular: {} {}\n",
            draft.n_answered,
            draft.items.len(),
            draft.n_suggested,
            draft.n_open,
            draft.citation,
            draft.url
        ));
        for section in &draft.sections {
            out.push_str(&format!("\n## {} ({})\n", section.label, section.original));
            for item in draft.items.iter().filter(|i| i.item.section == section.key) {
                out.push_str(&format!("\n### {} {} · {}\n\n", item.item.number, item.item.original, item.item.label));
                match item.status {
                    "answered" => out.push_str(&format!("{}\n", item.text())),
                    "suggested" => {
                        out.push_str(&tr!("*Vorschlag aus dem Review, noch nicht geprüft:*\n\n{}\n", item.text()))
                    }
                    _ => out.push_str(&tr!("*Noch offen.*\n")),
                }
            }
        }
        Ok(out)
    }

    /// Was das Review zu den Feldern schon hergibt. Felder ohne Eintrag bleiben offen.
    fn registration_suggestions(&self) -> Result<BTreeMap<&'static str, String>> {
        let detail = self.detail()?;
        let workflow = self.workflow()?;
        let template: &Template = &workflow.template;
        let published = workflow.published();
        let searches = Searches::build(&self.store().events_of_types(&SEARCH_TYPES)?);
        let latest: Vec<_> = searches.list.iter().filter_map(|s| s.latest()).collect();
        let forms = self.forms()?;
        let mut out: BTreeMap<&'static str, String> = BTreeMap::new();
        let mut put = |key: &'static str, text: String| {
            if !text.trim().is_empty() {
                out.insert(key, text);
            }
        };
        let stations = |kind: &str| -> Vec<&Station> { template.stations.iter().filter(|s| s.kind == kind).collect() };
        let per_station = |kind: &str, line: &dyn Fn(&Station, &Rules) -> String| -> String {
            match published {
                true => list(stations(kind).into_iter().map(|s| line(s, &template.rules_for(s)))),
                false => String::new(),
            }
        };

        // Metadaten und Review-Methoden
        put("title", detail.title.clone());
        let team: Vec<_> = detail.members.iter().filter(|m| m.role != "viewer").collect();
        put("contributors", team.iter().map(|m| m.name.clone()).collect::<Vec<_>>().join("; "));
        put(
            "tasks_roles",
            list(team.iter().map(|m| {
                let task = match m.role.as_str() {
                    "owner" | "admin" => tr!("Leitung: Protokoll, Kriterien, Suche, Öffnen und Schließen der Stufen"),
                    _ => tr!("Auswahl und Extraktion"),
                };
                format!("{}: {task}", m.name)
            })),
        );
        if published {
            let name = i18n::text(&template.name);
            let mut text = tr!("{} (Vorlage „{}“ im Open Review Tool).", name, name);
            if let Some(g) = guidance::guideline(&template.key) {
                let cited: Vec<&str> = g.sources.iter().map(|s| s.citation.as_str()).collect();
                text.push_str(&tr!(" Das Vorgehen folgt: {}", cited.join(" ")));
            }
            if let Some(name) = template.report.get("checklist").and_then(Value::as_str) {
                text.push_str(&tr!(" Berichtet wird nach {}.", name));
            }
            put("review_type", text);
            put("review_stages", template.stations.iter().map(|s| i18n::text(&s.label)).collect::<Vec<_>>().join("; "));
            let views = self.stations()?;
            let labels = |state: &str| -> Vec<String> {
                views.iter().filter(|s| s.state == state && s.blocking).map(|s| i18n::text(&s.label)).collect()
            };
            let (done, running) = (labels("done"), labels("running"));
            let mut stage = match done.is_empty() {
                true => tr!("Noch kein Schritt ist abgeschlossen."),
                false => tr!("Abgeschlossen: {}.", done.join("; ")),
            };
            if !running.is_empty() {
                stage.push_str(&tr!(" In Arbeit: {}.", running.join("; ")));
            }
            if detail.n_records > 0 {
                stage.push_str(&tr!(" Eingelesen sind {} Treffer.", detail.n_records));
            }
            put("current_stage", stage);
        }
        put(
            "software",
            tr!(
                "Open Review Tool {} für Import, Dubletten, Auswahl, Extraktion und Bericht.",
                env!("CARGO_PKG_VERSION")
            ),
        );

        // Suche
        let origin = |origins: &[&str]| -> Vec<&crate::search::SearchVersion> {
            latest.iter().copied().filter(|v| origins.contains(&v.origin.as_str())).collect()
        };
        let databases = origin(&["database", "register"]);
        put("databases", databases.iter().map(|v| v.source.clone()).collect::<Vec<_>>().join("; "));
        put(
            "interfaces",
            list(
                databases
                    .iter()
                    .filter(|v| !v.platform.trim().is_empty())
                    .map(|v| tr!("{} über {}", v.source, v.platform)),
            ),
        );
        put("grey_literature", origin(&["other"]).iter().map(|v| v.source.clone()).collect::<Vec<_>>().join("; "));
        if !origin(&["citation"]).is_empty() || template.stations.iter().any(|s| s.tool.as_deref() == Some("citation"))
        {
            put(
                "other_search",
                tr!(
                    "Zitationssuche vorwärts und rückwärts, ausgehend von den eingeschlossenen Quellen, bis nichts Neues mehr dazukommt. Neue Treffer durchlaufen dieselbe Auswahl."
                ),
            );
        }
        let criteria = &detail.criteria;
        let element = |key: &str| template.criteria_schema.iter().find(|e| e.key == key).map(|e| i18n::text(&e.label));
        let kind = |kind: &str| -> String {
            list(criteria.items.iter().filter(|c| c.kind == kind).map(|c| {
                let code = if c.code.is_empty() { String::new() } else { format!("{} ", c.code) };
                match element(&c.element) {
                    Some(label) => format!("{code}({label}): {}", c.text),
                    None => format!("{code}{}", c.text),
                }
            }))
        };
        let (include, exclude) = (kind("include"), kind("exclude"));
        if !include.is_empty() || !exclude.is_empty() {
            put("criteria", tr!("Einschluss:\n{}\n\nAusschluss:\n{}", include, exclude));
        }
        put(
            "query_strings",
            latest
                .iter()
                .filter(|v| v.origin != "citation")
                .map(|v| {
                    let platform =
                        if v.platform.trim().is_empty() { String::new() } else { format!(" ({})", v.platform) };
                    let limits =
                        if v.limits.trim().is_empty() { String::new() } else { tr!("\nGrenzen: {}", v.limits) };
                    tr!("{}{}, Version {}:\n{}{}", v.source, platform, v.version, v.query, limits)
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
        put(
            "search_validation",
            list(latest.iter().flat_map(|v| {
                v.peer_reviews.iter().map(move |p| match p.note.trim().is_empty() {
                    true => {
                        tr!("Die Suche in {} wurde am {} von einer zweiten Person geprüft", v.source, p.reviewed_on)
                    }
                    false => tr!(
                        "Die Suche in {} wurde am {} von einer zweiten Person geprüft: {}",
                        v.source,
                        p.reviewed_on,
                        p.note
                    ),
                })
            })),
        );
        put(
            "search_repetition",
            list(
                latest
                    .iter()
                    .filter(|v| !v.searched_on.is_empty())
                    .map(|v| tr!("{}: Stand {}", v.source, v.searched_on)),
            ),
        );

        // Screening. Namen und Beschreibungen der Stationen stammen aus der Vorlage im Log und werden
        // für die Ausgabe übersetzt, soweit der Katalog sie kennt.
        let named = |s: &Station, text: String| format!("{}: {text}", i18n::text(&s.label));
        put("screening_stages", per_station("screening", &|s, _| named(s, i18n::text(&s.description))));
        put("screening_blinding", per_station("screening", &|s, r| named(s, blind_text(r))));
        let codes: Vec<&str> = criteria.items.iter().filter(|c| c.kind == "exclude").map(|c| c.code.as_str()).collect();
        let required: Vec<String> =
            template.stations.iter().filter(|s| s.reason_required).map(|s| tr!("„{}“", i18n::text(&s.label))).collect();
        if !codes.is_empty() || !required.is_empty() {
            let mut text = match codes.is_empty() {
                true => tr!("Ausschlussgründe sind die Kürzel der Ausschlusskriterien."),
                false => tr!("Ausschlussgründe sind die Kürzel der Ausschlusskriterien ({}).", codes.join(", ")),
            };
            if !required.is_empty() {
                text.push_str(&tr!(
                    " In {} braucht jeder Ausschluss genau einen Hauptgrund.",
                    required.join(&tr!(" und "))
                ));
            }
            put("exclusion_criteria", text);
        }
        put("screener_instructions", criteria.guidance.clone());
        let reliability = per_station("screening", &|s, r| {
            let threshold = s.gate.iter().find(|g| g.check == "agreement_min").and_then(|g| g.percent);
            match threshold {
                Some(p) => {
                    tr!("{}: {}; weiter ab {} Übereinstimmung", i18n::text(&s.label), rule_text(r, false), percent(p))
                }
                None => named(s, rule_text(r, false)),
            }
        });
        if !reliability.is_empty() {
            put(
                "screening_reliability",
                tr!(
                    "{}\n\nBerichtet werden die Prozent-Übereinstimmung, Cohens Kappa je Paar mit PABAK, Fleiss’ Kappa und Krippendorffs Alpha.",
                    reliability
                ),
            );
        }
        if published {
            put(
                "screening_reconciliation",
                match template.rules.conflict_rule.value.as_str() {
                    "third_person" => {
                        tr!("Abweichungen entscheidet eine dritte Person, die den Eintrag nicht selbst bewertet hat.")
                    }
                    _ => tr!(
                        "Abweichungen klären die beteiligten Personen im Gespräch; das Ergebnis wird als Konsens festgehalten."
                    ),
                },
            );
        }
        put(
            "screening_sampling",
            per_station("screening", &|s, _| match s.input.as_ref().filter(|i| i.scope == "sample") {
                Some(input) => tr!(
                    "{}: Zufallsstichprobe von {} Einträgen; der Startwert der Ziehung wird festgehalten",
                    i18n::text(&s.label),
                    input.sample_size.unwrap_or(25)
                ),
                None => tr!("{}: alle Einträge der Stufe", i18n::text(&s.label)),
            }),
        );
        let stored = tr!(
            "Jede Entscheidung liegt als signiertes, unveränderliches Ereignis in der Review-Datei des Open Review Tool."
        );
        put("screening_data", stored.to_string());

        // Extraktion und Synthese
        let fields: Vec<Field> = forms
            .forms
            .iter()
            .find(|f| f.purpose == "charting")
            .map(|f| f.current().fields.clone())
            .or_else(|| forms.starters.iter().find(|s| s.purpose == "charting").map(|s| s.fields.clone()))
            .unwrap_or_default();
        put("entities", list(fields.iter().map(field_line)));
        put("extraction_stages", per_station("form", &|s, _| named(s, i18n::text(&s.description))));
        if !fields.is_empty() {
            put(
                "extractor_instructions",
                tr!("Das Formular nennt zu jedem Feld eine Beschreibung und, wo möglich, feste Auswahlwerte."),
            );
        }
        put("extractor_blinding", per_station("form", &|s, r| named(s, blind_text(r))));
        put("extraction_reliability", per_station("form", &|s, r| named(s, rule_text(r, true))));
        if published && !stations("form").is_empty() {
            put(
                "extraction_reconciliation",
                tr!("Abweichende Angaben werden je Feld geklärt; der geklärte Wert wird mit Begründung festgehalten."),
            );
            put("extraction_data", stored.to_string());
        }
        put(
            "synthesis_plan",
            list(
                fields.iter().filter(|f| !f.analysis.trim().is_empty()).map(|f| format!("{}: {}", f.label, f.analysis)),
            ),
        );
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jede Sprachfassung hat dieselben Felder wie die deutsche: Schlüssel, Nummern, Abschnitte und
    /// die Namen im Register. Nur Beschriftungen und Hinweise unterscheiden sich.
    #[test]
    fn every_language_version_has_the_same_fields() {
        type Frame = (Vec<String>, Vec<(String, String)>, Vec<(i64, String, String, String)>);
        let frame = |json: &str| -> (RegistrationForm, Frame) {
            let form: RegistrationForm = serde_json::from_str(json).unwrap();
            let head = vec![form.key.clone(), form.name.clone(), form.registry.clone(), form.url.clone()];
            let sections = form.sections.iter().map(|s| (s.key.clone(), s.original.clone())).collect();
            let items =
                form.items.iter().map(|i| (i.number, i.key.clone(), i.section.clone(), i.original.clone())).collect();
            (form, (head, sections, items))
        };
        let (source, json) = OSF_JSON[0];
        assert_eq!(source, i18n::SOURCE, "die erste Fassung ist die der Quelltextsprache");
        let (_, expected) = frame(json);
        assert_eq!(expected.2.len(), 65);
        for (code, json) in &OSF_JSON[1..] {
            assert!(i18n::is_known(code), "{code} ist keine Sprache der App");
            let (form, found) = frame(json);
            assert_eq!(found, expected, "{code}");
            assert!(form.sections.iter().all(|s| !s.label.trim().is_empty()), "{code}: Abschnitt ohne Namen");
            assert!(
                form.items.iter().all(|i| !i.label.trim().is_empty() && !i.hint.trim().is_empty()),
                "{code}: Feld ohne Beschriftung oder Hinweis"
            );
        }
    }

    #[test]
    fn the_form_follows_the_language() {
        let label = |code: &str| i18n::with(code, osf_form).items[0].label.clone();
        assert_eq!(label("de"), "Titel");
        assert_eq!(label("en"), "Title");
        assert_eq!(osf_form().items[0].label, "Titel");
    }
}
