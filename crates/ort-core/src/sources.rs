//! Quellen: Bündel aus Einträgen derselben Studie und die Quellenart je Eintrag.
//!
//! Ein Eintrag ist ein Treffer aus einer Suche, eine Quelle das, was Bericht und Charting zählen.
//! Begleitpublikationen derselben Studie werden mit `RecordsLinkedToStudy` zu einer Quelle gebündelt;
//! einer der Einträge ist der Haupteintrag. Ohne Bündel ist jeder Eintrag seine eigene Quelle.
//! `SourceTypeSet` gibt Einträgen eine Quellenart; für eine Quelle gilt die ihres Haupteintrags.
//!
//! Beides ist eine Projektion über das Log. Je Bündel gilt das letzte Ereignis. Nennt ein späteres
//! Bündel einen Eintrag, der schon in einem anderen steckt, wandert er dorthin; ein Bündel, dem
//! dadurch weniger als zwei Einträge bleiben, ist aufgelöst.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::catalog::{MAX_LABEL, MAX_PAIRS, MAX_STUDY_RECORDS};
use crate::error::{conflict, invalid, not_found, Result};
use crate::event::Event;
use crate::projection::order_key;
use crate::service::{require, RecordItem, Review, MANAGE_ROLES, ROLES};
use crate::tr;
use crate::workflow::SchemaElement;

pub const SOURCE_TYPES: [&str; 2] = ["RecordsLinkedToStudy", "SourceTypeSet"];

#[derive(Clone, Debug, PartialEq)]
pub struct Study {
    pub id: Uuid,
    pub label: String,
    /// In der Reihenfolge des Ereignisses.
    pub records: Vec<Uuid>,
    pub primary: Uuid,
}

/// Bündel und Quellenarten, aus dem Log berechnet.
#[derive(Clone, Debug, Default)]
pub struct Studies {
    pub list: BTreeMap<Uuid, Study>,
    /// Eintrag → Bündel.
    pub of_record: BTreeMap<Uuid, Uuid>,
    /// Eintrag → Quellenart; ohne Angabe fehlt der Eintrag.
    pub types: BTreeMap<Uuid, String>,
}

fn ids(payload: &Value, key: &str) -> Vec<Uuid> {
    let list = payload.get(key).and_then(Value::as_array).into_iter().flatten();
    list.filter_map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok())).collect()
}

impl Studies {
    pub fn build(events: &[Event]) -> Studies {
        let mut sorted: Vec<&Event> = events.iter().filter(|e| SOURCE_TYPES.contains(&e.event_type.as_str())).collect();
        sorted.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
        let mut out = Studies::default();
        for e in sorted {
            if e.event_type == "SourceTypeSet" {
                let kind = e.payload_str("source_type").unwrap_or_default().trim().to_string();
                for id in ids(&e.payload, "records") {
                    match kind.is_empty() {
                        true => out.types.remove(&id),
                        false => out.types.insert(id, kind.clone()),
                    };
                }
                continue;
            }
            let Some(study_id) = e.payload_str("study_id").and_then(|s| Uuid::parse_str(s).ok()) else { continue };
            let records = ids(&e.payload, "records");
            let primary = e.payload_str("primary").and_then(|s| Uuid::parse_str(s).ok());
            out.remove(&study_id);
            let Some(primary) = primary.filter(|p| records.len() >= 2 && records.contains(p)) else { continue };
            for id in &records {
                out.take(id);
            }
            for id in &records {
                out.of_record.insert(*id, study_id);
            }
            let label = e.payload_str("label").unwrap_or_default().trim().to_string();
            out.list.insert(study_id, Study { id: study_id, label, records, primary });
        }
        out
    }

    fn remove(&mut self, study_id: &Uuid) {
        if let Some(study) = self.list.remove(study_id) {
            for id in study.records {
                self.of_record.remove(&id);
            }
        }
    }

    /// Nimmt einen Eintrag aus seinem bisherigen Bündel. Bleibt dort nur einer, ist es aufgelöst;
    /// war es der Haupteintrag, rückt der erste verbliebene nach.
    fn take(&mut self, record: &Uuid) {
        let Some(study_id) = self.of_record.remove(record) else { return };
        let Some(study) = self.list.get_mut(&study_id) else { return };
        study.records.retain(|r| r != record);
        if study.records.len() < 2 {
            self.remove(&study_id);
        } else if study.primary == *record {
            study.primary = study.records[0];
        }
    }

    pub fn study_of(&self, record: &Uuid) -> Option<&Study> {
        self.of_record.get(record).and_then(|id| self.list.get(id))
    }
}

