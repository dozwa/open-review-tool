//! Bericht, Teil 1: Zahlen des Flussdiagramms, das Diagramm als SVG und die Anhänge zur Auswahl.
//!
//! Alles hier ist eine Projektion; der Bericht schreibt nichts ins Log. Eine Ebene des Diagramms ist
//! eine Screening-Station mit allen ihren Runden (im freien Ablauf jede Stufe für sich). Stichproben
//! wie der Pilot sind keine Ebene, weil sie nichts aussortieren. Für einen Eintrag gilt die letzte
//! Runde, in der er zugeteilt war. Einträge in einer noch verblindeten Stufe zählen als offen; ihre
//! Entscheidungen erscheinen nirgends.
//!
//! [`Flow::check`] rechnet jede Ebene nach: Was hineingeht, muss sich auf geprüft, nicht zugeteilt
//! und nicht beschafft verteilen, und was geprüft ist, auf eingeschlossen, ausgeschlossen und offen.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::agreement::{self, Metric};
use crate::criteria::Criteria;
use crate::error::{invalid, Result};
use crate::forms::{Field, StageSummary};
use crate::projection::{MaybeMode, StageState};
use crate::protocol::{self, Checklist, Evidence, Protocol};
use crate::service::{csv_field, require, stage_criteria, Review, SourceCount, BOOTSTRAP_N, BOOTSTRAP_SEED, ROLES};
use crate::sources::{self, TypeCount, Unit as SourceUnit};
use crate::store::RecordRow;
use crate::workflow::{Station, Workflow};

/// Dateien, die der Bericht ausgibt: Schlüssel und vorgeschlagener Dateiname.
pub const REPORT_FILES: [(&str, &str); 9] = [
    ("diagram", "flussdiagramm.svg"),
    ("excluded", "ausgeschlossene-volltexte.csv"),
    ("included", "eingeschlossen.ris"),
    ("sources", "eingeschlossene-quellen.csv"),
    ("search", "suchanhang.md"),
    ("methods", "methoden-auswahl.md"),
    ("table", "tabelle-quellen.csv"),
    ("deviations", "protokoll-abweichungen.md"),
    ("checklist", "checkliste.md"),
];

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReasonCount {
    /// Kürzel des Ausschlusskriteriums; leer, wenn kein Hauptgrund genannt ist.
    pub code: String,
    pub text: String,
    pub n: i64,
}

/// Beschaffung der Volltexte vor einer Ebene. `sought` verteilt sich auf geprüft, `waiting`,
/// `unavailable` und `open`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Retrieval {
    pub sought: i64,
    /// Mit Grund als nicht beschaffbar vermerkt.
    pub unavailable: i64,
    /// Noch ohne Status.
    pub open: i64,
    /// Beschafft, aber noch keiner Runde zugeteilt.
    pub waiting: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Level {
    /// Schlüssel der Station, im freien Ablauf die ID der Stufe.
    pub key: String,
    pub label: String,
    pub rounds: i64,
    /// Ebene, deren Eingeschlossene hier hineingehen; ohne Angabe alle Einträge ohne Dubletten.
    pub input_from: Option<String>,
    pub input: i64,
    pub retrieval: Option<Retrieval>,
    /// Im Eingang, aber keiner Stufe zugeteilt (nur ohne Beschaffung).
    pub not_assigned: i64,
    pub assessed: i64,
    pub included: i64,
    pub excluded: i64,
    /// Unentschieden: ohne genug Stimmen, im Konflikt, in Klärung oder noch verblindet.
    pub open: i64,
    /// Davon in einer noch verblindeten Stufe.
    pub blinded: i64,
    /// Gründe der Ausgeschlossenen in der Reihenfolge der Kriterien; die Summe ist `excluded`.
    pub reasons: Vec<ReasonCount>,
    /// Volltext-Ebene: Das Diagramm nennt die Gründe, der Export listet die Ausschlüsse.
    pub fulltext: bool,
    /// Von den Geprüften und den Eingeschlossenen: Einträge aus anderen Quellen als Datenbanken und
    /// Registern (Zitationssuche, andere Wege).
    pub other_assessed: i64,
    pub other_included: i64,
}

/// Zahlen des Flussdiagramms.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Flow {
    pub sources: Vec<SourceCount>,
    pub identified: i64,
    pub exact_duplicates: i64,
    pub resolved_duplicates: i64,
    pub unique: i64,
    /// Von `identified`: Treffer aus Zitationssuche und anderen Wegen, der Zweig „andere Quellen“.
    pub other_identified: i64,
    pub levels: Vec<Level>,
    /// Eingeschlossene der letzten Ebene; fehlt, solange es keine Ebene gibt.
    pub included: Option<i64>,
    /// Dasselbe nach dem Bündeln: Begleitpublikationen derselben Studie zählen als eine Quelle.
    pub included_sources: Option<i64>,
    /// Eingeschlossene Quellen je Quellenart; leer, solange keine Quellenart vergeben ist.
    pub source_types: Vec<TypeCount>,
    /// Ob in keiner Ebene mehr etwas offen ist.
    pub complete: bool,
    pub record_term: String,
    pub records_term: String,
    pub source_term: String,
    pub sources_term: String,
}

impl Flow {
    /// Rechnet jede Ebene nach und nennt, was nicht aufgeht. Leer heißt: Das Diagramm stimmt.
    pub fn check(&self) -> Vec<String> {
        let mut out = Vec::new();
        // Obergrenzen statt Gleichungen; sie stehen am Ende der Liste.
        let mut beyond = Vec::new();
        let mut expect = |what: String, left: i64, right: i64| {
            if left != right {
                out.push(format!("{what}: {left} ist nicht {right}."));
            }
        };
        expect(
            "Treffer je Quelle und Summe".to_string(),
            self.sources.iter().map(|s| s.n_records).sum(),
            self.identified,
        );
        expect(
            "Treffer aus anderen Quellen".to_string(),
            self.sources.iter().filter(|s| is_other(&s.origin)).map(|s| s.n_records).sum(),
            self.other_identified,
        );
        expect(
            "Treffer ohne Dubletten".to_string(),
            self.identified - self.exact_duplicates - self.resolved_duplicates,
            self.unique,
        );
        for level in &self.levels {
            let name = &level.label;
            let from = match &level.input_from {
                None => Some(self.unique),
                Some(key) => self.levels.iter().find(|l| &l.key == key).map(|l| l.included),
            };
            if let Some(from) = from {
                expect(format!("Eingang von „{name}“"), level.input, from);
            }
            let aside = match &level.retrieval {
                Some(r) => {
                    expect(format!("Gesuchte Volltexte vor „{name}“"), r.sought, level.input);
                    r.unavailable + r.open + r.waiting
                }
                None => level.not_assigned,
            };
            expect(format!("Verbleib des Eingangs von „{name}“"), level.assessed + aside, level.input);
            expect(format!("Entscheidungen in „{name}“"), level.included + level.excluded + level.open, level.assessed);
            expect(format!("Gründe in „{name}“"), level.reasons.iter().map(|r| r.n).sum(), level.excluded);
            for (what, part, whole) in [
                ("Geprüfte", level.other_assessed, level.assessed.min(self.other_identified)),
                ("Eingeschlossene", level.other_included, level.included.min(level.other_assessed)),
            ] {
                if part > whole {
                    beyond.push(format!("{what} aus anderen Quellen in „{name}“: {part} ist mehr als {whole}."));
                }
            }
        }
        if let (Some(sources), Some(records)) = (self.included_sources, self.included) {
            if sources > records {
                beyond.push(format!("Eingeschlossene Quellen: {sources} ist mehr als {records} Einträge."));
            }
            if !self.source_types.is_empty() {
                expect("Quellen je Quellenart".to_string(), self.source_types.iter().map(|t| t.n).sum(), sources);
            }
        }
        expect(
            "Eingeschlossene am Ende".to_string(),
            self.included.unwrap_or(0),
            self.levels.last().map_or(0, |l| l.included),
        );
        out.extend(beyond);
        out
    }
}

/// Bericht für die Oberfläche.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub flow: Flow,
    /// Stellen, an denen das Diagramm nicht aufgeht; im Normalfall leer.
    pub problems: Vec<String>,
    pub svg: String,
    pub methods: String,
    pub search_appendix: String,
    /// Zeilen des Exports der ausgeschlossenen und nicht beschafften Volltexte.
    pub n_excluded_rows: i64,
    /// Registrierung des Protokolls und was danach geändert wurde.
    pub protocol: Protocol,
    /// Checkliste des Berichts mit Stand je Punkt; fehlt, wenn die Vorlage keine nennt.
    pub checklist: Option<Checklist>,
    pub table: TableInfo,
}

/// Umfang der Tabelle der eingeschlossenen Quellen.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TableInfo {
    pub n_sources: i64,
    /// Quellen, die extrahiert und abgeglichen sind.
    pub n_charted: i64,
    pub n_fields: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReportFile {
    pub filename: String,
    pub content: String,
}

// ---------------------------------------------------------------- Zahlen

enum Outcome {
    Include,
    Exclude(String),
    Open,
    Blind,
}

