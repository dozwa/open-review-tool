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

/// Ein Text aus einer Registrierung bei OSF, einem Feld des Formulars zugeordnet.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImportedAnswer {
    pub key: String,
    pub text: String,
}

/// Ein Text aus einer Registrierung bei OSF, zu dem das Formular kein Feld hat.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UnmatchedAnswer {
    /// Name des Feldes bei OSF.
    pub label: String,
    pub text: String,
}

/// Eine bestehende Registrierung bei OSF, auf die Felder des Formulars verteilt. Nichts davon steht im
/// Log, bis die Leitung die Texte übernimmt (`Review::import_registration`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OsfImport {
    /// Kurze Adresse, etwa `osf.io/abc12`.
    pub identifier: String,
    pub url: String,
    pub title: String,
    /// Name des Formulars, mit dem bei OSF registriert wurde.
    pub schema: String,
    /// Tag der Registrierung, `JJJJ-MM-TT`.
    pub registered_on: String,
    /// Texte in der Reihenfolge des Formulars.
    pub answers: Vec<ImportedAnswer>,
    pub unmatched: Vec<UnmatchedAnswer>,
    /// Namen der Dateien, die an der Registrierung hängen. Die App lädt sie nicht.
    pub files: Vec<String>,
}

/// Kleinbuchstaben und Ziffern, Wörter durch ein Leerzeichen getrennt: So lassen sich Feldnamen
/// vergleichen, auch wenn OSF Leerzeichen oder Schrägstriche anders setzt.
fn plain(text: &str) -> String {
    let lower = text.to_lowercase();
    lower.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Frühere Namen von Feldern bei OSF und der Name, den das Feld heute trägt.
const OSF_FORMER_NAMES: [(&str, &str); 1] = [("extractor blinding", "extractor masking")];

/// Verteilt eine Registrierung von OSF auf die Felder des Formulars. `registration` ist die Antwort von
/// `/v2/registrations/<id>/`, `schema` die des Formulars, `blocks` sind dessen Blöcke (`schema_blocks`,
/// alle Seiten hintereinander), `contributors` die genannten Mitwirkenden (`bibliographic_contributors`).
///
/// Zugeordnet wird über den Namen des Feldes, nicht über dessen Kennung: So passen auch ältere Fassungen
/// des Formulars. Heißen zwei Felder gleich, entscheidet die Seite, auf der sie stehen. Texte aus
/// anderen Formularen bleiben ohne Feld und werden nur gezeigt.
pub fn osf_import(registration: &Value, schema: &Value, blocks: &[Value], contributors: &[Value]) -> Result<OsfImport> {
    let form = osf_form();
    let data = &registration["data"];
    let attributes = &data["attributes"];
    let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_string();
    let id = data.get("id").and_then(Value::as_str).unwrap_or_default();
    let responses = attributes.get("registration_responses").and_then(Value::as_object);
    let (Some(responses), false) = (responses, id.is_empty()) else {
        return Err(invalid(tr!("Unter dieser Adresse liegt keine lesbare Registrierung.")));
    };
    if attributes.get("withdrawn").and_then(Value::as_bool) == Some(true) {
        return Err(invalid(tr!("Diese Registrierung ist zurückgezogen; OSF gibt ihre Texte nicht mehr heraus.")));
    }

    let mut answers: BTreeMap<String, String> = BTreeMap::new();
    let mut unmatched = Vec::new();
    let mut files = Vec::new();
    let mut place = |label: &str, page: &str, value: String, answers: &mut BTreeMap<String, String>| {
        if value.is_empty() {
            return;
        }
        let mut name = plain(label);
        if let Some((_, now)) = OSF_FORMER_NAMES.iter().find(|(former, _)| *former == name) {
            name = now.to_string();
        }
        let same: Vec<&Item> = form.items.iter().filter(|i| plain(&i.original) == name).collect();
        let on_page =
            |item: &&&Item| form.sections.iter().any(|s| s.key == item.section && plain(&s.original) == plain(page));
        let item = match same.len() {
            1 => same.first(),
            _ => same.iter().find(on_page),
        };
        match item.filter(|i| !answers.contains_key(&i.key) && value.chars().count() <= MAX_ANSWER) {
            Some(item) => drop(answers.insert(item.key.clone(), value)),
            None => unmatched.push(UnmatchedAnswer { label: label.trim().to_string(), text: value }),
        }
    };

    // Metadaten stehen bei OSF nicht im Formular, sondern an der Registrierung selbst.
    let title = text(attributes, "title");
    place("Title", "", title.clone(), &mut answers);
    let names: Vec<String> = contributors
        .iter()
        .map(|c| {
            let user = text(&c["embeds"]["users"]["data"]["attributes"], "full_name");
            if user.is_empty() {
                text(&c["attributes"], "unregistered_contributor")
            } else {
                user
            }
        })
        .filter(|name| !name.is_empty())
        .collect();
    place("Contributors", "", names.join("; "), &mut answers);
    let mut subjects: Vec<String> = Vec::new();
    for path in attributes.get("subjects").and_then(Value::as_array).into_iter().flatten() {
        let leaf = path.as_array().and_then(|p| p.last()).map(|s| text(s, "text")).unwrap_or_default();
        if !leaf.is_empty() && !subjects.contains(&leaf) {
            subjects.push(leaf);
        }
    }
    place("Subjects", "", subjects.join("; "), &mut answers);
    place("Description", "", text(attributes, "description"), &mut answers);

    // Die Blöcke des Formulars in ihrer Reihenfolge: Seite, Name des Feldes, dann das Feld selbst.
    let mut from_form = 0;
    let mut sorted: Vec<&Value> = blocks.iter().collect();
    sorted.sort_by_key(|b| b["attributes"]["index"].as_i64().unwrap_or(i64::MAX));
    let (mut page, mut label) = (String::new(), String::new());
    for block in sorted {
        let block = &block["attributes"];
        let shown = text(block, "display_text");
        match block.get("block_type").and_then(Value::as_str).unwrap_or_default() {
            "page-heading" => (page, label) = (shown.clone(), shown),
            "section-heading" | "subsection-heading" | "question-label" if !shown.is_empty() => label = shown,
            _ => {}
        }
        let Some(value) = block.get("registration_response_key").and_then(Value::as_str).and_then(|k| responses.get(k))
        else {
            continue;
        };
        match value {
            Value::String(s) => {
                from_form += usize::from(!s.trim().is_empty());
                place(&label, &page, s.trim().to_string(), &mut answers);
            }
            Value::Array(list) => {
                let mut chosen = Vec::new();
                for entry in list {
                    match entry {
                        Value::String(s) if !s.trim().is_empty() => chosen.push(s.trim().to_string()),
                        Value::Object(_) => files.extend(Some(text(entry, "file_name")).filter(|n| !n.is_empty())),
                        _ => {}
                    }
                }
                from_form += chosen.len();
                place(&label, &page, chosen.join("; "), &mut answers);
            }
            _ => {}
        }
    }
    if from_form == 0 {
        return Err(invalid(tr!("Diese Registrierung enthält keine Texte, die sich übernehmen lassen.")));
    }
    Ok(OsfImport {
        identifier: format!("osf.io/{id}"),
        url: format!("https://osf.io/{id}/"),
        title,
        schema: text(&schema["data"]["attributes"], "name"),
        registered_on: text(attributes, "date_registered").chars().take(10).collect(),
        answers: form
            .items
            .iter()
            .filter_map(|i| answers.remove(&i.key).map(|text| ImportedAnswer { key: i.key.clone(), text }))
            .collect(),
        unmatched,
        files,
    })
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

    /// Übernimmt Texte aus einer bestehenden Registrierung als eigene Texte, in einem Ereignis. Ein Feld,
    /// das schon einen eigenen Text hat, bleibt, außer mit `replace`. Leere Texte nehmen nichts zurück.
    pub fn import_registration(&self, texts: &BTreeMap<String, String>, replace: bool) -> Result<RegistrationDraft> {
        require(&self.state()?, &self.me().actor_id, &MANAGE_ROLES)?;
        let form = osf_form();
        let (current, _) = answers(&self.store().events_of_types(&["ProtocolDrafted"])?);
        let mut new = serde_json::Map::new();
        for (key, text) in texts {
            if !form.items.iter().any(|i| &i.key == key) {
                return Err(invalid(tr!("Dieses Feld kennt das Registrierungsformular nicht.")));
            }
            let text = text.trim();
            if text.chars().count() > MAX_ANSWER {
                return Err(invalid(tr!(
                    "Ein Feld fasst höchstens {} Zeichen. Längeres gehört in einen Anhang.",
                    MAX_ANSWER
                )));
            }
            let own = current.get(key).map(String::as_str);
            if !text.is_empty() && own != Some(text) && (replace || own.is_none()) {
                new.insert(key.clone(), Value::String(text.to_string()));
            }
        }
        if !new.is_empty() {
            self.append("ProtocolDrafted", None::<Uuid>, json!({"form": OSF_FORM, "answers": new}))?;
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
            // Heißt die Seite im Register so wie hier (englische Ausgabe), steht der Name nur einmal.
            match section.label == section.original {
                true => out.push_str(&format!("\n## {}\n", section.label)),
                false => out.push_str(&format!("\n## {} ({})\n", section.label, section.original)),
            }
            for item in draft.items.iter().filter(|i| i.item.section == section.key) {
                match item.item.label == item.item.original {
                    true => out.push_str(&format!("\n### {} {}\n\n", item.item.number, item.item.label)),
                    false => out.push_str(&format!(
                        "\n### {} {} · {}\n\n",
                        item.item.number, item.item.original, item.item.label
                    )),
                }
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

    /// Eine Registrierung, wie OSF sie herausgibt, auf das Nötige gekürzt.
    fn osf_answer() -> (Value, Value, Vec<Value>, Vec<Value>) {
        let registration = json!({"data": {"id": "abc12", "attributes": {
            "title": "Schlaf und Kopfschmerz: Protokoll eines Scoping Reviews",
            "description": "Kurzfassung",
            "date_registered": "2026-03-04T10:11:12.000000",
            "withdrawn": false,
            "subjects": [[{"text": "Medicine and Health Sciences"}, {"text": "Neurology"}]],
            "registration_responses": {
                "q-8": " Scoping Review nach JBI ", "q-86": "Zotero und Tabellen", "q-100": "Keine Verblindung",
                "q-108": "Tabellen im Projekt", "q-76": [{"file_name": "Protokoll.pdf"}], "q-98": [],
                "q-9": "", "q-200": "Nur bei OSF gefragt", "q-201": ["a", "b"],
            },
        }}});
        let schema = json!({"data": {"attributes": {"name": "Generalized Systematic Review Registration"}}});
        let block = |index: i64, kind: &str, shown: &str, key: Option<&str>| json!({"attributes": {"index": index, "block_type": kind, "display_text": shown, "registration_response_key": key}});
        let blocks = vec![
            // Die Reihenfolge der Antwort zählt nicht, nur `index`.
            block(8, "long-text-input", "", Some("q-8")),
            block(5, "page-heading", "Review Methods", None),
            block(7, "question-label", "Type of review ", None),
            block(9, "long-text-input", "", Some("q-9")),
            block(65, "page-heading", "Screening", None),
            block(73, "question-label", "Screener instructions", None),
            block(75, "question-label", "", None),
            block(76, "file-input", "", Some("q-76")),
            block(85, "question-label", "Data management and sharing ", None),
            block(86, "long-text-input", "", Some("q-86")),
            block(89, "page-heading", "Extraction", None),
            block(97, "question-label", "", None),
            block(98, "file-input", "", Some("q-98")),
            block(99, "question-label", "Extractor blinding", None),
            block(100, "long-text-input", "", Some("q-100")),
            block(107, "question-label", "Data management and sharing", None),
            block(108, "long-text-input", "", Some("q-108")),
            block(199, "question-label", "Preferred colour", None),
            block(200, "long-text-input", "", Some("q-200")),
            block(201, "multi-select-input", "", Some("q-201")),
        ];
        let contributors = vec![
            json!({"attributes": {}, "embeds": {"users": {"data": {"attributes": {"full_name": "Ada Beispiel"}}}}}),
            json!({"attributes": {"unregistered_contributor": "Ben Muster"}}),
        ];
        (registration, schema, blocks, contributors)
    }

    #[test]
    fn an_osf_registration_is_spread_over_the_fields_by_name_and_page() {
        let (registration, schema, blocks, contributors) = osf_answer();
        let found = osf_import(&registration, &schema, &blocks, &contributors).unwrap();
        assert_eq!((found.identifier.as_str(), found.url.as_str()), ("osf.io/abc12", "https://osf.io/abc12/"));
        assert_eq!(
            (found.registered_on.as_str(), found.schema.as_str()),
            ("2026-03-04", "Generalized Systematic Review Registration")
        );
        let pairs: Vec<(&str, &str)> = found.answers.iter().map(|a| (a.key.as_str(), a.text.as_str())).collect();
        assert_eq!(
            pairs,
            [
                ("title", "Schlaf und Kopfschmerz: Protokoll eines Scoping Reviews"),
                ("contributors", "Ada Beispiel; Ben Muster"),
                ("subjects", "Neurology"),
                ("review_type", "Scoping Review nach JBI"),
                ("screening_data", "Zotero und Tabellen"),
                ("extractor_blinding", "Keine Verblindung"),
                ("extraction_data", "Tabellen im Projekt"),
            ]
        );
        let rest: Vec<(&str, &str)> = found.unmatched.iter().map(|u| (u.label.as_str(), u.text.as_str())).collect();
        assert_eq!(
            rest,
            [("Description", "Kurzfassung"), ("Preferred colour", "Nur bei OSF gefragt"), ("Preferred colour", "a; b")]
        );
        assert_eq!(found.files, ["Protokoll.pdf"]);
    }

    #[test]
    fn unreadable_withdrawn_and_empty_registrations_are_refused() {
        let (registration, schema, blocks, contributors) = osf_answer();
        assert!(osf_import(&json!({"errors": [{"detail": "Not found."}]}), &schema, &blocks, &contributors).is_err());
        let mut withdrawn = registration.clone();
        withdrawn["data"]["attributes"]["withdrawn"] = json!(true);
        assert!(osf_import(&withdrawn, &schema, &blocks, &contributors).is_err());
        let mut empty = registration;
        empty["data"]["attributes"]["registration_responses"] = json!({});
        empty["data"]["attributes"]["description"] = json!("");
        assert!(osf_import(&empty, &schema, &blocks, &[]).is_err());
    }

    #[test]
    fn the_form_follows_the_language() {
        let label = |code: &str| i18n::with(code, osf_form).items[0].label.clone();
        assert_eq!(label("de"), "Titel");
        assert_eq!(label("en"), "Title");
        assert_eq!(osf_form().items[0].label, "Titel");
    }
}