/// Eine Quelle vor dem Nachschlagen der Einträge.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    /// Haupteintrag; bei einem Bündel, dessen Haupteintrag nicht eingeschlossen ist, der erste
    /// eingeschlossene.
    pub primary: Uuid,
    pub study: Option<Uuid>,
    /// Alle Einträge der Quelle, der Haupteintrag zuerst; bei einem Bündel auch nicht eingeschlossene.
    pub records: Vec<Uuid>,
    pub source_type: String,
}

/// Fasst eingeschlossene Einträge zu Quellen zusammen, in der Reihenfolge von `included`.
pub fn units(included: &[Uuid], studies: &Studies) -> Vec<Unit> {
    let set: BTreeSet<&Uuid> = included.iter().collect();
    let mut seen: BTreeSet<Uuid> = BTreeSet::new();
    let mut out = Vec::new();
    for id in included {
        let Some(study) = studies.study_of(id) else {
            let source_type = studies.types.get(id).cloned().unwrap_or_default();
            out.push(Unit { primary: *id, study: None, records: vec![*id], source_type });
            continue;
        };
        if !seen.insert(study.id) {
            continue;
        }
        let primary = match set.contains(&study.primary) {
            true => study.primary,
            false => study.records.iter().copied().find(|r| set.contains(r)).unwrap_or(*id),
        };
        let mut records = vec![primary];
        records.extend(study.records.iter().copied().filter(|r| *r != primary));
        let source_type = records.iter().find_map(|r| studies.types.get(r)).cloned().unwrap_or_default();
        out.push(Unit { primary, study: Some(study.id), records, source_type });
    }
    out
}

// ---------------------------------------------------------------- Ansicht