/// Eine Ebene vor dem Zählen: ihre Stufen und woher ihr Eingang kommt.
struct Unit<'a> {
    key: String,
    label: String,
    stages: Vec<&'a StageState>,
    from: Option<String>,
    requires_fulltext: bool,
}

struct ExcludedRow {
    record_id: Uuid,
    level: String,
    /// `excluded` oder `not_retrieved`.
    status: &'static str,
    code: String,
    reason: String,
}

struct Data {
    workflow: Workflow,
    stages: Vec<StageState>,
    flow: Flow,
    included: Vec<Uuid>,
    /// Die Eingeschlossenen als Quellen, Bündel zusammengefasst.
    units: Vec<SourceUnit>,
    excluded: Vec<ExcludedRow>,
}

/// Was am Ende der Auswahl eingeschlossen ist. Grundlage für das Bündeln und später das Charting.
pub(crate) struct Selection {
    pub included: Vec<Uuid>,
    /// Name der letzten Ebene.
    pub level: Option<String>,
    pub complete: bool,
}

/// Formular-Station mit ihren Stufen in der Reihenfolge der Runden.
struct ChartStation<'a> {
    station: &'a Station,
    stages: Vec<(&'a StageState, StageSummary)>,
}

/// Zeile der Tabelle der eingeschlossenen Quellen.
struct TableRow<'a> {
    unit: &'a SourceUnit,
    /// Stand aus dem Formular, sonst `blind` oder `not_assigned`.
    status: &'static str,
    values: Vec<String>,
}

/// Herkunft außerhalb von Datenbanken und Registern.
fn is_other(origin: &str) -> bool {
    matches!(origin, "citation" | "other")
}

fn stages_of<'a>(workflow: &Workflow, stages: &'a [StageState], station: &Station) -> Vec<&'a StageState> {
    let mut out: Vec<&StageState> =
        stages.iter().filter(|s| workflow.station_of(s).is_some_and(|x| x.key == station.key)).collect();
    out.sort_by_key(|s| s.config.round);
    out
}

fn is_sample(station: &Station) -> bool {
    station.input.as_ref().is_some_and(|i| i.scope == "sample")
}

fn units<'a>(workflow: &Workflow, stages: &'a [StageState]) -> Vec<Unit<'a>> {
    let mut out = Vec::new();
    for station in workflow.template.stations.iter().filter(|s| s.kind == "screening") {
        let own = stages_of(workflow, stages, station);
        if station.free_rounds {
            // Freier Ablauf: Jede Stufe ist eine Ebene, in der Reihenfolge ihrer Eröffnung.
            for stage in stages.iter().filter(|s| own.iter().any(|o| o.config.id == s.config.id)) {
                if stage.config.scope.as_deref() == Some("sample") {
                    continue;
                }
                out.push(Unit {
                    key: stage.config.id.to_string(),
                    label: stage.config.name.clone(),
                    stages: vec![stage],
                    from: stage.config.source_stage.clone(),
                    requires_fulltext: false,
                });
            }
        } else if !is_sample(station) {
            let from = station.input.as_ref().filter(|i| i.scope == "from_station").and_then(|i| i.station.clone());
            // Ohne eigene Stufe erscheint die Ebene erst, wenn ihre Vorstufe läuft: Dann zeigt sie,
            // was auf die Beschaffung oder die nächste Stufe wartet.
            let fed = from.as_ref().is_some_and(|key| out.iter().any(|u: &Unit| &u.key == key));
            if own.is_empty() && !fed {
                continue;
            }
            out.push(Unit {
                key: station.key.clone(),
                label: station.label.clone(),
                stages: own,
                from,
                requires_fulltext: station.requires_fulltext,
            });
        }
    }
    out
}

/// Hauptgrund eines Ausschlusses: der Grund der Klärung, sonst der gemeinsame Grund der Stimmen.
/// Nennen die Stimmen verschiedene Gründe, gilt der, der in den Kriterien zuerst steht.
fn exclusion_reason(stage: &StageState, record: &str, named: Option<&BTreeSet<&str>>, order: &[String]) -> String {
    if let Some(consensus) = stage.consensus.get(record) {
        return consensus.primary_reason.clone().unwrap_or_default();
    }
    let Some(named) = named else { return String::new() };
    order
        .iter()
        .map(String::as_str)
        .find(|code| named.contains(code))
        .or_else(|| named.iter().next().copied())
        .unwrap_or_default()
        .to_string()
}

impl Review {
    fn report_data(&self) -> Result<Data> {
        let state = self.state()?;
        require(&state, &self.me.actor_id, &ROLES)?;
        let workflow = self.workflow()?;
        let stages = self.stage_states(&state)?;
        let resolutions = self.resolutions()?;
        let active: BTreeSet<Uuid> = self
            .store
            .record_keys()?
            .into_iter()
            .filter(|k| k.duplicate_of.is_none() && !resolutions.is_removed(&k.id))
            .map(|k| k.id)
            .collect();
        let summary = self.import_summary()?;
        let (_, _, resolved) = self.duplicate_counts(&resolutions)?;
        let fulltexts = self.fulltexts()?;
        let other = self.records_of_origin(is_other)?;

        let mut levels: Vec<Level> = Vec::new();
        let mut included_of: BTreeMap<String, BTreeSet<Uuid>> = BTreeMap::new();
        let mut excluded_rows = Vec::new();
        let mut last_included: Vec<Uuid> = Vec::new();
        for unit in units(&workflow, &stages) {
            let mut latest: BTreeMap<Uuid, Outcome> = BTreeMap::new();
            let mut order: Vec<String> = Vec::new();
            let mut texts: BTreeMap<String, String> = BTreeMap::new();
            for stage in &unit.stages {
                let criteria = stage_criteria(&state, stage);
                for item in criteria.items.iter().filter(|i| i.kind == "exclude" && !i.code.is_empty()) {
                    if !order.contains(&item.code) {
                        order.push(item.code.clone());
                    }
                    texts.insert(item.code.clone(), item.text.clone());
                }
                let blind = stage.blind_active();
                let finals = if blind { BTreeMap::new() } else { stage.final_decisions() };
                let mut named: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
                for ((record, _), vote) in &stage.votes {
                    if let Some(reason) = vote.primary_reason.as_deref().filter(|_| vote.decision == "exclude") {
                        named.entry(record.as_str()).or_default().insert(reason);
                    }
                }
                for assignment in self.store.assignments(Some(&stage.config.id))? {
                    if !active.contains(&assignment.record_id) {
                        continue;
                    }
                    let id = assignment.record_id.to_string();
                    let outcome = match finals.get(&id).and_then(|d| d.decision.as_deref()) {
                        _ if blind => Outcome::Blind,
                        Some("include") => Outcome::Include,
                        Some("exclude") => {
                            Outcome::Exclude(exclusion_reason(stage, &id, named.get(id.as_str()), &order))
                        }
                        _ => Outcome::Open,
                    };
                    latest.insert(assignment.record_id, outcome);
                }
            }

            let feeding = unit.from.as_ref().and_then(|key| included_of.get(key));
            // Was geprüft wurde, ist auch hineingegangen, selbst wenn die Vorstufe es heute anders sieht.
            let mut input: BTreeSet<Uuid> = feeding.cloned().unwrap_or_else(|| active.clone());
            input.extend(latest.keys().copied());
            let is_fulltext = unit.requires_fulltext
                || unit.stages.iter().any(|s| s.config.reason_required || s.config.kind == "fulltext");

            let mut retrieval = unit.requires_fulltext.then_some(Retrieval {
                sought: input.len() as i64,
                unavailable: 0,
                open: 0,
                waiting: 0,
            });
            let mut not_assigned = 0;
            for id in input.iter().filter(|id| !latest.contains_key(id)) {
                match (&mut retrieval, fulltexts.status.get(id)) {
                    (None, _) => not_assigned += 1,
                    (Some(r), Some(ft)) if ft.status == "retrieved" => r.waiting += 1,
                    (Some(r), Some(ft)) => {
                        r.unavailable += 1;
                        excluded_rows.push(ExcludedRow {
                            record_id: *id,
                            level: unit.label.clone(),
                            status: "not_retrieved",
                            code: String::new(),
                            reason: ft.reason.clone(),
                        });
                    }
                    (Some(r), None) => r.open += 1,
                }
            }

            let mut level = Level {
                key: unit.key.clone(),
                label: unit.label.clone(),
                rounds: unit.stages.len() as i64,
                input_from: unit.from.clone().filter(|_| feeding.is_some()),
                input: input.len() as i64,
                retrieval,
                not_assigned,
                assessed: latest.len() as i64,
                included: 0,
                excluded: 0,
                open: 0,
                blinded: 0,
                reasons: Vec::new(),
                fulltext: is_fulltext,
                other_assessed: latest.keys().filter(|id| other.contains(id)).count() as i64,
                other_included: 0,
            };
            let mut included = BTreeSet::new();
            let mut by_reason: BTreeMap<String, i64> = BTreeMap::new();
            for (id, outcome) in &latest {
                match outcome {
                    Outcome::Include => {
                        level.included += 1;
                        level.other_included += i64::from(other.contains(id));
                        included.insert(*id);
                    }
                    Outcome::Exclude(code) => {
                        level.excluded += 1;
                        *by_reason.entry(code.clone()).or_default() += 1;
                        if is_fulltext {
                            excluded_rows.push(ExcludedRow {
                                record_id: *id,
                                level: unit.label.clone(),
                                status: "excluded",
                                code: code.clone(),
                                reason: texts.get(code).cloned().unwrap_or_default(),
                            });
                        }
                    }
                    Outcome::Open => level.open += 1,
                    Outcome::Blind => {
                        level.open += 1;
                        level.blinded += 1;
                    }
                }
            }
            // Erst die Kriterien in ihrer Reihenfolge, dann unbekannte Kürzel, zuletzt „ohne Hauptgrund“.
            let unknown: Vec<String> =
                by_reason.keys().filter(|c| !c.is_empty() && !order.contains(c)).cloned().collect();
            for code in order.iter().chain(&unknown).chain(std::iter::once(&String::new())) {
                if let Some(n) = by_reason.get(code) {
                    let text = match code.is_empty() {
                        true => "ohne Hauptgrund".to_string(),
                        false => texts.get(code).cloned().unwrap_or_default(),
                    };
                    level.reasons.push(ReasonCount { code: code.clone(), text, n: *n });
                }
            }
            last_included = included.iter().copied().collect();
            included_of.insert(unit.key, included);
            levels.push(level);
        }

        let complete = !levels.is_empty()
            && levels.iter().all(|l| {
                l.open == 0 && l.not_assigned == 0 && l.retrieval.as_ref().is_none_or(|r| r.open == 0 && r.waiting == 0)
            });
        let studies = self.studies()?;
        let units = match levels.is_empty() {
            true => Vec::new(),
            false => sources::units(&last_included, &studies),
        };
        let source_types = match units.iter().any(|u| !u.source_type.is_empty()) {
            true => sources::count_types(&units, &workflow.template.source_types),
            false => Vec::new(),
        };
        let flow = Flow {
            other_identified: summary.sources.iter().filter(|s| is_other(&s.origin)).map(|s| s.n_records).sum(),
            included_sources: levels.last().map(|_| units.len() as i64),
            source_types,
            sources: summary.sources,
            identified: summary.n_records,
            exact_duplicates: summary.n_exact_duplicates,
            resolved_duplicates: resolved,
            unique: active.len() as i64,
            included: levels.last().map(|l| l.included),
            complete,
            levels,
            record_term: workflow.template.term("record").to_string(),
            records_term: workflow.template.term("records").to_string(),
            source_term: workflow.template.term("source").to_string(),
            sources_term: workflow.template.term("sources").to_string(),
        };
        Ok(Data { workflow, stages, flow, included: last_included, units, excluded: excluded_rows })
    }

