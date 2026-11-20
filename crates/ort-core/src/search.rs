//! Suchen eines Reviews: je Quelle eine Suche mit Versionen (Suchstring, Datum, Grenzen) und
//! Vermerken über ein Peer-Review. Ein Import nennt die Suchversion, aus der seine Treffer stammen.

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::event::Event;
use crate::projection::order_key;

pub const SEARCH_TYPES: [&str; 2] = ["SearchVersionPublished", "SearchPeerReviewed"];

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PeerReview {
    pub reviewed_on: String,
    pub note: String,
    /// Mitglied, das geprüft hat. Leer, wenn die Person nicht zum Team gehört; dann steht die Rolle im Vermerk.
    pub reviewer: Option<Uuid>,
    pub recorded_by: Uuid,
}

/// Eine Zeile der Suche mit eigener Trefferzahl: ein Begriff, ein Block oder eine Verknüpfung.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchLine {
    /// Wie die Zeile in der Quelle heißt, z. B. `#3` oder `S3`; darf leer sein.
    pub label: String,
    pub query: String,
    pub hits: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SearchVersion {
    pub version: i64,
    pub previous_version: Option<i64>,
    /// Datenbank, Register oder andere Quelle, z. B. MEDLINE.
    pub source: String,
    /// Oberfläche, über die gesucht wurde, z. B. Ovid.
    pub platform: String,
    /// `database`, `register`, `citation` oder `other`.
    pub origin: String,
    pub query: String,
    /// Tag der Suche als JJJJ-MM-TT.
    pub searched_on: String,
    pub limits: String,
    /// Obergrenze übernommener Treffer, z. B. die ersten 200 einer Websuche.
    pub hit_limit: Option<i64>,
    /// Trefferzahl, wie die Quelle sie gemeldet hat.
    pub hits: Option<i64>,
    /// Die Suche Zeile für Zeile, je mit Trefferzahl; leer, wenn nur das Gesamtergebnis festgehalten ist.
    pub lines: Vec<SearchLine>,
    pub change_note: String,
    pub published_at: String,
    pub peer_reviews: Vec<PeerReview>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Search {
    pub id: Uuid,
    pub versions: Vec<SearchVersion>,
}

impl Search {
    pub fn latest(&self) -> Option<&SearchVersion> {
        self.versions.last()
    }

    pub fn version(&self, version: i64) -> Option<&SearchVersion> {
        self.versions.iter().find(|v| v.version == version)
    }
}

fn lines(payload: &Value) -> Vec<SearchLine> {
    let text = |line: &Value, key: &str| line.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    payload
        .get("lines")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|line| SearchLine {
            label: text(line, "label"),
            query: text(line, "query"),
            hits: line.get("hits").and_then(Value::as_i64),
        })
        .collect()
}

/// Alle Suchen in der Reihenfolge ihrer ersten Veröffentlichung.
#[derive(Clone, Debug, Default)]
pub struct Searches {
    pub list: Vec<Search>,
}

impl Searches {
    pub fn build(events: &[Event]) -> Searches {
        let mut sorted: Vec<&Event> = events.iter().filter(|e| SEARCH_TYPES.contains(&e.event_type.as_str())).collect();
        sorted.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
        let mut out = Searches::default();
        let text = |e: &Event, key: &str| e.payload_str(key).unwrap_or_default().to_string();
        let number = |e: &Event, key: &str| e.payload.get(key).and_then(Value::as_i64);
        for e in sorted {
            let Some(id) = e.payload_str("search_id").and_then(|s| Uuid::parse_str(s).ok()) else { continue };
            let Some(version) = number(e, "version") else { continue };
            match e.event_type.as_str() {
                "SearchVersionPublished" => {
                    let entry = SearchVersion {
                        version,
                        previous_version: number(e, "previous_version"),
                        source: text(e, "source"),
                        platform: text(e, "platform"),
                        origin: text(e, "origin"),
                        query: text(e, "query"),
                        searched_on: text(e, "searched_on"),
                        limits: text(e, "limits"),
                        hit_limit: number(e, "hit_limit"),
                        hits: number(e, "hits"),
                        lines: lines(&e.payload),
                        change_note: text(e, "change_note"),
                        published_at: e.occurred_at.clone(),
                        peer_reviews: Vec::new(),
                    };
                    let index = match out.list.iter().position(|s| s.id == id) {
                        Some(index) => index,
                        None => {
                            out.list.push(Search { id, versions: Vec::new() });
                            out.list.len() - 1
                        }
                    };
                    let versions = &mut out.list[index].versions;
                    // Bei gleicher Nummer gilt die später veröffentlichte Fassung.
                    versions.retain(|v| v.version != version);
                    versions.push(entry);
                    versions.sort_by_key(|v| v.version);
                }
                _ => {
                    let target = out.list.iter_mut().find(|s| s.id == id);
                    if let Some(v) = target.and_then(|s| s.versions.iter_mut().find(|v| v.version == version)) {
                        v.peer_reviews.push(PeerReview {
                            reviewed_on: text(e, "reviewed_on"),
                            note: text(e, "note"),
                            reviewer: e.payload_str("reviewer").and_then(|s| Uuid::parse_str(s).ok()),
                            recorded_by: e.actor_id,
                        });
                    }
                }
            }
        }
        out
    }

    pub fn get(&self, id: &Uuid) -> Option<&Search> {
        self.list.iter().find(|s| s.id == *id)
    }

    /// Alle bekannten Paare aus Suche und Version, für die Prüfung beim Einlesen.
    pub fn known(&self) -> BTreeSet<(Uuid, i64)> {
        self.list.iter().flat_map(|s| s.versions.iter().map(|v| (s.id, v.version))).collect()
    }
}