#[derive(Clone, Debug, Serialize)]
pub struct SourceRecord {
    #[serde(flatten)]
    pub record: RecordItem,
    pub primary: bool,
    /// Ob der Eintrag am Ende der Auswahl eingeschlossen ist. Ein Bündel kann auch andere nennen.
    pub included: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceView {
    /// ID des Haupteintrags.
    pub id: Uuid,
    pub study_id: Option<Uuid>,
    pub label: String,
    pub source_type: String,
    pub records: Vec<SourceRecord>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TypeCount {
    /// Schlüssel der Quellenart; leer für „ohne Quellenart“.
    pub key: String,
    pub label: String,
    pub n: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceList {
    pub sources: Vec<SourceView>,
    /// Quellenarten der Vorlage; ohne Liste ist die Quellenart freier Text.
    pub source_types: Vec<SchemaElement>,
    /// Eingeschlossene Einträge und was nach dem Bündeln als Quelle zählt.
    pub n_records: i64,
    pub n_sources: i64,
    pub n_without_type: i64,
    pub by_type: Vec<TypeCount>,
    /// Station oder Stufe, aus der die Eingeschlossenen stammen.
    pub level: Option<String>,
    /// Ob die Auswahl abgeschlossen ist; sonst können noch Quellen dazukommen.
    pub complete: bool,
}

/// Quellen je Quellenart in der Reihenfolge der Vorlage, danach freie Angaben, zuletzt die ohne.
pub fn count_types(units: &[Unit], known: &[SchemaElement]) -> Vec<TypeCount> {
    let mut counts: BTreeMap<&str, i64> = BTreeMap::new();
    for unit in units {
        *counts.entry(unit.source_type.as_str()).or_default() += 1;
    }
    let mut out = Vec::new();
    for element in known {
        if let Some(n) = counts.remove(element.key.as_str()) {
            out.push(TypeCount { key: element.key.clone(), label: element.label.clone(), n });
        }
    }
    let without = counts.remove("");
    for (key, n) in counts {
        out.push(TypeCount { key: key.to_string(), label: key.to_string(), n });
    }
    if let Some(n) = without {
        out.push(TypeCount { key: String::new(), label: tr!("ohne Quellenart"), n });
    }
    out
}

impl Review {
    pub(crate) fn studies(&self) -> Result<Studies> {
        Ok(Studies::build(&self.store.events_of_types(&SOURCE_TYPES)?))
    }

    /// Eingeschlossene Quellen am Ende der Auswahl: Bündel und einzelne Einträge mit Quellenart.
    pub fn sources(&self) -> Result<SourceList> {
        require(&self.state()?, &self.me.actor_id, &ROLES)?;
        let selection = self.selection()?;
        let studies = self.studies()?;
        let known = self.workflow()?.template.localized().source_types;
        let units = units(&selection.included, &studies);
        let included: BTreeSet<&Uuid> = selection.included.iter().collect();
        let mut sources = Vec::new();
        for unit in &units {
            let mut records = Vec::new();
            for id in &unit.records {
                let Some(row) = self.store.record(id)? else { continue };
                records.push(SourceRecord {
                    record: row.into(),
                    primary: *id == unit.primary,
                    included: included.contains(id),
                });
            }
            let label = unit.study.and_then(|s| studies.list.get(&s)).map(|s| s.label.clone()).unwrap_or_default();
            sources.push(SourceView {
                id: unit.primary,
                study_id: unit.study,
                label,
                source_type: unit.source_type.clone(),
                records,
            });
        }
        // Gleiche Erstautoren stehen nebeneinander; so fallen Begleitpublikationen auf.
        sources.sort_by_cached_key(|s| {
            let first = s.records.first();
            (
                first.map(|r| r.record.authors.to_lowercase()).unwrap_or_default(),
                first.and_then(|r| r.record.year),
                first.map(|r| r.record.title.to_lowercase()).unwrap_or_default(),
                s.id,
            )
        });
        Ok(SourceList {
            n_records: selection.included.len() as i64,
            n_sources: units.len() as i64,
            n_without_type: units.iter().filter(|u| u.source_type.is_empty()).count() as i64,
            by_type: count_types(&units, &known),
            source_types: known,
            level: selection.level,
            complete: selection.complete,
            sources,
        })
    }

    /// Bündelt Einträge zu einer Quelle. Mit `study_id` wird ein bestehendes Bündel ersetzt.
    pub fn link_study(&self, study_id: Option<Uuid>, label: &str, records: &[Uuid], primary: &Uuid) -> Result<Uuid> {
        require(&self.state()?, &self.me.actor_id, &MANAGE_ROLES)?;
        let label = label.trim();
        if label.chars().count() > MAX_LABEL {
            return Err(invalid(tr!("Die Bezeichnung hat höchstens {} Zeichen.", MAX_LABEL)));
        }
        let unique: BTreeSet<&Uuid> = records.iter().collect();
        if unique.len() != records.len() {
            return Err(invalid(tr!("Ein Eintrag ist doppelt gewählt.")));
        }
        if records.len() < 2 {
            return Err(invalid(tr!("Ein Bündel braucht mindestens zwei Einträge.")));
        }
        if records.len() > MAX_STUDY_RECORDS {
            return Err(invalid(tr!("Ein Bündel hat höchstens {} Einträge.", MAX_STUDY_RECORDS)));
        }
        if !records.contains(primary) {
            return Err(invalid(tr!("Der Haupteintrag muss einer der Einträge des Bündels sein.")));
        }
        let studies = self.studies()?;
        if study_id.is_some_and(|id| !studies.list.contains_key(&id)) {
            return Err(not_found(tr!("Dieses Bündel gibt es nicht.")));
        }
        let resolutions = self.resolutions()?;
        for id in records {
            let record = self.store.record(id)?.ok_or_else(|| not_found(tr!("Eintrag nicht gefunden.")))?;
            let title: String = record.title.chars().take(60).collect();
            if record.duplicate_of.is_some() || resolutions.is_removed(id) {
                return Err(invalid(tr!("„{}“ ist als Dublette entfernt und kann nicht gebündelt werden.", title)));
            }
            if studies.of_record.get(id).is_some_and(|other| Some(*other) != study_id) {
                return Err(conflict(tr!("„{}“ gehört schon zu einem anderen Bündel. Erst dort lösen.", title)));
            }
        }
        let study_id = study_id.unwrap_or_else(Uuid::new_v4);
        self.append(
            "RecordsLinkedToStudy",
            None,
            json!({
                "study_id": study_id.to_string(), "label": label,
                "records": records.iter().map(Uuid::to_string).collect::<Vec<_>>(),
                "primary": primary.to_string(),
            }),
        )?;
        Ok(study_id)
    }

    /// Löst ein Bündel auf; seine Einträge zählen wieder einzeln.
    pub fn dissolve_study(&self, study_id: &Uuid) -> Result<()> {
        require(&self.state()?, &self.me.actor_id, &MANAGE_ROLES)?;
        if !self.studies()?.list.contains_key(study_id) {
            return Err(not_found(tr!("Dieses Bündel gibt es nicht.")));
        }
        self.append(
            "RecordsLinkedToStudy",
            None,
            json!({"study_id": study_id.to_string(), "label": "", "records": [], "primary": null}),
        )?;
        Ok(())
    }

    /// Setzt die Quellenart für Einträge; ein leerer Text nimmt sie zurück. Nennt die Vorlage
    /// Quellenarten, muss es eine davon sein.
    pub fn set_source_type(&self, records: &[Uuid], source_type: &str) -> Result<usize> {
        require(&self.state()?, &self.me.actor_id, &MANAGE_ROLES)?;
        let source_type = source_type.trim();
        if records.is_empty() {
            return Err(invalid(tr!("Kein Eintrag gewählt.")));
        }
        if source_type.chars().count() > MAX_LABEL {
            return Err(invalid(tr!("Die Quellenart hat höchstens {} Zeichen.", MAX_LABEL)));
        }
        let known = self.workflow()?.template.source_types;
        if !source_type.is_empty() && !known.is_empty() && !known.iter().any(|k| k.key == source_type) {
            return Err(invalid(tr!("Diese Quellenart kennt die Vorlage nicht.")));
        }
        let mut unique: Vec<Uuid> = Vec::new();
        for id in records {
            if self.store.record(id)?.is_none() {
                return Err(not_found(tr!("Eintrag nicht gefunden.")));
            }
            if !unique.contains(id) {
                unique.push(*id);
            }
        }
        for chunk in unique.chunks(MAX_PAIRS) {
            let list: Vec<String> = chunk.iter().map(Uuid::to_string).collect();
            self.append("SourceTypeSet", None, json!({"records": list, "source_type": source_type}))?;
        }
        Ok(unique.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Identity;

    fn log(payloads: Vec<(&str, Value)>) -> Vec<Event> {
        let review = Review::create_in_memory(Identity::generate("Leitung").unwrap(), "Test").unwrap();
        for (kind, payload) in payloads {
            review.append(kind, None, payload).unwrap();
        }
        review.events().unwrap()
    }

    fn link(study: Uuid, records: &[Uuid], primary: Option<Uuid>) -> (&'static str, Value) {
        let records: Vec<String> = records.iter().map(Uuid::to_string).collect();
        (
            "RecordsLinkedToStudy",
            json!({"study_id": study.to_string(), "label": "", "records": records, "primary": primary.map(|p| p.to_string())}),
        )
    }

    #[test]
    fn later_bundle_takes_a_record_and_dissolves_what_is_left_alone() {
        let r: Vec<Uuid> = (0..5).map(|_| Uuid::new_v4()).collect();
        let (s1, s2) = (Uuid::new_v4(), Uuid::new_v4());
        let studies = Studies::build(&log(vec![link(s1, &r[0..3], Some(r[0])), link(s2, &[r[0], r[3]], Some(r[3]))]));
        // r0 ist gewandert; im ersten Bündel rückt r1 als Haupteintrag nach.
        assert_eq!(studies.list[&s1].records, vec![r[1], r[2]]);
        assert_eq!(studies.list[&s1].primary, r[1]);
        assert_eq!(studies.of_record[&r[0]], s2);

        let s3 = Uuid::new_v4();
        let studies = Studies::build(&log(vec![
            link(s1, &r[0..2], Some(r[0])),
            link(s3, &[r[1], r[4]], Some(r[4])),
            link(s3, &[], None),
        ]));
        assert!(studies.list.is_empty(), "zu zweit gebündelt, einer genommen, das zweite Bündel aufgelöst");
        assert!(studies.of_record.is_empty());
    }

    #[test]
    fn a_bundle_counts_as_one_source_with_the_type_of_its_primary() {
        let r: Vec<Uuid> = (0..4).map(|_| Uuid::new_v4()).collect();
        let study = Uuid::new_v4();
        let ids = |list: &[Uuid]| list.iter().map(Uuid::to_string).collect::<Vec<_>>();
        let studies = Studies::build(&log(vec![
            link(study, &[r[0], r[1], r[3]], Some(r[3])),
            ("SourceTypeSet", json!({"records": ids(&r[0..3]), "source_type": "review"})),
            ("SourceTypeSet", json!({"records": ids(&r[2..3]), "source_type": ""})),
        ]));
        // r3 ist Haupteintrag, aber nicht eingeschlossen: r0 vertritt die Quelle.
        let found = units(&r[0..3], &studies);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].primary, found[0].study), (r[0], Some(study)));
        assert_eq!(found[0].records, vec![r[0], r[1], r[3]]);
        assert_eq!(found[0].source_type, "review");
        assert_eq!((found[1].primary, found[1].source_type.as_str()), (r[2], ""));

        let known = vec![SchemaElement { key: "review".into(), label: "Review".into() }];
        let counts = count_types(&found, &known);
        assert_eq!(
            counts.iter().map(|c| (c.label.as_str(), c.n)).collect::<Vec<_>>(),
            [("Review", 1), ("ohne Quellenart", 1)]
        );
    }
}