    /// Eingeschlossene der letzten Ebene. Entscheidungen verblindeter Stufen sind nicht dabei.
    pub(crate) fn selection(&self) -> Result<Selection> {
        let data = self.report_data()?;
        Ok(Selection {
            included: data.included,
            level: data.flow.levels.last().map(|l| l.label.clone()),
            complete: data.flow.complete,
        })
    }

    /// Flussdiagramm, Methodentext, Suchanhang, Protokoll und Checkliste auf dem Stand des Logs.
    pub fn report(&self) -> Result<Report> {
        let data = self.report_data()?;
        let charting = self.charting(&data);
        let protocol = self.protocol()?;
        let (methods, n_limitations) = self.methods_text(&data, &charting, &protocol)?;
        let (fields, rows) = table_rows(&data, &charting);
        let table = TableInfo {
            n_sources: rows.len() as i64,
            n_charted: rows.iter().filter(|r| r.status == "done").count() as i64,
            n_fields: fields.len() as i64,
        };
        let checklist = self.checklist(&data, &charting, &protocol, &table, n_limitations)?;
        Ok(Report {
            problems: data.flow.check(),
            svg: diagram_svg(&data.flow),
            methods,
            search_appendix: self.search_appendix()?,
            n_excluded_rows: data.excluded.len() as i64,
            flow: data.flow.clone(),
            protocol,
            checklist,
            table,
        })
    }

