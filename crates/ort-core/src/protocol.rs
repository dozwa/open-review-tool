//! Protokoll, Abweichungen und Checkliste.
//!
//! `ProtocolRegistered` hält fest, wann und wo das Protokoll registriert wurde. Die Liste der
//! Abweichungen ist eine Projektion: alles, was danach als neue Version veröffentlicht wurde
//! (Kriterien, Suchen, Ablauf), dazu unabhängig von der Registrierung jede Formularänderung nach der
//! Bestätigung und jedes übersteuerte Gate. Die Checkliste zeigt je Punkt, ob das Review das Material
//! dafür schon hergibt; gespeichert wird davon nichts.

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use uuid::Uuid;

use crate::catalog;
use crate::error::{invalid, Result};
use crate::event::Event;
use crate::i18n;
use crate::projection::order_key;
use crate::service::{require, Review, MANAGE_ROLES};
use crate::tr;
use crate::workflow::Workflow;

/// Ereignisse, aus denen Registrierung und Abweichungen entstehen.
pub const PROTOCOL_TYPES: [&str; 7] = [
    "ProtocolRegistered",
    "CriteriaVersionPublished",
    "SearchVersionPublished",
    "FormVersionPublished",
    "FormConfirmed",
    "WorkflowVersionPublished",
    "GateOverridden",
];

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Registration {
    /// Tag der Registrierung als JJJJ-MM-TT.
    pub registered_on: String,
    /// Register oder Ort, z. B. OSF.
    pub registry: String,
    /// Kennung oder Adresse; darf leer sein.
    pub identifier: String,
    pub note: String,
    /// Zeitpunkt des ersten Eintrags; ab hier zählen neue Versionen als Abweichung.
    pub recorded_at: String,
    /// Die Angaben wurden später berichtigt. Der Zeitpunkt bleibt der des ersten Eintrags.
    pub corrected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Deviation {
    /// `criteria`, `search`, `form`, `workflow` oder `gate`.
    pub kind: &'static str,
    pub at: String,
    pub title: String,
    /// Begründung aus dem Ereignis.
    pub note: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Protocol {
    pub registration: Option<Registration>,
    pub deviations: Vec<Deviation>,
}

impl Protocol {
    /// Baut Registrierung und Abweichungen aus den Ereignissen in [`PROTOCOL_TYPES`].
    pub fn build(events: &[Event], workflow: &Workflow) -> Protocol {
        let mut sorted: Vec<&Event> = events.iter().collect();
        sorted.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
        let text = |e: &Event, key: &str| e.payload_str(key).unwrap_or_default().trim().to_string();
        let version = |e: &Event| e.payload.get("version").and_then(Value::as_i64).unwrap_or(0);
        let mut out = Protocol::default();
        let mut confirmed: BTreeSet<String> = BTreeSet::new();
        for e in sorted {
            let registered = out.registration.is_some();
            let (kind, title, note) = match e.event_type.as_str() {
                "ProtocolRegistered" => {
                    let first = out.registration.take();
                    out.registration = Some(Registration {
                        registered_on: text(e, "registered_on"),
                        registry: text(e, "registry"),
                        identifier: text(e, "identifier"),
                        note: text(e, "note"),
                        recorded_at: first.as_ref().map_or(e.occurred_at.clone(), |f| f.recorded_at.clone()),
                        corrected: first.is_some(),
                    });
                    continue;
                }
                "FormConfirmed" => {
                    confirmed.insert(text(e, "form_id"));
                    continue;
                }
                "CriteriaVersionPublished" if registered && version(e) > 1 => {
                    ("criteria", tr!("Kriterien geändert (Version {})", version(e)), text(e, "change_note"))
                }
                "SearchVersionPublished" if registered && version(e) > 1 => (
                    "search",
                    tr!("Suche in {} geändert oder wiederholt (Version {})", text(e, "source"), version(e)),
                    text(e, "change_note"),
                ),
                "WorkflowVersionPublished" if registered && version(e) > 1 => {
                    ("workflow", tr!("Ablauf geändert (Version {})", version(e)), text(e, "change_note"))
                }
                "FormVersionPublished" if confirmed.contains(&text(e, "form_id")) => (
                    "form",
                    tr!("Formular „{}“ nach der Bestätigung geändert (Version {})", text(e, "name"), version(e)),
                    text(e, "change_note"),
                ),
                "GateOverridden" => {
                    let key = text(e, "station");
                    let label = workflow.template.station(&key).map_or(key.clone(), |s| i18n::text(&s.label));
                    ("gate", tr!("„{}“ ohne erfülltes Gate freigegeben", label), text(e, "reason"))
                }
                _ => continue,
            };
            out.deviations.push(Deviation { kind, at: e.occurred_at.clone(), title, note });
        }
        out
    }
}

// ---------------------------------------------------------------- Checkliste

/// Was das Review für die Checkliste schon hergibt. Der Bericht füllt das aus dem Log.
#[derive(Clone, Debug, Default)]
pub struct Evidence {
    pub registered: bool,
    pub criteria_version: i64,
    pub n_searches: i64,
    pub selection_started: bool,
    pub has_levels: bool,
    pub form_published: bool,
    pub charting_started: bool,
    /// Ein Feld des Formulars nennt, wie es ausgewertet wird.
    pub analysis_planned: bool,
    pub n_charted: i64,
    pub has_choice_fields: bool,
    /// Limitationen und Abweichungen im Methodentext.
    pub n_limitations: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChecklistItem {
    pub number: i64,
    pub section: String,
    pub topic: String,
    /// `ready` (Material liegt vor), `open` (das Review kann es liefern, es fehlt aber noch),
    /// `manual` (schreibt das Team selbst) oder `optional`.
    pub status: &'static str,
    /// Wo das Material steht oder was noch zu tun ist.
    pub material: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Checklist {
    pub name: String,
    pub items: Vec<ChecklistItem>,
}

/// Name der Checkliste, die diese App-Version kennt.
pub const PRISMA_SCR: &str = "PRISMA-ScR";

/// Punkte der PRISMA-ScR in eigenen Worten: Nummer, Abschnitt, Thema. Abschnitt und Thema werden bei
/// der Ausgabe übersetzt; die Übersetzungen stehen im Katalog.
const PRISMA_SCR_ITEMS: [(i64, &str, &str); 22] = [
    (1, "Titel", "Bericht als Scoping Review kennzeichnen"),
    (2, "Zusammenfassung", "Strukturierte Zusammenfassung"),
    (3, "Einleitung", "Begründung des Reviews"),
    (4, "Einleitung", "Ziele und Fragestellung"),
    (5, "Methoden", "Protokoll und Registrierung"),
    (6, "Methoden", "Einschlusskriterien"),
    (7, "Methoden", "Informationsquellen mit Tag der letzten Suche"),
    (8, "Methoden", "Vollständige Suchstrategie mindestens einer Quelle"),
    (9, "Methoden", "Auswahl der Quellen"),
    (10, "Methoden", "Ablauf des Chartings"),
    (11, "Methoden", "Erhobene Angaben"),
    (12, "Methoden", "Kritische Bewertung der Quellen, falls gemacht"),
    (13, "Methoden", "Vorgehen bei der Zusammenfassung der Ergebnisse"),
    (14, "Ergebnisse", "Auswahl der Quellen mit Zahlen und Flussdiagramm"),
    (15, "Ergebnisse", "Merkmale der Quellen"),
    (16, "Ergebnisse", "Kritische Bewertung innerhalb der Quellen, falls gemacht"),
    (17, "Ergebnisse", "Ergebnisse je Quelle"),
    (18, "Ergebnisse", "Zusammenfassung der Ergebnisse"),
    (19, "Diskussion", "Zusammenfassung der Evidenz"),
    (20, "Diskussion", "Limitationen"),
    (21, "Diskussion", "Schlussfolgerungen"),
    (22, "Finanzierung", "Finanzierung des Reviews und der Quellen"),
];

/// Checkliste mit Stand je Punkt. `None`, wenn die Vorlage keine Checkliste nennt, die die App kennt.
pub fn checklist(workflow: &Workflow, have: &Evidence) -> Option<Checklist> {
    let name = workflow.template.report.get("checklist").and_then(Value::as_str)?;
    if name != PRISMA_SCR {
        return None;
    }
    let extraction = i18n::text(workflow.template.term("extraction"));
    let sources = i18n::text(workflow.template.term("sources"));
    let state = |ready: bool, found: String, missing: String| match ready {
        true => ("ready", found),
        false => ("open", missing),
    };
    let own = |hint: String| ("manual", hint);
    let items = PRISMA_SCR_ITEMS
        .iter()
        .map(|(number, section, topic)| {
            let (status, material) = match number {
                4 => own(tr!("Schreibt ihr selbst. Die Elemente der Fragestellung stehen in den Kriterien.")),
                5 => state(
                    have.registered,
                    tr!("Registrierung und Abweichungen stehen im Bericht unter „Protokoll“."),
                    tr!("Registrierung im Bericht unter „Protokoll“ eintragen. Gibt es kein Protokoll, gehört das in den Text."),
                ),
                6 => state(
                    have.criteria_version > 0,
                    tr!("Reiter „Kriterien“, Version {}.", have.criteria_version),
                    tr!("Kriterien veröffentlichen."),
                ),
                7 => state(
                    have.n_searches > 0,
                    tr!("Suchanhang: Quellen und Tag der Suche."),
                    tr!("Suchen im Reiter „Suche“ festhalten."),
                ),
                8 => state(
                    have.n_searches > 0,
                    tr!("Suchanhang: Suchstring, Grenzen und Treffer je Quelle."),
                    tr!("Suchen im Reiter „Suche“ festhalten."),
                ),
                9 => state(
                    have.selection_started,
                    tr!("Methodentext, Abschnitte zur Auswahl."),
                    tr!("Entsteht mit der ersten Stufe der Auswahl."),
                ),
                10 => state(
                    have.charting_started,
                    tr!("Methodentext, Abschnitt „{}“.", extraction),
                    tr!("Entsteht mit der ersten Stufe im {}.", extraction),
                ),
                11 => state(
                    have.form_published,
                    tr!("Die Felder des Formulars stehen im Methodentext."),
                    tr!("Formular veröffentlichen."),
                ),
                12 | 16 => (
                    "optional",
                    tr!("In dieser Vorlage nicht vorgesehen. Wer bewertet, beschreibt Vorgehen und Ergebnis im Text."),
                ),
                13 if have.analysis_planned => {
                    ("ready", tr!("Die Auswertung je Feld steht im Formular und im Methodentext."))
                }
                13 => own(tr!(
                    "Schreibt ihr selbst. Im Formular lässt sich je Feld festhalten, wie es ausgewertet wird."
                )),
                14 => state(
                    have.has_levels,
                    tr!("Flussdiagramm und ausgeschlossene Volltexte mit Grund."),
                    tr!("Entsteht mit der ersten Stufe der Auswahl."),
                ),
                15 | 17 => state(
                    have.n_charted > 0,
                    tr!("Tabelle der eingeschlossenen {}.", sources),
                    tr!("Entsteht, sobald {} extrahiert und abgeglichen sind.", sources),
                ),
                18 if have.has_choice_fields && have.n_charted > 0 => (
                    "ready",
                    tr!("Häufigkeiten je Auswahlfeld stehen in der Formular-Stufe. Den Text schreibt ihr selbst."),
                ),
                20 if have.n_limitations > 0 => (
                    "ready",
                    tr!("Methodentext, Abschnitt „Abweichungen und Limitationen“. Das betrifft das Vorgehen; die Grenzen der Evidenz schreibt ihr selbst."),
                ),
                _ => own(tr!("Schreibt ihr selbst.")),
            };
            ChecklistItem { number: *number, section: i18n::text(section), topic: i18n::text(topic), status, material }
        })
        .collect();
    Some(Checklist { name: name.to_string(), items })
}

/// Checkliste als Markdown-Tabelle.
pub fn checklist_markdown(list: &Checklist) -> String {
    let mut out = tr!(
        "# Checkliste {}\n\n| Nr. | Abschnitt | Punkt | Stand | Material |\n| --- | --- | --- | --- | --- |\n",
        list.name
    );
    for item in &list.items {
        let status = match item.status {
            "ready" => tr!("liegt vor"),
            "open" => tr!("fehlt noch"),
            "optional" => tr!("freiwillig"),
            _ => tr!("selbst schreiben"),
        };
        out.push_str(&format!(
            "| {} | {} | {} | {status} | {} |\n",
            item.number, item.section, item.topic, item.material
        ));
    }
    out
}

impl Review {
    /// Registrierung und Abweichungen auf dem Stand des Logs.
    pub fn protocol(&self) -> Result<Protocol> {
        Ok(Protocol::build(&self.store().events_of_types(&PROTOCOL_TYPES)?, &self.workflow()?))
    }

    /// Hält die Registrierung des Protokolls fest. Der erste Eintrag legt fest, ab wann neue Versionen
    /// als Abweichung zählen; ein weiterer berichtigt nur die Angaben.
    pub fn register_protocol(
        &self,
        registered_on: &str,
        registry: &str,
        identifier: &str,
        note: &str,
    ) -> Result<Protocol> {
        require(&self.state()?, &self.me().actor_id, &MANAGE_ROLES)?;
        if !catalog::is_date(registered_on) {
            return Err(invalid(tr!("Das Datum der Registrierung fehlt oder hat nicht die Form JJJJ-MM-TT.")));
        }
        if registry.trim().is_empty() {
            return Err(invalid(tr!("Wo ist das Protokoll registriert oder abgelegt? Die Angabe fehlt.")));
        }
        self.append(
            "ProtocolRegistered",
            None::<Uuid>,
            json!({
                "registered_on": registered_on, "registry": registry.trim(), "identifier": identifier.trim(),
                "note": note.trim(),
            }),
        )?;
        self.protocol()
    }
}