    /// Formular-Stationen mit dem Stand ihrer Stufen. Eine Stufe, deren Formular hier fehlt, bleibt weg.
    fn charting<'a>(&self, data: &'a Data) -> Vec<ChartStation<'a>> {
        let mut out = Vec::new();
        for station in data.workflow.template.stations.iter().filter(|s| s.kind == "form") {
            let mut stages: Vec<(&StageState, StageSummary)> = stages_of(&data.workflow, &data.stages, station)
                .into_iter()
                .filter_map(|stage| self.form_summary(stage).ok().map(|summary| (stage, summary)))
                .collect();
            stages.sort_by_key(|(stage, _)| stage.config.round);
            if !stages.is_empty() {
                out.push(ChartStation { station, stages });
            }
        }
        out
    }

    fn checklist(
        &self,
        data: &Data,
        charting: &[ChartStation],
        protocol: &Protocol,
        table: &TableInfo,
        n_limitations: i64,
    ) -> Result<Option<Checklist>> {
        let forms = self.form_defs()?;
        let fields = || forms.list.iter().flat_map(|f| f.current().fields.iter());
        let have = Evidence {
            registered: protocol.registration.is_some(),
            criteria_version: Criteria::from_payload(self.state()?.criteria.as_ref()).version,
            n_searches: self.searches()?.len() as i64,
            selection_started: data.stages.iter().any(|s| s.config.form_id.is_none()),
            has_levels: !data.flow.levels.is_empty(),
            form_published: !forms.list.is_empty(),
            charting_started: !charting.is_empty(),
            analysis_planned: fields().any(|f| !f.analysis.trim().is_empty()),
            n_charted: table.n_charted,
            has_choice_fields: fields().any(|f| matches!(f.kind.as_str(), "choice" | "multi_choice")),
            n_limitations,
        };
        Ok(protocol::checklist(&data.workflow, &have))
    }

    /// Tabelle der eingeschlossenen Quellen mit den gültigen Werten aus dem Formular.
    fn table_csv(&self, data: &Data, charting: &[ChartStation]) -> Result<String> {
        let known = &data.workflow.template.source_types;
        let studies = self.studies()?;
        let (fields, rows) = table_rows(data, charting);
        let line = |cells: Vec<String>| cells.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(",") + "\n";
        let mut head: Vec<String> =
            ["nr", "bundle", "source_type", "authors", "year", "title", "doi", "status"].map(String::from).into();
        head.extend(fields.iter().map(|f| f.label.clone()));
        let mut out = line(head);
        for (number, row) in rows.iter().enumerate() {
            let unit = row.unit;
            let Some(r) = self.store.record(&unit.primary)? else { continue };
            let label = unit.study.and_then(|s| studies.list.get(&s)).map(|s| s.label.clone()).unwrap_or_default();
            let kind = known.iter().find(|k| k.key == unit.source_type).map_or(&unit.source_type, |k| &k.label);
            let mut cells = vec![
                (number + 1).to_string(),
                label,
                kind.clone(),
                r.authors,
                r.year.map(|y| y.to_string()).unwrap_or_default(),
                r.title,
                r.doi,
                row.status.to_string(),
            ];
            cells.extend(row.values.iter().cloned());
            cells.resize(8 + fields.len(), String::new());
            out.push_str(&line(cells));
        }
        Ok(out)
    }

    /// Eine Datei des Berichts: `diagram`, `excluded`, `included`, `sources`, `search` oder `methods`.
    pub fn report_file(&self, kind: &str) -> Result<ReportFile> {
        let filename = REPORT_FILES
            .iter()
            .find(|(key, _)| *key == kind)
            .map(|(_, name)| name.to_string())
            .ok_or_else(|| invalid("Diese Datei kennt der Bericht nicht."))?;
        let data = self.report_data()?;
        let methods = |review: &Review| -> Result<(String, i64)> {
            review.methods_text(&data, &review.charting(&data), &review.protocol()?)
        };
        let content = match kind {
            "diagram" => diagram_svg(&data.flow),
            "excluded" => self.excluded_csv(&data)?,
            "included" => {
                if data.flow.levels.is_empty() {
                    return Err(invalid("Es gibt noch keine Auswahl, aus der Eingeschlossene hervorgehen."));
                }
                let mut records = Vec::new();
                for id in &data.included {
                    records.extend(self.store.record(id)?);
                }
                to_ris(&records)
            }
            "sources" => self.sources_csv(&data)?,
            "search" => self.search_appendix()?,
            "table" => self.table_csv(&data, &self.charting(&data))?,
            "deviations" => deviations_markdown(&self.protocol()?),
            "checklist" => {
                let charting = self.charting(&data);
                let protocol = self.protocol()?;
                let (fields, rows) = table_rows(&data, &charting);
                let table = TableInfo {
                    n_sources: rows.len() as i64,
                    n_charted: rows.iter().filter(|r| r.status == "done").count() as i64,
                    n_fields: fields.len() as i64,
                };
                let list = self.checklist(&data, &charting, &protocol, &table, methods(self)?.1)?;
                let list = list.ok_or_else(|| invalid("Die Vorlage dieses Reviews nennt keine Checkliste."))?;
                protocol::checklist_markdown(&list)
            }
            _ => methods(self)?.0,
        };
        Ok(ReportFile { filename, content })
    }

    fn excluded_csv(&self, data: &Data) -> Result<String> {
        let mut out = String::from("record_id,title,authors,year,journal,doi,station,status,reason_code,reason\n");
        for row in &data.excluded {
            let Some(r) = self.store.record(&row.record_id)? else { continue };
            let fields = [
                r.id.to_string(),
                r.title,
                r.authors,
                r.year.map(|y| y.to_string()).unwrap_or_default(),
                r.journal,
                r.doi,
                row.level.clone(),
                row.status.to_string(),
                row.code.clone(),
                row.reason.clone(),
            ];
            out.push_str(&fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
            out.push('\n');
        }
        Ok(out)
    }

    /// Eingeschlossene Quellen: je Eintrag eine Zeile, Einträge desselben Bündels tragen dieselbe Nummer.
    fn sources_csv(&self, data: &Data) -> Result<String> {
        let known = &data.workflow.template.source_types;
        let studies = self.studies()?;
        let included: BTreeSet<&Uuid> = data.included.iter().collect();
        let mut out =
            String::from("source,bundle,source_type,role,included,record_id,title,authors,year,journal,doi\n");
        for (number, unit) in data.units.iter().enumerate() {
            let label = unit.study.and_then(|s| studies.list.get(&s)).map(|s| s.label.clone()).unwrap_or_default();
            let kind = known.iter().find(|k| k.key == unit.source_type).map_or(&unit.source_type, |k| &k.label);
            for id in &unit.records {
                let Some(r) = self.store.record(id)? else { continue };
                let fields = [
                    (number + 1).to_string(),
                    label.clone(),
                    kind.clone(),
                    if *id == unit.primary { "primary" } else { "companion" }.to_string(),
                    if included.contains(id) { "yes" } else { "no" }.to_string(),
                    r.id.to_string(),
                    r.title,
                    r.authors,
                    r.year.map(|y| y.to_string()).unwrap_or_default(),
                    r.journal,
                    r.doi,
                ];
                out.push_str(&fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
                out.push('\n');
            }
        }
        Ok(out)
    }

    /// Einträge, deren Import eine Herkunft nennt, auf die `wanted` zutrifft.
    fn records_of_origin(&self, wanted: fn(&str) -> bool) -> Result<BTreeSet<Uuid>> {
        let batches: Vec<String> =
            self.import_origins()?.into_iter().filter(|(_, o)| wanted(o)).map(|(b, _)| b.to_string()).collect();
        let mut out = BTreeSet::new();
        if batches.is_empty() {
            return Ok(out);
        }
        let mut stmt = self.store.conn().prepare("SELECT id, batch_id FROM record")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, batch) = row?;
            if batches.contains(&batch) {
                out.extend(Uuid::parse_str(&id));
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------ Suchanhang

    /// Suchen mit Suchstring, Datum, Grenzen und Trefferzahlen als Markdown.
    fn search_appendix(&self) -> Result<String> {
        let searches = self.searches()?;
        let summary = self.import_summary()?;
        let mut out = String::from("# Suchanhang\n");
        if searches.is_empty() {
            out.push_str("\nEs ist keine Suche festgehalten.\n");
        }
        for search in &searches {
            let Some(latest) = search.versions.last() else { continue };
            let v = &latest.version;
            let platform = if v.platform.is_empty() { String::new() } else { format!(" ({})", v.platform) };
            out.push_str(&format!("\n## {}{platform}\n\n", v.source));
            out.push_str(&format!("- Herkunft: {}\n", origin_label(&v.origin)));
            out.push_str(&format!("- Tag der Suche: {}\n", german_date(&v.searched_on)));
            if !v.limits.is_empty() {
                out.push_str(&format!("- Grenzen: {}\n", v.limits));
            }
            if let Some(limit) = v.hit_limit {
                out.push_str(&format!("- Übernommen wurden höchstens die ersten {limit} Treffer.\n"));
            }
            if let Some(hits) = v.hits {
                out.push_str(&format!("- Treffer laut Quelle: {hits}\n"));
            }
            out.push_str(&format!(
                "- Importiert: {} (davon {} beim Import als Dublette erkannt)\n",
                latest.n_records, latest.n_duplicates
            ));
            for review in &v.peer_reviews {
                out.push_str(&format!(
                    "- Peer-Review am {}: {}\n",
                    german_date(&review.reviewed_on),
                    one_line(&review.note)
                ));
            }
            out.push_str(&format!("\nSuchstring (Version {}):\n\n{}\n", v.version, code_block(&v.query)));
            let older: Vec<_> = search.versions.iter().rev().skip(1).collect();
            for earlier in older {
                let e = &earlier.version;
                out.push_str(&format!(
                    "\nVersion {} vom {}, importiert: {}.",
                    e.version,
                    german_date(&e.searched_on),
                    earlier.n_records
                ));
                if let Some(next) = search.versions.iter().find(|x| x.version.previous_version == Some(e.version)) {
                    if !next.version.change_note.is_empty() {
                        out.push_str(&format!(
                            " Geändert zu Version {}: {}",
                            next.version.version,
                            one_line(&next.version.change_note)
                        ));
                    }
                }
                out.push_str(&format!("\n\n{}\n", code_block(&e.query)));
            }
        }
        let loose: Vec<&SourceCount> = summary.sources.iter().filter(|s| s.search_id.is_none()).collect();
        if !loose.is_empty() {
            out.push_str("\n## Importe ohne festgehaltene Suche\n\n");
            for s in loose {
                let source = if s.source.is_empty() { "ohne Angabe" } else { &s.source };
                out.push_str(&format!(
                    "- {source} ({}): {} importiert, davon {} beim Import als Dublette erkannt\n",
                    origin_label(&s.origin),
                    s.n_records,
                    s.n_duplicates
                ));
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------ Methodentext

    /// Methodentext zur Auswahl als Markdown: Regel, Personenzahl, Pilot mit Runden, Übereinstimmung,
    /// Klärung und die Limitation bei abgekürzter Auswahl. Namen stehen nicht darin.
    fn methods_text(&self, data: &Data, charting: &[ChartStation], protocol: &Protocol) -> Result<(String, i64)> {
        let (flow, workflow) = (&data.flow, &data.workflow);
        let records = &flow.records_term;
        let mut out = format!("# Methoden: Auswahl der {}\n\n", flow.sources_term);
        if workflow.published() {
            out.push_str(&format!(
                "Der Ablauf folgte der Vorlage „{}“ (Version {}).",
                workflow.template.name, workflow.version
            ));
        } else {
            out.push_str("Das Review lief ohne feste Vorlage.");
        }
        if let Some(r) = &protocol.registration {
            let identifier = if r.identifier.is_empty() { String::new() } else { format!(" ({})", r.identifier) };
            out.push_str(&format!(
                " Das Protokoll wurde am {} registriert: {}{identifier}.",
                german_date(&r.registered_on),
                r.registry
            ));
        }
        if !flow.complete {
            out.push_str(" Stand: Die Auswahl ist noch nicht abgeschlossen; die Zahlen sind vorläufig.");
        }
        out.push_str("\n\n## Suche und Dubletten\n\n");
        let mut names: Vec<&str> = Vec::new();
        for s in &flow.sources {
            let name = if s.source.is_empty() { "ohne Angabe" } else { s.source.as_str() };
            if !names.contains(&name) {
                names.push(name);
            }
        }
        out.push_str(&format!(
            "Aus {} wurden {} {records} übernommen ({}). ",
            count(names.len() as i64, "Quelle", "Quellen"),
            flow.identified,
            names.join(", ")
        ));
        if flow.other_identified > 0 {
            out.push_str(&format!(
                "Davon stammen {} aus der Zitationssuche oder von anderen Wegen außerhalb von Datenbanken und Registern; \
                 sie durchliefen dieselben Schritte als weitere Runde. ",
                flow.other_identified
            ));
        }
        out.push_str(&format!(
            "Beim Import wurden {} als exakte Dubletten erkannt (gleiche DOI, sonst gleicher Titel und gleiches Jahr). \
             Mögliche weitere Dubletten wurden paarweise geprüft; {} wurden entfernt. Es blieben {} {records}. \
             Suchstrings, Datum und Grenzen je Quelle stehen im Suchanhang.\n",
            flow.exact_duplicates, flow.resolved_duplicates, flow.unique
        ));
        for search in self.searches()? {
            let (Some(first), Some(last)) = (search.versions.first(), search.versions.last()) else { continue };
            if search.versions.len() > 1 {
                out.push_str(&format!(
                    "\nDie Suche in {} wurde nach dem {} geändert oder wiederholt, zuletzt am {} (Version {}).\n",
                    last.version.source,
                    german_date(&first.version.searched_on),
                    german_date(&last.version.searched_on),
                    last.version.version
                ));
            }
        }

        let mut limitations: Vec<String> = Vec::new();
        for station in workflow.template.stations.iter().filter(|s| s.kind == "screening") {
            let own = stages_of(workflow, &data.stages, station);
            if own.is_empty() {
                continue;
            }
            let groups: Vec<(String, Vec<&StageState>)> = match station.free_rounds {
                true => own.iter().map(|s| (s.config.name.clone(), vec![*s])).collect(),
                false => vec![(station.label.clone(), own)],
            };
            for (label, group) in groups {
                out.push_str(&format!("\n## {label}\n\n"));
                let level = flow
                    .levels
                    .iter()
                    .find(|l| l.key == station.key || group.first().is_some_and(|s| l.key == s.config.id.to_string()));
                out.push_str(&self.station_text(data, station, &label, &group, level, &mut limitations));
            }
        }

        for chart in charting {
            out.push_str(&format!("\n## {}\n\n", chart.station.label));
            out.push_str(&charting_text(data, chart, &mut limitations));
        }

        let changes = protocol.deviations.iter().filter(|d| d.kind != "gate").map(|d| {
            let note = if d.note.is_empty() { String::new() } else { format!(": {}", one_line(&d.note)) };
            format!("- {}: {}{note}\n", german_date(&d.at[..d.at.len().min(10)]), d.title)
        });
        limitations.extend(changes);
        let overrides: Vec<String> = workflow
            .overrides
            .iter()
            .map(|o| {
                let label = workflow.template.station(&o.station).map_or(o.station.as_str(), |s| s.label.as_str());
                format!("- „{label}“ wurde von der Leitung ohne erfülltes Gate freigegeben: {}\n", one_line(&o.reason))
            })
            .collect();
        if !limitations.is_empty() || !overrides.is_empty() {
            out.push_str("\n## Abweichungen und Limitationen\n\n");
            for line in limitations.iter().chain(&overrides) {
                out.push_str(line);
            }
        }
        if let Some(n) = flow.included {
            out.push_str(&format!(
                "\n## Ergebnis\n\n{} {} eingeschlossen{}.\n",
                n,
                if n == 1 { "Eintrag wurde".to_string() } else { format!("{records} wurden") },
                if flow.complete { "" } else { " (vorläufig)" }
            ));
            let other = flow.levels.last().map_or(0, |l| l.other_included);
            if flow.other_identified > 0 {
                out.push_str(&format!(
                    "Davon {} aus der Zitationssuche oder von anderen Wegen.\n",
                    if other == 1 { "stammt 1".to_string() } else { format!("stammen {other}") }
                ));
            }
            if let Some(sources) = flow.included_sources.filter(|s| *s != n) {
                out.push_str(&format!(
                    "\nMehrere Veröffentlichungen derselben Studie wurden zu einer {} gebündelt, damit nichts doppelt \
                     zählt. Gezählt werden {}.\n",
                    flow.source_term,
                    count(sources, &flow.source_term, &flow.sources_term)
                ));
            }
            if !flow.source_types.is_empty() {
                out.push_str(&format!("\n{} je Quellenart:\n\n", flow.sources_term));
                for kind in &flow.source_types {
                    out.push_str(&format!("- {}: {}\n", kind.label, kind.n));
                }
            }
        }
        Ok((out, (limitations.len() + overrides.len()) as i64))
    }

    fn station_text(
        &self,
        data: &Data,
        station: &Station,
        label: &str,
        stages: &[&StageState],
        level: Option<&Level>,
        limitations: &mut Vec<String>,
    ) -> String {
        let records = &data.flow.records_term;
        let Some(last) = stages.last() else { return String::new() };
        let config = &last.config;
        let mut out = String::new();
        let pilot = is_sample(station) || config.scope.as_deref() == Some("sample");
        if pilot {
            out.push_str(&format!(
                "Vor der Auswahl bewertete das Team in {} eine Zufallsstichprobe. ",
                count(stages.len() as i64, "Runde", "Runden")
            ));
            if let Some(percent) = station.gate.iter().find(|g| g.check == "agreement_min").and_then(|g| g.percent) {
                out.push_str(&format!("Für den Übergang waren mindestens {percent} % Übereinstimmung verlangt. "));
            }
        }

        // Regel der Auswahl
        match config.decision_rule.as_str() {
            "single_verified" => {
                let scope = match config.verify.as_ref().filter(|v| v.scope == "sample").and_then(|v| v.percent) {
                    Some(p) => format!("eine Zufallsstichprobe von {p} % der Ausschlüsse"),
                    None => "alle Ausschlüsse".to_string(),
                };
                out.push_str(&format!("Eine Person bewertete jeden Eintrag; eine zweite Person prüfte {scope}. "));
                limitations.push(format!(
                    "- In „{label}“ bewertete nur eine Person jeden Eintrag; eine zweite prüfte {scope}. \
                     Einschlüsse wurden nicht doppelt bewertet. Das weicht von zwei unabhängigen Stimmen je Eintrag ab; \
                     einzelne passende {} können dadurch fehlen.\n",
                    data.flow.sources_term
                ));
            }
            rule => {
                let n = config.min_reviewers.max(if rule == "liberal" { 2 } else { 1 });
                let how = match (n, config.blind) {
                    (1, _) => "Jeder Eintrag wurde von einer Person bewertet".to_string(),
                    (n, true) => format!(
                        "Jeder Eintrag wurde von mindestens {n} Personen unabhängig bewertet; die Stimmen der anderen waren bis zum Entblinden nicht sichtbar"
                    ),
                    (n, false) => format!(
                        "Jeder Eintrag wurde von mindestens {n} Personen bewertet; die Stimmen waren gegenseitig sichtbar"
                    ),
                };
                out.push_str(&how);
                out.push_str(match (rule, n) {
                    ("liberal", _) => ". Ein Eintrag kam weiter, sobald eine Person ihn einschloss; ausgeschlossen wurde nur einstimmig. ",
                    (_, 1) => ". ",
                    _ => ". Entschieden war ein Eintrag bei übereinstimmenden Stimmen. ",
                });
                if n == 1 {
                    limitations.push(format!(
                        "- In „{label}“ bewertete nur eine Person jeden Eintrag, ohne Prüfung durch eine zweite. \
                         Einzelne passende {} können dadurch fehlen.\n",
                        data.flow.sources_term
                    ));
                }
                if !config.blind && n > 1 {
                    limitations.push(format!("- In „{label}“ waren die Stimmen nicht verblindet.\n"));
                }
            }
        }
        out.push_str(match config.maybe_as.as_deref() {
            Some("include") => "„Vielleicht“ zählte als Einschluss. ",
            Some("exclude") => "„Vielleicht“ zählte als Ausschluss. ",
            _ => "„Vielleicht“ blieb eine eigene Antwort und musste geklärt werden. ",
        });
        if config.decision_rule != "single_verified" && config.min_reviewers.max(1) > 1
            || config.decision_rule == "liberal"
        {
            out.push_str(match config.conflict_rule.as_str() {
                "third_person" => {
                    "Abweichungen entschied eine dritte Person, die den Eintrag nicht selbst bewertet hatte. "
                }
                _ => "Abweichungen wurden im Gespräch geklärt. ",
            });
        }
        if config.reason_required {
            out.push_str(
                "Jeder Ausschluss trägt genau einen Hauptgrund aus den Ausschlusskriterien. Nannten die Stimmen \
                 verschiedene Gründe und gab es keine Klärung, zählt der Grund, der in den Kriterien zuerst steht. ",
            );
        }
        if let Some(version) = config.criteria_version.as_deref().filter(|v| !v.is_empty()) {
            out.push_str(&format!("Es galten die Kriterien in Version {version}. "));
        }
        out.push('\n');

        // Zahlen je Runde
        for stage in stages {
            let c = &stage.config;
            let head = match stages.len() {
                1 => format!("\n{} {records}", c.n_records),
                _ => format!("\nRunde {}: {} {records}", c.round, c.n_records),
            };
            out.push_str(&head);
            if stage.blind_active() {
                out.push_str("; die Stufe ist noch verblindet, Zahlen folgen nach dem Entblinden.\n");
                continue;
            }
            let people: BTreeSet<Uuid> = stage.votes.keys().map(|(_, actor)| *actor).collect();
            out.push_str(&format!(", {} beteiligt. ", count(people.len() as i64, "Person", "Personen")));
            out.push_str(&agreement_text(stage, records));
            if !stage.consensus.is_empty() {
                out.push_str(&format!(
                    "Über {} wurde in der Klärung entschieden. ",
                    count(stage.consensus.len() as i64, "Eintrag", records)
                ));
            }
            if !stage.closed {
                out.push_str("Die Stufe ist noch offen. ");
            }
            out.push('\n');
        }

        if let Some(level) = level {
            out.push('\n');
            if let Some(r) = &level.retrieval {
                out.push_str(&format!(
                    "Für {} wurde der Volltext gesucht; {} nicht beschaffbar",
                    count(r.sought, "Eintrag", records),
                    if r.unavailable == 1 { "1 war".to_string() } else { format!("{} waren", r.unavailable) }
                ));
                if r.open + r.waiting > 0 {
                    out.push_str(&format!(", bei {} läuft die Beschaffung oder Prüfung noch", r.open + r.waiting));
                }
                out.push_str(". ");
            }
            out.push_str(&format!(
                "Geprüft: {}, eingeschlossen: {}, ausgeschlossen: {}",
                level.assessed, level.included, level.excluded
            ));
            if level.open > 0 {
                out.push_str(&format!(", noch offen: {}", level.open));
            }
            out.push_str(".\n");
            if level.fulltext && level.excluded > 0 {
                out.push_str("\nAusschlussgründe:\n\n");
                for reason in &level.reasons {
                    out.push_str(&format!("- {}: {}\n", reason_label(reason), reason.n));
                }
            }
        }
        out
    }
}

/// Felder und Zeilen der Tabelle der eingeschlossenen Quellen. Die Werte kommen aus der letzten
/// Formular-Station, die alle Quellen bearbeitet; für eine Quelle gilt ihre späteste Runde.
fn table_rows<'a>(data: &'a Data, charting: &[ChartStation]) -> (Vec<Field>, Vec<TableRow<'a>>) {
    let main = charting.iter().rev().find(|c| !is_sample(c.station));
    let fields = main.and_then(|c| c.stages.last()).map(|(_, s)| s.fields.clone()).unwrap_or_default();
    let mut found: BTreeMap<Uuid, (&'static str, Vec<String>)> = BTreeMap::new();
    for (_, summary) in main.into_iter().flat_map(|c| &c.stages) {
        for row in &summary.rows {
            found.insert(row.record_id, (row.status, row.values.clone()));
        }
    }
    let rows = data
        .units
        .iter()
        .map(|unit| match unit.records.iter().find_map(|r| found.get(r)) {
            Some((status, values)) => TableRow { unit, status, values: values.clone() },
            None => TableRow { unit, status: "not_assigned", values: Vec::new() },
        })
        .collect();
    (fields, rows)
}

/// Abschnitt des Methodentexts zu einer Formular-Station.
fn charting_text(data: &Data, chart: &ChartStation, limitations: &mut Vec<String>) -> String {
    let (source, sources) = (&data.flow.source_term, &data.flow.sources_term);
    let label = &chart.station.label;
    let Some((last, summary)) = chart.stages.last() else { return String::new() };
    let config = &last.config;
    let mut out = String::new();
    let rounds = count(chart.stages.len() as i64, "Runde", "Runden");
    if is_sample(chart.station) {
        out.push_str(&format!(
            "Das Formular „{}“ wurde in {rounds} an einer Zufallsstichprobe je Quellenart erprobt und im Team abgestimmt. ",
            summary.form_name
        ));
        out.push_str(match summary.confirmed {
            true => "Danach bestätigte die Leitung das Formular. ",
            false => "Das Formular ist noch nicht bestätigt. ",
        });
    } else {
        out.push_str(&format!(
            "Extrahiert wurde mit dem Formular „{}“ (Version {}). ",
            summary.form_name, summary.form_version
        ));
    }
    match (config.decision_rule.as_str(), config.min_reviewers.max(1), config.blind) {
        ("single_verified", _, _) => {
            out.push_str(&format!(
                "Eine Person extrahierte jede {source}; eine zweite prüfte die Angaben und berichtigte sie. "
            ));
            limitations.push(format!(
                "- In „{label}“ extrahierte nur eine Person jede {source}; eine zweite prüfte die Angaben. \
                 Das weicht von zwei unabhängigen Extraktionen ab.\n"
            ));
        }
        (_, 1, _) => {
            out.push_str(&format!("Jede {source} wurde von einer Person extrahiert. "));
            limitations.push(format!(
                "- In „{label}“ extrahierte nur eine Person jede {source}, ohne Prüfung durch eine zweite.\n"
            ));
        }
        (_, n, true) => out.push_str(&format!(
            "Jede {source} wurde von mindestens {n} Personen unabhängig extrahiert; die Angaben der anderen waren bis \
             zum Entblinden nicht sichtbar. Abweichungen wurden je Feld geklärt. "
        )),
        (_, n, false) => {
            out.push_str(&format!(
                "Jede {source} wurde von mindestens {n} Personen extrahiert; die Angaben waren gegenseitig sichtbar. \
                 Abweichungen wurden je Feld geklärt. "
            ));
            limitations.push(format!("- In „{label}“ waren die Angaben nicht verblindet.\n"));
        }
    }
    out.push('\n');
    if !is_sample(chart.station) {
        out.push_str(&format!("\nErhoben wurden {}:\n\n", count(summary.fields.len() as i64, "Angabe", "Angaben")));
        for field in &summary.fields {
            let mut line = format!("- {}", field.label);
            if !field.description.trim().is_empty() {
                line.push_str(&format!(": {}", one_line(&field.description)));
            }
            if !field.options.is_empty() {
                line.push_str(&format!(" ({})", field.options.join(", ")));
            }
            if !field.analysis.trim().is_empty() {
                line.push_str(&format!(" Auswertung: {}.", one_line(field.analysis.trim_end_matches('.'))));
            }
            out.push_str(&line);
            out.push('\n');
        }
    }
    for (stage, s) in &chart.stages {
        let head = match chart.stages.len() {
            1 => format!("\n{} {sources}", s.n_sources),
            _ => format!("\nRunde {}: {} {sources}", stage.config.round, s.n_sources),
        };
        out.push_str(&head);
        let (Some(counts), Some(people)) = (&s.counts, s.n_people) else {
            out.push_str("; die Stufe ist noch verblindet, Zahlen folgen nach dem Entblinden.\n");
            continue;
        };
        out.push_str(&format!(", {} beteiligt. Abgeglichen: {}", count(people, "Person", "Personen"), counts.done));
        for (n, what) in [
            (counts.conflict, "mit offener Abweichung"),
            (counts.incomplete, "mit leerem Pflichtfeld"),
            (counts.extracted, "wartet auf Prüfung"),
            (counts.open, "noch offen"),
        ] {
            if n > 0 {
                out.push_str(&format!(", {what}: {n}"));
            }
        }
        out.push_str(". ");
        if s.n_resolved > 0 {
            out.push_str(&format!(
                "{} in der Klärung festgelegt. ",
                count(s.n_resolved, "Feld wurde", "Felder wurden")
            ));
        }
        if !stage.closed {
            out.push_str("Die Stufe ist noch offen. ");
        }
        out.push('\n');
    }
    out
}

/// Registrierung und Abweichungen als Markdown.
fn deviations_markdown(protocol: &Protocol) -> String {
    let mut out = String::from("# Protokoll und Abweichungen\n\n");
    match &protocol.registration {
        Some(r) => {
            let identifier = if r.identifier.is_empty() { String::new() } else { format!(" ({})", r.identifier) };
            out.push_str(&format!(
                "Protokoll registriert am {}: {}{identifier}.",
                german_date(&r.registered_on),
                r.registry
            ));
            if !r.note.is_empty() {
                out.push_str(&format!(" {}", one_line(&r.note)));
            }
            out.push_str(&format!(
                "\n\nIm Review festgehalten am {}. Neue Versionen von Kriterien, Suchen und Ablauf seit diesem Tag \
                 stehen unten.\n",
                german_date(&r.recorded_at[..r.recorded_at.len().min(10)])
            ));
        }
        None => out.push_str(
            "Es ist kein Protokoll registriert. Ohne diesen Bezugspunkt stehen hier nur Formularänderungen nach der \
             Bestätigung und übersteuerte Gates.\n",
        ),
    }
    out.push_str("\n## Abweichungen\n\n");
    if protocol.deviations.is_empty() {
        out.push_str("Keine.\n");
    }
    for d in &protocol.deviations {
        let note = if d.note.is_empty() { "ohne Begründung".to_string() } else { one_line(&d.note) };
        out.push_str(&format!("- {}: {}. Begründung: {note}\n", german_date(&d.at[..d.at.len().min(10)]), d.title));
    }
    out
}

/// Übereinstimmung vor der Klärung: Prozent und Krippendorffs Alpha, beide mit Intervall.
fn agreement_text(stage: &StageState, records: &str) -> String {
    let votes = stage.stage_votes(&MaybeMode::Stage);
    let (agree, n) = agreement::agreement_counts(&votes);
    if n == 0 {
        return String::new();
    }
    let percent = agreement::bootstrap(&votes, &Metric::Percent, BOOTSTRAP_N, BOOTSTRAP_SEED, 0.95);
    let alpha = agreement::bootstrap(&votes, &Metric::Alpha, BOOTSTRAP_N, BOOTSTRAP_SEED, 0.95);
    let interval = |low: Option<f64>, high: Option<f64>, scale: f64, digits: usize| match (low, high) {
        (Some(low), Some(high)) => {
            format!("95-%-Intervall {} bis {}", decimal(low * scale, digits), decimal(high * scale, digits))
        }
        _ => "Intervall nicht bestimmbar".to_string(),
    };
    let mut out = format!(
        "Übereinstimmung vor der Klärung: {} % ({agree} von {n} mehrfach bewerteten {records}n; {})",
        decimal(agree as f64 * 100.0 / n as f64, 1),
        interval(percent.ci_low, percent.ci_high, 100.0, 1)
    );
    match alpha.value {
        Some(value) => out.push_str(&format!(
            ", Krippendorffs Alpha {} ({}; Perzentil-Bootstrap mit {BOOTSTRAP_N} Ziehungen, Startwert {BOOTSTRAP_SEED}). ",
            decimal(value, 2),
            interval(alpha.ci_low, alpha.ci_high, 1.0, 2)
        )),
        None => out.push_str("; Krippendorffs Alpha ist nicht bestimmbar, weil alle Stimmen gleich lauten. "),
    }
    out
}

// ---------------------------------------------------------------- Texthelfer

fn count(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Zahl mit Dezimalkomma.
fn decimal(value: f64, digits: usize) -> String {
    format!("{value:.digits$}").replace('.', ",")
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Codeblock in Markdown; der Zaun ist länger als jede Folge von Backticks im Text.
fn code_block(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}\n{}\n{fence}", text.trim_end())
}

fn german_date(iso: &str) -> String {
    let parts: Vec<&str> = iso.split('-').collect();
    match parts.as_slice() {
        [y, m, d] => format!("{d}.{m}.{y}"),
        _ => iso.to_string(),
    }
}

fn origin_label(origin: &str) -> &'static str {
    match origin {
        "database" => "Datenbank",
        "register" => "Register",
        "citation" => "Zitationssuche",
        "other" => "andere Quelle",
        _ => "ohne Angabe",
    }
}

fn reason_label(reason: &ReasonCount) -> String {
    match (reason.code.is_empty(), reason.text.is_empty()) {
        (true, _) => reason.text.clone(),
        (false, true) => reason.code.clone(),
        (false, false) => format!("{} {}", reason.code, reason.text),
    }
}

// ---------------------------------------------------------------- RIS

/// Schreibt Einträge als RIS. Der Typ kommt aus den Rohdaten eines RIS-Imports, sonst `JOUR`.
pub fn to_ris(records: &[RecordRow]) -> String {
    let mut out = String::new();
    let line = |out: &mut String, tag: &str, value: &str| {
        let value = one_line(value);
        if !value.is_empty() {
            out.push_str(&format!("{tag}  - {value}\n"));
        }
    };
    for r in records {
        let kind = r
            .raw
            .get("TY")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_uppercase()))
            .unwrap_or("JOUR");
        line(&mut out, "TY", kind);
        for author in r.authors.split(';') {
            line(&mut out, "AU", author);
        }
        line(&mut out, "TI", &r.title);
        line(&mut out, "PY", &r.year.map(|y| y.to_string()).unwrap_or_default());
        line(&mut out, "JO", &r.journal);
        line(&mut out, "DO", &r.doi);
        line(&mut out, "AN", &r.pmid);
        line(&mut out, "UR", &r.url);
        for keyword in r.keywords.split(';') {
            line(&mut out, "KW", keyword);
        }
        line(&mut out, "AB", &r.abstract_text);
        out.push_str("ER  - \n\n");
    }
    out
}

// ---------------------------------------------------------------- Diagramm

const MAIN_WIDTH: i64 = 330;
const SIDE_WIDTH: i64 = 310;
const COLUMN_GAP: i64 = 50;
const ROW_GAP: i64 = 30;
const MARGIN: i64 = 12;
const PADDING: i64 = 10;
const LINE: i64 = 18;
/// Zeichen je Zeile; bei 13 px Schrift passen gut sieben Pixel je Zeichen.
const MAIN_CHARS: usize = 42;
const SIDE_CHARS: usize = 39;

/// Kasten des Diagramms: eine fette Kopfzeile und Zeilen darunter.
struct TextBox {
    head: String,
    lines: Vec<String>,
}

impl TextBox {
    fn new(head: String) -> TextBox {
        TextBox { head, lines: Vec::new() }
    }

    fn line(mut self, text: String) -> TextBox {
        self.lines.push(text);
        self
    }

    fn line_if(self, condition: bool, text: String) -> TextBox {
        match condition {
            true => self.line(text),
            false => self,
        }
    }

    /// Zeilen nach dem Umbruch: (Text, fett, eingerückt).
    fn wrapped(&self, chars: usize) -> Vec<(String, bool, bool)> {
        let mut out: Vec<(String, bool, bool)> =
            wrap(&self.head, chars).into_iter().map(|l| (l, true, false)).collect();
        for line in &self.lines {
            for (i, part) in wrap(line, chars - 2).into_iter().enumerate() {
                out.push((part, false, i > 0));
            }
        }
        out
    }
}

/// Bricht an Leerzeichen um; ein Wort, das länger ist als die Zeile, bleibt ganz.
fn wrap(text: &str, chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > chars {
            out.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Kasten rechts neben der Hauptspalte.
enum Side {
    None,
    /// Was die Hauptspalte verlässt (entfernt, ausgeschlossen).
    Out(TextBox),
    /// Was von der Seite dazukommt und unterhalb der Zeile in den Fluss mündet.
    In(TextBox),
}

impl Side {
    fn text(&self) -> Option<&TextBox> {
        match self {
            Side::None => None,
            Side::Out(text) | Side::In(text) => Some(text),
        }
    }
}

fn rows(flow: &Flow) -> Vec<(TextBox, Side)> {
    let records = &flow.records_term;
    let mut out = Vec::new();
    // Je Quelle eine Zeile, getrennt nach Datenbanken und Registern und nach anderen Wegen.
    let mut by_source: Vec<(bool, String, i64)> = Vec::new();
    for s in &flow.sources {
        let name = if s.source.is_empty() { "ohne Angabe".to_string() } else { s.source.clone() };
        let other = is_other(&s.origin);
        match by_source.iter_mut().find(|(o, n, _)| *o == other && *n == name) {
            Some(entry) => entry.2 += s.n_records,
            None => by_source.push((other, name, s.n_records)),
        }
    }
    let listed = |mut text: TextBox, other: bool| {
        for (_, name, n) in by_source.iter().filter(|(o, _, _)| *o == other) {
            text = text.line(format!("{name}: {n}"));
        }
        text
    };
    let branch = flow.other_identified > 0;
    let total = format!("Identifiziert: {}", count(flow.identified, &flow.record_term, records));
    let removed =
        TextBox::new(format!("Vor der Auswahl entfernt: {}", flow.exact_duplicates + flow.resolved_duplicates))
            .line(format!("Dubletten, beim Import erkannt: {}", flow.exact_duplicates))
            .line(format!("Dubletten, einzeln entschieden: {}", flow.resolved_duplicates));
    if branch {
        let main = TextBox::new(format!("Datenbanken und Register: {}", flow.identified - flow.other_identified));
        let side = TextBox::new(format!("Andere Quellen: {}", flow.other_identified));
        out.push((listed(main, false), Side::In(listed(side, true))));
        out.push((TextBox::new(total), Side::Out(removed)));
    } else {
        out.push((listed(TextBox::new(total), false), Side::Out(removed)));
    }

    for level in &flow.levels {
        if let Some(r) = &level.retrieval {
            let missing = TextBox::new(format!("Nicht beschafft: {}", r.unavailable + r.open))
                .line(format!("nicht beschaffbar: {}", r.unavailable))
                .line_if(r.open > 0, format!("Beschaffung offen: {}", r.open));
            out.push((TextBox::new(format!("Volltext gesucht: {}", r.sought)), Side::Out(missing)));
        }
        let main = TextBox::new(format!("{}: {} geprüft", level.label, level.assessed))
            .line_if(level.rounds > 1, format!("in {} Runden", level.rounds))
            .line_if(level.not_assigned > 0, format!("noch keiner Stufe zugeteilt: {}", level.not_assigned))
            .line_if(
                level.retrieval.as_ref().is_some_and(|r| r.waiting > 0),
                format!("beschafft, noch nicht zugeteilt: {}", level.retrieval.as_ref().map_or(0, |r| r.waiting)),
            )
            .line_if(level.open > 0, format!("noch offen: {}", level.open))
            .line_if(level.blinded > 0, format!("davon in verblindeter Stufe: {}", level.blinded))
            .line_if(branch, format!("aus anderen Quellen: {}", level.other_assessed));
        let mut excluded = TextBox::new(format!("Ausgeschlossen: {}", level.excluded));
        if level.fulltext {
            for reason in &level.reasons {
                excluded = excluded.line(format!("{}: {}", reason_label(reason), reason.n));
            }
        }
        out.push((main, Side::Out(excluded)));
    }

    if let Some(n) = flow.included {
        let sources = flow.included_sources.unwrap_or(n);
        let other = flow.levels.last().map_or(0, |l| l.other_included);
        let mut last =
            TextBox::new(format!("Eingeschlossen: {}", count(sources, &flow.source_term, &flow.sources_term)))
                .line_if(sources != n, format!("{records} darin: {n}"))
                .line_if(branch, format!("{records} aus anderen Quellen: {other}"));
        for kind in &flow.source_types {
            last = last.line(format!("{}: {}", kind.label, kind.n));
        }
        out.push((last.line_if(!flow.complete, "vorläufig, die Auswahl läuft noch".to_string()), Side::None));
    }
    out
}

/// Flussdiagramm als SVG. Linien und Schrift nehmen `currentColor`; ohne Vorgabe ist das Schwarz.
pub fn diagram_svg(flow: &Flow) -> String {
    let rows = rows(flow);
    let side_x = MARGIN + MAIN_WIDTH + COLUMN_GAP;
    let width = side_x + SIDE_WIDTH + MARGIN;
    let height_of = |lines: usize| lines as i64 * LINE + 2 * PADDING;
    let mut body = String::new();
    let mut y = MARGIN;
    let draw = |body: &mut String, x: i64, top: i64, w: i64, lines: &[(String, bool, bool)]| {
        body.push_str(&format!(
            "<rect x=\"{x}\" y=\"{top}\" width=\"{w}\" height=\"{}\" rx=\"4\" fill=\"none\" stroke=\"currentColor\"/>\n",
            height_of(lines.len())
        ));
        for (i, (text, bold, indent)) in lines.iter().enumerate() {
            body.push_str(&format!(
                "<text x=\"{}\" y=\"{}\"{}>{}</text>\n",
                x + PADDING + if *indent { 12 } else { 0 },
                top + PADDING + 13 + i as i64 * LINE,
                if *bold { " font-weight=\"bold\"" } else { "" },
                xml(text)
            ));
        }
    };
    let arrow = |body: &mut String, x1: i64, y1: i64, x2: i64, y2: i64| {
        body.push_str(&format!(
            "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" stroke=\"currentColor\" marker-end=\"url(#pfeil)\"/>\n"
        ));
    };
    let count = rows.len();
    for (i, (main, side)) in rows.iter().enumerate() {
        let main_lines = main.wrapped(MAIN_CHARS);
        let side_lines = side.text().map(|s| s.wrapped(SIDE_CHARS));
        let main_height = height_of(main_lines.len());
        let side_height = side_lines.as_ref().map_or(0, |l| height_of(l.len()));
        let row_height = main_height.max(side_height);
        let main_top = y + (row_height - main_height) / 2;
        draw(&mut body, MARGIN, main_top, MAIN_WIDTH, &main_lines);
        if let Some(lines) = &side_lines {
            let side_top = y + (row_height - side_height) / 2;
            draw(&mut body, side_x, side_top, SIDE_WIDTH, lines);
            let middle = y + row_height / 2;
            match side {
                Side::In(_) => {
                    // Mündet unterhalb der Zeile in den Pfeil der Hauptspalte.
                    let (x, join) = (side_x + SIDE_WIDTH / 2, y + row_height + ROW_GAP / 2);
                    body.push_str(&format!(
                        "<polyline points=\"{x},{} {x},{join} {},{join}\" fill=\"none\" stroke=\"currentColor\" marker-end=\"url(#pfeil)\"/>\n",
                        side_top + side_height,
                        MARGIN + MAIN_WIDTH / 2 + 2
                    ));
                }
                _ => arrow(&mut body, MARGIN + MAIN_WIDTH, middle, side_x - 2, middle),
            }
        }
        if i + 1 < count {
            let next_main = height_of(rows[i + 1].0.wrapped(MAIN_CHARS).len());
            let next_side = rows[i + 1].1.text().map_or(0, |s| height_of(s.wrapped(SIDE_CHARS).len()));
            let next_top = y + row_height + ROW_GAP + (next_main.max(next_side) - next_main) / 2;
            let x = MARGIN + MAIN_WIDTH / 2;
            arrow(&mut body, x, main_top + main_height, x, next_top - 2);
        }
        y += row_height + ROW_GAP;
    }
    let height = y - ROW_GAP + MARGIN;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" width=\"{width}\" height=\"{height}\" \
         font-family=\"Helvetica, Arial, sans-serif\" font-size=\"13\" fill=\"currentColor\" role=\"img\" \
         aria-label=\"Flussdiagramm der Auswahl\">\n\
         <defs><marker id=\"pfeil\" markerWidth=\"8\" markerHeight=\"8\" refX=\"7\" refY=\"4\" orient=\"auto\">\
         <path d=\"M0,0 L8,4 L0,8 z\" fill=\"currentColor\"/></marker></defs>\n{body}</svg>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn level(key: &str, from: Option<&str>, input: i64, assessed: i64, included: i64, excluded: i64) -> Level {
        Level {
            key: key.into(),
            label: key.into(),
            rounds: 1,
            input_from: from.map(str::to_string),
            input,
            retrieval: None,
            not_assigned: input - assessed,
            assessed,
            included,
            excluded,
            open: assessed - included - excluded,
            blinded: 0,
            reasons: vec![ReasonCount { code: "E1".into(), text: "Falsche <Population> & mehr".into(), n: excluded }],
            fulltext: true,
            other_assessed: 0,
            other_included: 0,
        }
    }

    fn flow() -> Flow {
        let source = |name: &str, n: i64| SourceCount {
            source: name.into(),
            origin: "database".into(),
            search_id: None,
            search_version: None,
            n_imports: 1,
            n_records: n,
            n_duplicates: 0,
        };
        Flow {
            sources: vec![source("MEDLINE", 60), source("Embase", 40), source("MEDLINE", 20)],
            identified: 120,
            exact_duplicates: 15,
            resolved_duplicates: 5,
            unique: 100,
            levels: vec![level("ta", None, 100, 100, 30, 70), level("vt", Some("ta"), 30, 30, 12, 18)],
            included: Some(12),
            included_sources: Some(12),
            source_types: Vec::new(),
            other_identified: 0,
            complete: true,
            record_term: "Eintrag".into(),
            records_term: "Einträge".into(),
            source_term: "Quelle".into(),
            sources_term: "Quellen".into(),
        }
    }

    #[test]
    fn a_flow_that_adds_up_has_no_problems_and_a_broken_one_names_the_level() {
        let mut f = flow();
        assert_eq!(f.check(), Vec::<String>::new());
        f.levels[1].excluded = 17;
        let problems = f.check();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].contains("Entscheidungen in „vt“"), "{problems:?}");
        f = flow();
        f.levels[1].input = 31;
        assert!(f.check()[0].contains("Eingang von „vt“"));
        f = flow();
        f.unique = 99;
        assert!(f.check().iter().any(|p| p.contains("ohne Dubletten")));
    }

    #[test]
    fn diagram_names_every_number_and_escapes_text() {
        let svg = diagram_svg(&flow());
        for text in [
            "Identifiziert: 120 Einträge",
            "MEDLINE: 80",
            "Embase: 40",
            "Vor der Auswahl entfernt: 20",
            "ta: 100 geprüft",
            "Ausgeschlossen: 70",
            "Ausgeschlossen: 18",
            "Eingeschlossen: 12 Quellen",
            "E1 Falsche &lt;Population&gt; &amp; mehr: 18",
        ] {
            assert!(svg.contains(text), "{text} fehlt in\n{svg}");
        }
        assert!(!svg.contains("vorläufig"));
        assert_eq!(svg.matches("<rect").count(), 7);
        assert_eq!(svg.matches("<line").count(), 6, "drei nach unten, drei zur Seite");
        let mut open = flow();
        open.complete = false;
        assert!(diagram_svg(&open).contains("vorläufig"));
    }

    #[test]
    fn long_lines_wrap_at_spaces() {
        assert_eq!(wrap("eins zwei drei", 9), ["eins zwei", "drei"]);
        assert_eq!(wrap("Donaudampfschifffahrt ab", 5), ["Donaudampfschifffahrt", "ab"]);
        assert!(wrap("  ", 5).is_empty());
    }

    #[test]
    fn ris_export_is_read_back_by_the_importer() {
        let record = RecordRow {
            id: Uuid::new_v4(),
            batch_id: Uuid::new_v4(),
            title: "Ein Titel\nüber zwei Zeilen".into(),
            abstract_text: "Kurz.".into(),
            authors: "Miller, Anna; Chen, Li".into(),
            year: Some(2021),
            journal: "Journal of Tests".into(),
            doi: "10.1000/abc".into(),
            pmid: "12345".into(),
            url: String::new(),
            keywords: "headache; adult".into(),
            raw: json!({"TY": "CHAP"}),
            dedup_key: String::new(),
            duplicate_of: None,
        };
        let text = to_ris(&[record.clone(), RecordRow { raw: json!({}), ..record }]);
        assert!(text.starts_with("TY  - CHAP\nAU  - Miller, Anna\nAU  - Chen, Li\nTI  - Ein Titel über zwei Zeilen\n"));
        let items = crate::import::parse_ris(&text);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].authors, "Miller, Anna; Chen, Li");
        assert_eq!((items[0].year, items[0].doi.as_str()), (Some(2021), "10.1000/abc"));
        assert_eq!(items[0].keywords, "headache; adult");
        assert_eq!(items[1].raw["TY"], "JOUR");
    }

    #[test]
    fn helpers_format_german_text() {
        assert_eq!(decimal(91.25, 1), "91,2");
        assert_eq!(german_date("2026-11-19"), "19.11.2026");
        assert_eq!(code_block("a ``` b"), "````\na ``` b\n````");
        assert_eq!(count(1, "Runde", "Runden"), "1 Runde");
    }
}
