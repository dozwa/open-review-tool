//! Volltexte: Status je Eintrag, Abgleich mit einem Ordner und mit Zotero, Lesen mit Prüfsumme.
//!
//! Die Dateien liegen außerhalb der App, in einem verknüpften Ordner oder in Zotero. Im Log stehen nur
//! Prüfsumme, Herkunft und relativer Pfad oder Zotero-Schlüssel (`FullTextStatusSet`). Wo der Ordner
//! auf diesem Rechner liegt, ist eine lokale Einstellung (`meta`) und steht nicht im Log. Die App liest
//! die Dateien nur; sie ändert, verschiebt und kopiert nichts.
//!
//! Vorschläge für Zuordnungen sind eine Projektion wie die Dublettenkandidaten: Sie werden bei jedem
//! Aufruf neu berechnet, im Log steht nur die bestätigte Zuordnung.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::catalog;
use crate::duplicates::{first_author, normalize_title, similarity};
use crate::error::{integrity, invalid, not_found, Error, Result};
use crate::event::{sha256_hex, Event};
use crate::import::normalize_doi;
use crate::projection::{order_key, StageState};
use crate::service::{require, Review, MANAGE_ROLES, ROLES, WRITE_ROLES};
use crate::store::RecordRow;
use crate::workflow::Workflow;

pub const FULLTEXT_TYPES: [&str; 3] = ["FullTextStatusSet", "ZoteroLibraryLinked", "ZoteroItemMapped"];
const FOLDER_KEY: &str = "fulltext_folder";
/// Tiefe und Zahl der Dateien, bis zu der ein Ordner durchsucht wird.
const MAX_DEPTH: usize = 8;
const MAX_FILES: usize = 20_000;
/// So viel vom Anfang und Ende einer PDF wird nach einer DOI in den Metadaten durchsucht.
const HEAD_BYTES: usize = 256 * 1024;
const TAIL_BYTES: usize = 64 * 1024;
/// Ab dieser Ähnlichkeit von Dateiname und Titel wird eine Zuordnung vorgeschlagen.
const TITLE_MIN: i64 = 85;
/// Kürzere Namensteile sagen über den Titel nichts aus.
const MIN_NAME: usize = 12;

// ---------------------------------------------------------------- Projektion

/// Stand eines Volltexts aus dem letzten `FullTextStatusSet` zum Eintrag.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FullText {
    /// `retrieved` oder `unavailable`.
    pub status: String,
    /// `folder` oder `zotero`; leer bei `unavailable`.
    pub origin: String,
    /// Pfad relativ zum verknüpften Ordner.
    pub path: String,
    /// Schlüssel des Anhangs in Zotero.
    pub zotero_key: String,
    pub sha256: String,
    pub size: i64,
    pub reason: String,
    pub set_by: Uuid,
    pub set_at: String,
}

/// Verknüpfte Zotero-Bibliothek oder Collection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ZoteroLink {
    /// `user` (eigene Bibliothek, nur auf dem Rechner der verknüpfenden Person) oder `group`.
    pub library_type: String,
    pub library_id: i64,
    #[serde(default)]
    pub collection_key: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub struct FullTexts {
    pub status: BTreeMap<Uuid, FullText>,
    pub zotero: Option<ZoteroLink>,
    /// Eintrag → Zotero-Eintrag.
    pub items: BTreeMap<Uuid, String>,
}

impl FullTexts {
    pub fn build(events: &[Event]) -> FullTexts {
        let mut sorted: Vec<&Event> =
            events.iter().filter(|e| FULLTEXT_TYPES.contains(&e.event_type.as_str())).collect();
        sorted.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
        let mut out = FullTexts::default();
        let id_of = |v: &Value| v.get("record_id").and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok());
        for e in sorted {
            let text = |key: &str| e.payload_str(key).unwrap_or_default().to_string();
            match e.event_type.as_str() {
                "FullTextStatusSet" => {
                    let Some(id) = id_of(&e.payload) else { continue };
                    match e.payload_str("status") {
                        Some(status @ ("retrieved" | "unavailable")) => {
                            let entry = FullText {
                                status: status.to_string(),
                                origin: text("origin"),
                                path: text("path"),
                                zotero_key: text("zotero_key"),
                                sha256: text("sha256"),
                                size: e.payload.get("size").and_then(Value::as_i64).unwrap_or(0),
                                reason: text("reason"),
                                set_by: e.actor_id,
                                set_at: e.occurred_at.clone(),
                            };
                            out.status.insert(id, entry);
                        }
                        _ => {
                            out.status.remove(&id);
                        }
                    }
                }
                "ZoteroLibraryLinked" => out.zotero = serde_json::from_value(e.payload.clone()).ok(),
                _ => {
                    for item in e.payload.get("items").and_then(Value::as_array).into_iter().flatten() {
                        let Some(id) = id_of(item) else { continue };
                        match item.get("item_key").and_then(Value::as_str) {
                            Some(key) => out.items.insert(id, key.to_string()),
                            None => out.items.remove(&id),
                        };
                    }
                }
            }
        }
        out
    }

    pub fn is_retrieved(&self, id: &Uuid) -> bool {
        self.status.get(id).is_some_and(|f| f.status == "retrieved")
    }

    /// Zahl der beschafften und der nicht beschaffbaren unter den genannten Einträgen.
    pub fn counts(&self, needed: &BTreeSet<Uuid>) -> (i64, i64) {
        let count = |status: &str| {
            needed.iter().filter(|id| self.status.get(id).is_some_and(|f| f.status == status)).count() as i64
        };
        (count("retrieved"), count("unavailable"))
    }
}

// ---------------------------------------------------------------- Ordner

/// PDF-Datei im verknüpften Ordner.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderFile {
    /// Pfad relativ zum Ordner, mit `/` getrennt.
    pub path: String,
    pub size: i64,
    /// DOI aus den Metadaten der Datei, normalisiert.
    pub doi: String,
}

fn io(error: std::io::Error) -> Error {
    Error::Io(error)
}

/// Alle PDF-Dateien unterhalb von `root`, nach Pfad sortiert. Versteckte Dateien und Ordner bleiben aus.
pub fn scan_folder(root: &Path) -> Result<Vec<FolderFile>> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new(), 0usize)];
    while let Some((dir, prefix, depth)) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(io)? {
            let entry = entry.map_err(io)?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let relative = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            // `metadata` folgt Verknüpfungen; eine kaputte Verknüpfung wird übergangen.
            let Ok(meta) = std::fs::metadata(entry.path()) else { continue };
            if meta.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((entry.path(), relative, depth + 1));
                }
            } else if name.to_lowercase().ends_with(".pdf") && catalog::is_relative_path(&relative) {
                out.push(FolderFile { path: relative, size: meta.len() as i64, doi: String::new() });
                if out.len() >= MAX_FILES {
                    return Err(invalid(format!("Der Ordner enthält mehr als {MAX_FILES} PDF-Dateien.")));
                }
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// DOI aus den Metadaten einer PDF: die erste Angabe nach dem Wort „doi“ am Anfang oder Ende der Datei.
/// Liest die Datei nicht ganz und entpackt nichts; komprimierte Metadaten bleiben unerkannt.
pub fn doi_in_pdf(path: &Path) -> String {
    let Ok(mut file) = std::fs::File::open(path) else { return String::new() };
    let mut head = Vec::new();
    if file.by_ref().take(HEAD_BYTES as u64).read_to_end(&mut head).is_err() {
        return String::new();
    }
    let mut rest = Vec::new();
    let _ = file.read_to_end(&mut rest);
    let tail = &rest[rest.len().saturating_sub(TAIL_BYTES)..];
    doi_in_bytes(&head).or_else(|| doi_in_bytes(tail)).unwrap_or_default()
}

fn doi_in_bytes(data: &[u8]) -> Option<String> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(
            r#"(?i-u)doi[^0-9A-Za-z]{0,24}?(?:https?://(?:dx\.)?doi\.org/)?(10\.[0-9]{4,9}/[^\s<>"'\\\]\[)(]{2,120})"#,
        )
        .expect("fester Ausdruck")
    });
    let found = pattern.captures(data)?.get(1)?;
    let text = String::from_utf8_lossy(found.as_bytes());
    Some(normalize_doi(text.trim_end_matches(['.', ',', ';'])))
}

/// Vorschlag, eine Datei einem Eintrag zuzuordnen.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Suggestion {
    pub record_id: Uuid,
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    pub path: String,
    pub size: i64,
    /// `doi_metadata`, `doi_filename`, `title` oder `author_year`.
    pub basis: &'static str,
    /// Ähnlichkeit von Dateiname und Titel in Prozent; 100 bei gleicher DOI, 0 bei Autor und Jahr.
    pub similarity: i64,
}

fn alnum(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).filter(|c| c.is_alphanumeric()).collect()
}

/// Name ohne Ordner und Endung.
fn stem(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// Wie gut ein Dateiname zum Titel passt. Dateinamen kürzen den Titel oft ab und stellen Autor und Jahr
/// voran („Autor - 2020 - Titel“); verglichen wird deshalb jeder Namensteil mit dem gleich langen
/// Anfang des Titels.
fn title_match(stem: &str, title: &[char]) -> Option<i64> {
    if title.len() < MIN_NAME {
        return None;
    }
    let whole = normalize_title(stem);
    let mut parts: Vec<Vec<char>> = stem.split(" - ").map(normalize_title).collect();
    parts.push(whole.clone());
    let mut best: Option<i64> = None;
    for part in parts.iter().filter(|p| p.len() >= MIN_NAME) {
        let start = &title[..title.len().min(part.len())];
        // Der Name darf den Titel abkürzen, aber nicht nur ein kurzes Stück davon sein.
        if 2 * start.len() < title.len().min(60) {
            continue;
        }
        if let Some(found) = similarity(part, start, TITLE_MIN) {
            best = best.max(Some(found));
        }
    }
    // Titel irgendwo im Namen, z. B. hinter einer laufenden Nummer.
    if best.is_none() && title.len() >= 20 {
        let needle: String = title.iter().take(40).collect();
        if whole.iter().collect::<String>().contains(&needle) {
            best = Some(95);
        }
    }
    best
}

/// Vorschläge für offene Einträge aus den noch nicht zugeordneten Dateien: zuerst gleiche DOI, dann der
/// Titel im Dateinamen, zuletzt Autor und Jahr, wenn beides nur auf einen Eintrag passt. Jede Datei und
/// jeder Eintrag kommt höchstens einmal vor.
pub fn suggest(records: &[RecordRow], files: &[FolderFile]) -> Vec<Suggestion> {
    struct Key<'a> {
        row: &'a RecordRow,
        doi: String,
        title: Vec<char>,
        author_year: String,
    }
    let keys: Vec<Key> = records
        .iter()
        .map(|row| Key {
            row,
            doi: alnum(&row.doi),
            title: normalize_title(&row.title),
            author_year: match (first_author(&row.authors), row.year) {
                (author, Some(year)) if author.chars().count() >= 3 => format!("{author}{year}"),
                _ => String::new(),
            },
        })
        .collect();
    let mut by_author_year: BTreeMap<&str, usize> = BTreeMap::new();
    for key in keys.iter().filter(|k| !k.author_year.is_empty()) {
        *by_author_year.entry(key.author_year.as_str()).or_default() += 1;
    }
    // (Rang, Ähnlichkeit, Datei, Eintrag); kleinerer Rang ist sicherer.
    let mut found: Vec<(u8, i64, usize, usize, &'static str)> = Vec::new();
    for (f, file) in files.iter().enumerate() {
        let name = stem(&file.path);
        let plain = alnum(name);
        let file_doi = alnum(&file.doi);
        for (r, key) in keys.iter().enumerate() {
            if !key.doi.is_empty() && key.doi == file_doi {
                found.push((0, 100, f, r, "doi_metadata"));
            } else if key.doi.len() >= 8 && plain.contains(&key.doi) {
                found.push((1, 100, f, r, "doi_filename"));
            } else if !file_doi.is_empty() && !key.doi.is_empty() {
                // Zwei verschiedene DOIs: Der Titel allein überzeugt dann nicht.
            } else if let Some(percent) = title_match(name, &key.title) {
                found.push((2, percent, f, r, "title"));
            } else if !key.author_year.is_empty()
                && by_author_year.get(key.author_year.as_str()) == Some(&1)
                && plain.starts_with(&key.author_year)
            {
                found.push((3, 0, f, r, "author_year"));
            }
        }
    }
    found.sort_by_key(|a| (a.0, -a.1, a.2, a.3));
    let (mut used_files, mut used_records) = (BTreeSet::new(), BTreeSet::new());
    let mut out = Vec::new();
    for (_, percent, f, r, basis) in found {
        if used_files.contains(&f) || used_records.contains(&r) {
            continue;
        }
        used_files.insert(f);
        used_records.insert(r);
        let row = keys[r].row;
        out.push(Suggestion {
            record_id: row.id,
            title: row.title.clone(),
            authors: row.authors.clone(),
            year: row.year,
            path: files[f].path.clone(),
            size: files[f].size,
            basis,
            similarity: percent,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

// ---------------------------------------------------------------- Zotero

/// Eintrag einer Zotero-Bibliothek, wie die App ihn über die lokale Schnittstelle liest.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZoteroItem {
    pub key: String,
    pub title: String,
    pub doi: String,
    pub year: Option<i64>,
    /// Nachname der ersten Autorin oder des ersten Autors.
    pub first_author: String,
    /// Schlüssel des PDF-Anhangs; leer, wenn der Eintrag keine PDF hat.
    pub attachment_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ZoteroSuggestion {
    pub record_id: Uuid,
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    pub item_key: String,
    pub item_title: String,
    pub attachment_key: Option<String>,
    /// `doi` oder `title`.
    pub basis: &'static str,
    pub similarity: i64,
}

/// Bestätigte Zuordnung eines Eintrags zu einem Zotero-Eintrag. Mit `file` (Pfad der PDF auf diesem
/// Rechner, von Zotero gemeldet) gilt der Volltext als beschafft.
#[derive(Clone, Debug, Deserialize)]
pub struct ZoteroConfirm {
    pub record_id: Uuid,
    pub item_key: String,
    #[serde(default)]
    pub attachment_key: Option<String>,
    #[serde(default)]
    pub file: Option<PathBuf>,
}

/// Ordnet Einträge Zotero-Einträgen zu: gleiche DOI, sonst gleicher Titel (ab 95 %) bei passendem Jahr.
pub fn match_zotero(records: &[RecordRow], items: &[ZoteroItem]) -> Vec<ZoteroSuggestion> {
    let by_doi: BTreeMap<String, &ZoteroItem> =
        items.iter().filter(|i| !i.doi.trim().is_empty()).map(|i| (normalize_doi(&i.doi), i)).collect();
    let titles: Vec<(Vec<char>, &ZoteroItem)> = items.iter().map(|i| (normalize_title(&i.title), i)).collect();
    let mut used: BTreeSet<&str> = BTreeSet::new();
    let mut out = Vec::new();
    for row in records {
        let doi = normalize_doi(&row.doi);
        let mut hit = by_doi.get(&doi).filter(|_| !doi.is_empty()).map(|i| (*i, "doi", 100));
        if hit.is_none() {
            let title = normalize_title(&row.title);
            hit = titles
                .iter()
                .filter(|(t, i)| {
                    t.len() >= MIN_NAME
                        && (i.doi.trim().is_empty() || doi.is_empty())
                        && match (row.year, i.year) {
                            (Some(a), Some(b)) => (a - b).abs() <= 1,
                            _ => true,
                        }
                })
                .filter_map(|(t, i)| similarity(t, &title, 95).map(|percent| (*i, "title", percent)))
                .max_by_key(|(i, _, percent)| (*percent, i.attachment_key.is_some()));
        }
        let Some((item, basis, percent)) = hit else { continue };
        if !used.insert(item.key.as_str()) {
            continue;
        }
        out.push(ZoteroSuggestion {
            record_id: row.id,
            title: row.title.clone(),
            authors: row.authors.clone(),
            year: row.year,
            item_key: item.key.clone(),
            item_title: item.title.clone(),
            attachment_key: item.attachment_key.clone(),
            basis,
            similarity: percent,
        });
    }
    out
}

// ---------------------------------------------------------------- Ansichten

#[derive(Clone, Debug, Serialize)]
pub struct FullTextRow {
    pub record_id: Uuid,
    pub title: String,
    pub authors: String,
    pub year: Option<i64>,
    pub doi: String,
    /// `false` bei Einträgen mit Status, die (noch) nicht zu den Eingeschlossenen gehören.
    pub needed: bool,
    pub fulltext: Option<FullText>,
    /// Bei Dateien aus dem Ordner: `ok` oder `missing`; sonst leer. Die Prüfsumme prüft erst das Öffnen.
    pub file: &'static str,
    pub zotero_item: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FullTextOverview {
    /// Ordner auf diesem Rechner; steht nicht im Log.
    pub folder: Option<String>,
    pub folder_found: bool,
    pub zotero: Option<ZoteroLink>,
    pub needed: i64,
    pub retrieved: i64,
    pub unavailable: i64,
    pub rows: Vec<FullTextRow>,
}

/// Zuordnung einer Datei. Der Pfad ist relativ zum Ordner oder absolut und liegt dann im Ordner.
#[derive(Clone, Debug, Deserialize)]
pub struct Assign {
    pub record_id: Uuid,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FileProblem {
    pub record_id: Uuid,
    pub title: String,
    pub path: String,
    /// `missing` oder `changed`.
    pub problem: &'static str,
}

/// Wo der Volltext eines Eintrags liegt.
#[derive(Clone, Debug, PartialEq)]
pub enum Location {
    Folder(PathBuf),
    /// Schlüssel des Anhangs; den Pfad der Datei kennt nur Zotero.
    Zotero(String),
}

fn hash_file(path: &Path) -> Result<(String, i64)> {
    let data = std::fs::read(path).map_err(io)?;
    Ok((sha256_hex(&data), data.len() as i64))
}

impl Review {
    pub(crate) fn fulltexts(&self) -> Result<FullTexts> {
        Ok(FullTexts::build(&self.store.events_of_types(&FULLTEXT_TYPES)?))
    }

    /// Einträge, die einen Volltext brauchen: die Eingeschlossenen der Station, aus der die
    /// Volltext-Station der Vorlage schöpft (letzte Runde). Ohne solche Station die Eingeschlossenen
    /// aller Stufen. Verblindete Stufen zählen nicht, weil ihr Ergebnis Stimmen anderer verriete.
    pub(crate) fn fulltext_scope(&self, workflow: &Workflow, stages: &[StageState]) -> Result<BTreeSet<Uuid>> {
        let template = &workflow.template;
        let source = template.stations.iter().find(|s| s.requires_fulltext).and_then(|s| s.input.as_ref());
        let sources: Vec<&StageState> = match source.and_then(|i| i.station.as_deref()) {
            // Alle Runden der Station; für einen Eintrag gilt die letzte, in der er entschieden ist.
            Some(key) => {
                let mut rounds: Vec<&StageState> =
                    stages.iter().filter(|s| workflow.station_of(s).is_some_and(|x| x.key == key)).collect();
                rounds.sort_by_key(|s| s.config.round);
                rounds
            }
            None => stages.iter().collect(),
        };
        let resolutions = self.resolutions()?;
        let mut decided: BTreeMap<Uuid, bool> = BTreeMap::new();
        for stage in sources.into_iter().filter(|s| !s.blind_active()) {
            for (id, decision) in stage.final_decisions() {
                let Ok(id) = Uuid::parse_str(&id) else { continue };
                let included = decision.decision.as_deref() == Some("include") && !resolutions.is_removed(&id);
                // Ohne Vorlage zählt ein Einschluss in irgendeiner Stufe, wie bisher.
                let entry = decided.entry(id).or_insert(false);
                *entry = if source.is_some_and(|i| i.station.is_some()) { included } else { *entry || included };
            }
        }
        Ok(decided.into_iter().filter(|(_, included)| *included).map(|(id, _)| id).collect())
    }

    fn scope_now(&self) -> Result<BTreeSet<Uuid>> {
        let state = self.state()?;
        let stages: Vec<StageState> =
            state.stage_ids.iter().filter_map(|id| StageState::build(id, &self.store.stage_events(id).ok()?)).collect();
        self.fulltext_scope(&self.workflow()?, &stages)
    }

    /// Ordner mit den Volltexten auf diesem Rechner.
    pub fn fulltext_folder(&self) -> Result<Option<PathBuf>> {
        Ok(self.store.meta(FOLDER_KEY)?.filter(|p| !p.is_empty()).map(PathBuf::from))
    }

    /// Verknüpft den Ordner auf diesem Rechner oder löst die Verknüpfung. Das ist eine lokale
    /// Einstellung: Jede Person legt selbst fest, wo der gemeinsame Ordner bei ihr liegt.
    pub fn set_fulltext_folder(&self, folder: Option<&Path>) -> Result<()> {
        require(&self.state()?, &self.me.actor_id, &ROLES)?;
        let value = match folder {
            None => String::new(),
            Some(path) => {
                let path = path.canonicalize().map_err(|_| invalid("Diesen Ordner gibt es nicht."))?;
                if !path.is_dir() {
                    return Err(invalid("Das ist kein Ordner."));
                }
                path.to_string_lossy().to_string()
            }
        };
        self.store.set_meta(FOLDER_KEY, &value)
    }

    fn linked_folder(&self) -> Result<PathBuf> {
        let folder = self.fulltext_folder()?.ok_or_else(|| invalid("Auf diesem Rechner ist kein Ordner verknüpft."))?;
        folder
            .canonicalize()
            .map_err(|_| not_found(format!("Der verknüpfte Ordner „{}“ ist nicht erreichbar.", folder.display())))
    }

    /// Pfad innerhalb des Ordners als (relativ, absolut). Weist alles ab, was außerhalb liegt.
    fn inside_folder(&self, folder: &Path, path: &str) -> Result<(String, PathBuf)> {
        let given = Path::new(path);
        let joined = if given.is_absolute() { given.to_path_buf() } else { folder.join(given) };
        let full = joined.canonicalize().map_err(|_| not_found(format!("Die Datei „{path}“ gibt es nicht.")))?;
        let relative = full.strip_prefix(folder).map_err(|_| {
            invalid("Die Datei liegt nicht im verknüpften Ordner. Lege sie dort ab; die App kopiert nichts.")
        })?;
        let relative = relative.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        if !full.is_file() || !catalog::is_relative_path(&relative) {
            return Err(invalid("Das ist keine Datei im verknüpften Ordner."));
        }
        Ok((relative, full))
    }

    /// Alle Einträge, die einen Volltext brauchen oder einen Status haben, mit Zahlen.
    pub fn fulltext_overview(&self) -> Result<FullTextOverview> {
        require(&self.state()?, &self.me.actor_id, &ROLES)?;
        let fulltexts = self.fulltexts()?;
        let needed = self.scope_now()?;
        let folder = self.fulltext_folder()?;
        let root = folder.as_ref().and_then(|f| f.canonicalize().ok());
        let ids: BTreeSet<Uuid> = needed.iter().chain(fulltexts.status.keys()).copied().collect();
        let mut rows = Vec::new();
        for id in ids {
            let Some(record) = self.store.record(&id)? else { continue };
            let fulltext = fulltexts.status.get(&id).cloned();
            let file = match (&fulltext, &root) {
                (Some(f), Some(root)) if f.origin == "folder" => match root.join(&f.path).is_file() {
                    true => "ok",
                    false => "missing",
                },
                _ => "",
            };
            rows.push(FullTextRow {
                record_id: id,
                title: record.title,
                authors: record.authors,
                year: record.year,
                doi: record.doi,
                needed: needed.contains(&id),
                fulltext,
                file,
                zotero_item: fulltexts.items.get(&id).cloned(),
            });
        }
        rows.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()).then(a.record_id.cmp(&b.record_id)));
        let (retrieved, unavailable) = fulltexts.counts(&needed);
        Ok(FullTextOverview {
            folder: folder.map(|f| f.to_string_lossy().to_string()),
            folder_found: root.is_some(),
            zotero: fulltexts.zotero,
            needed: needed.len() as i64,
            retrieved,
            unavailable,
            rows,
        })
    }

    /// Einträge, die einen Volltext brauchen und noch keinen Status haben.
    fn open_records(&self, fulltexts: &FullTexts) -> Result<Vec<RecordRow>> {
        let mut out = Vec::new();
        for id in self.scope_now()?.iter().filter(|id| !fulltexts.status.contains_key(id)) {
            out.extend(self.store.record(id)?);
        }
        Ok(out)
    }

    /// Vorschläge aus dem verknüpften Ordner für alle offenen Einträge. Nichts davon gilt, bevor es
    /// bestätigt ist.
    pub fn fulltext_suggestions(&self) -> Result<Vec<Suggestion>> {
        require(&self.state()?, &self.me.actor_id, &WRITE_ROLES)?;
        let folder = self.linked_folder()?;
        let fulltexts = self.fulltexts()?;
        let taken: BTreeSet<&str> =
            fulltexts.status.values().filter(|f| f.origin == "folder").map(|f| f.path.as_str()).collect();
        let records = self.open_records(&fulltexts)?;
        let mut files: Vec<FolderFile> =
            scan_folder(&folder)?.into_iter().filter(|f| !taken.contains(f.path.as_str())).collect();
        if records.iter().any(|r| !r.doi.is_empty()) {
            for file in &mut files {
                file.doi = doi_in_pdf(&folder.join(&file.path));
            }
        }
        Ok(suggest(&records, &files))
    }

    fn require_record(&self, id: &Uuid) -> Result<RecordRow> {
        self.store.record(id)?.ok_or_else(|| not_found("Eintrag nicht gefunden."))
    }

    /// Ordnet Dateien aus dem Ordner zu: ein `FullTextStatusSet` je Eintrag mit Pfad und Prüfsumme.
    pub fn assign_fulltexts(&self, list: &[Assign]) -> Result<usize> {
        require(&self.state()?, &self.me.actor_id, &WRITE_ROLES)?;
        if list.is_empty() {
            return Err(invalid("Keine Zuordnung gewählt."));
        }
        let folder = self.linked_folder()?;
        let mut payloads = Vec::new();
        for item in list {
            self.require_record(&item.record_id)?;
            let (relative, full) = self.inside_folder(&folder, &item.path)?;
            let (sha256, size) = hash_file(&full)?;
            payloads.push(json!({
                "record_id": item.record_id.to_string(), "status": "retrieved", "origin": "folder",
                "path": relative, "sha256": sha256, "size": size,
            }));
        }
        let tx = self.store.conn().unchecked_transaction()?;
        for payload in payloads {
            self.append("FullTextStatusSet", None, payload)?;
        }
        tx.commit()?;
        Ok(list.len())
    }

    /// Vermerkt, dass ein Volltext nicht zu beschaffen ist. Der Grund steht später im Bericht.
    pub fn set_fulltext_unavailable(&self, record_id: &Uuid, reason: &str) -> Result<()> {
        require(&self.state()?, &self.me.actor_id, &WRITE_ROLES)?;
        self.require_record(record_id)?;
        if reason.trim().is_empty() {
            return Err(invalid("Warum ist der Volltext nicht beschaffbar? Der Grund fehlt."));
        }
        let payload = json!({"record_id": record_id.to_string(), "status": "unavailable", "reason": reason.trim()});
        self.append("FullTextStatusSet", None, payload)?;
        Ok(())
    }

    /// Nimmt Zuordnung oder Vermerk zurück; der Eintrag ist wieder offen.
    pub fn reset_fulltext(&self, record_id: &Uuid) -> Result<()> {
        require(&self.state()?, &self.me.actor_id, &WRITE_ROLES)?;
        if !self.fulltexts()?.status.contains_key(record_id) {
            return Err(invalid("Zu diesem Eintrag ist nichts vermerkt."));
        }
        self.append("FullTextStatusSet", None, json!({"record_id": record_id.to_string(), "status": "open"}))?;
        Ok(())
    }

    /// Verknüpft eine Zotero-Bibliothek oder Collection mit dem Review.
    pub fn link_zotero(&self, link: &ZoteroLink) -> Result<()> {
        require(&self.state()?, &self.me.actor_id, &MANAGE_ROLES)?;
        let payload = serde_json::to_value(link).map_err(|e| invalid(e.to_string()))?;
        self.append("ZoteroLibraryLinked", None, payload)?;
        Ok(())
    }

    /// Vorschläge aus den Einträgen der verknüpften Zotero-Bibliothek für alle offenen Einträge.
    pub fn zotero_suggestions(&self, items: &[ZoteroItem]) -> Result<Vec<ZoteroSuggestion>> {
        require(&self.state()?, &self.me.actor_id, &MANAGE_ROLES)?;
        let fulltexts = self.fulltexts()?;
        Ok(match_zotero(&self.open_records(&fulltexts)?, items))
    }

    /// Hält bestätigte Zuordnungen zu Zotero fest. Einträge mit PDF gelten damit als beschafft; die
    /// übrigen bleiben offen, sind aber dem Zotero-Eintrag zugeordnet. Gibt die Zahl der Volltexte zurück.
    pub fn confirm_zotero(&self, list: &[ZoteroConfirm]) -> Result<usize> {
        require(&self.state()?, &self.me.actor_id, &MANAGE_ROLES)?;
        if list.is_empty() {
            return Err(invalid("Keine Zuordnung gewählt."));
        }
        if self.fulltexts()?.zotero.is_none() {
            return Err(invalid("Es ist keine Zotero-Bibliothek verknüpft."));
        }
        let mut payloads = Vec::new();
        for item in list {
            self.require_record(&item.record_id)?;
            if let (Some(key), Some(file)) = (&item.attachment_key, &item.file) {
                let (sha256, size) = hash_file(file)
                    .map_err(|_| not_found(format!("Die Datei aus Zotero ist nicht lesbar: {}", file.display())))?;
                payloads.push(json!({
                    "record_id": item.record_id.to_string(), "status": "retrieved", "origin": "zotero",
                    "zotero_key": key, "sha256": sha256, "size": size,
                }));
            }
        }
        let tx = self.store.conn().unchecked_transaction()?;
        for chunk in list.chunks(catalog::MAX_MAPPINGS) {
            let items: Vec<Value> =
                chunk.iter().map(|i| json!({"record_id": i.record_id.to_string(), "item_key": i.item_key})).collect();
            self.append("ZoteroItemMapped", None, json!({ "items": items }))?;
        }
        let n = payloads.len();
        for payload in payloads {
            self.append("FullTextStatusSet", None, payload)?;
        }
        tx.commit()?;
        Ok(n)
    }

    /// Stand des Volltexts zu einem Eintrag.
    pub fn fulltext_of(&self, record_id: &Uuid) -> Result<Option<FullText>> {
        Ok(self.fulltexts()?.status.get(record_id).cloned())
    }

    /// Wo der beschaffte Volltext eines Eintrags auf diesem Rechner zu finden ist.
    pub fn fulltext_location(&self, record_id: &Uuid) -> Result<Location> {
        require(&self.state()?, &self.me.actor_id, &ROLES)?;
        let fulltext = self
            .fulltext_of(record_id)?
            .filter(|f| f.status == "retrieved")
            .ok_or_else(|| not_found("Zu diesem Eintrag ist kein Volltext beschafft."))?;
        match fulltext.origin.as_str() {
            "zotero" => Ok(Location::Zotero(fulltext.zotero_key)),
            _ => Ok(Location::Folder(self.linked_folder()?.join(&fulltext.path))),
        }
    }

    /// Liest den Volltext und prüft ihn gegen die Prüfsumme aus dem Log. Bei Volltexten aus Zotero
    /// nennt die App den Pfad, den Zotero gemeldet hat; was gelesen wird, entscheidet die Prüfsumme.
    pub fn read_fulltext(&self, record_id: &Uuid, zotero_file: Option<&Path>) -> Result<Vec<u8>> {
        let path = match (self.fulltext_location(record_id)?, zotero_file) {
            (Location::Folder(path), _) => path,
            (Location::Zotero(_), Some(path)) => path.to_path_buf(),
            (Location::Zotero(_), None) => return Err(invalid("Dieser Volltext liegt in Zotero.")),
        };
        let expected = self.fulltext_of(record_id)?.map(|f| f.sha256).unwrap_or_default();
        let data = std::fs::read(&path).map_err(|_| {
            not_found(format!("Die Datei „{}“ ist auf diesem Rechner nicht zu finden.", path.display()))
        })?;
        if sha256_hex(&data) != expected {
            return Err(integrity(format!(
                "Die Datei „{}“ ist nicht mehr die zugeordnete: Ihre Prüfsumme weicht ab. Ordne den Volltext neu zu, wenn die neue Datei gelten soll.",
                path.display()
            )));
        }
        Ok(data)
    }

    /// Prüft alle Volltexte aus dem Ordner gegen ihre Prüfsummen.
    pub fn verify_fulltexts(&self) -> Result<Vec<FileProblem>> {
        require(&self.state()?, &self.me.actor_id, &ROLES)?;
        let folder = self.linked_folder()?;
        let mut out = Vec::new();
        for (id, fulltext) in self.fulltexts()?.status.iter().filter(|(_, f)| f.origin == "folder") {
            let problem = match hash_file(&folder.join(&fulltext.path)) {
                Err(_) => "missing",
                Ok((sum, _)) if sum != fulltext.sha256 => "changed",
                Ok(_) => continue,
            };
            let title = self.store.record(id)?.map(|r| r.title).unwrap_or_default();
            out.push(FileProblem { record_id: *id, title, path: fulltext.path.clone(), problem });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(title: &str, authors: &str, year: Option<i64>, doi: &str) -> RecordRow {
        RecordRow {
            id: Uuid::new_v4(),
            batch_id: Uuid::nil(),
            title: title.into(),
            abstract_text: String::new(),
            authors: authors.into(),
            year,
            journal: String::new(),
            doi: doi.into(),
            pmid: String::new(),
            url: String::new(),
            keywords: String::new(),
            raw: "{}".into(),
            dedup_key: String::new(),
            duplicate_of: None,
        }
    }

    fn file(path: &str, doi: &str) -> FolderFile {
        FolderFile { path: path.into(), size: 1, doi: doi.into() }
    }

    #[test]
    fn doi_is_found_in_metadata_bytes() {
        let xmp = b"<prism:doi>10.1000/J.Test.2020.01</prism:doi>";
        assert_eq!(doi_in_bytes(xmp).as_deref(), Some("10.1000/j.test.2020.01"));
        let info = b"/Subject (Journal 12 (2020). doi:10.1186/s12913-020-1.) /Title";
        assert_eq!(doi_in_bytes(info).as_deref(), Some("10.1186/s12913-020-1"));
        let url = b"<dc:identifier>doi: https://doi.org/10.5555/abc.def</dc:identifier>";
        assert_eq!(doi_in_bytes(url).as_deref(), Some("10.5555/abc.def"));
        assert_eq!(doi_in_bytes(b"Version 10.1234 ohne Kennung"), None);
    }

    #[test]
    fn files_are_matched_by_doi_then_title_then_author_and_year() {
        let records = vec![
            row(
                "Telemedicine in rural primary care: a scoping review",
                "Meyer, Anna; Koch, B",
                Some(2020),
                "10.1000/tele.1",
            ),
            row("Community health workers and vaccination uptake", "Okafor, C", Some(2019), "10.1000/chw.7"),
            row("Barriers to digital health adoption among older adults", "Suzuki, K", Some(2021), ""),
            row("Short title", "Lindqvist, P", Some(2018), ""),
            row("A study nobody downloaded", "Nobody, N", Some(2000), ""),
        ];
        let files = vec![
            file("a/unbenannt.pdf", "10.1000/tele.1"),
            file("10.1000_chw.7.pdf", ""),
            file("Suzuki - 2021 - Barriers to digital health adoption among older.pdf", ""),
            file("lindqvist2018.pdf", ""),
            file("Urlaub.pdf", ""),
        ];
        let found = suggest(&records, &files);
        let basis = |path: &str| found.iter().find(|s| s.path == path).map(|s| (s.record_id, s.basis));
        assert_eq!(basis("a/unbenannt.pdf"), Some((records[0].id, "doi_metadata")));
        assert_eq!(basis("10.1000_chw.7.pdf"), Some((records[1].id, "doi_filename")));
        assert_eq!(basis(files[2].path.as_str()), Some((records[2].id, "title")));
        assert_eq!(basis("lindqvist2018.pdf"), Some((records[3].id, "author_year")));
        assert_eq!(found.len(), 4);
    }

    #[test]
    fn a_different_doi_or_an_ambiguous_author_year_is_no_match() {
        let records = vec![
            row("Telemedicine in rural primary care: a scoping review", "Meyer, A", Some(2020), "10.1000/tele.1"),
            row("Another paper by the same person", "Meyer, A", Some(2020), ""),
        ];
        // Gleicher Titel im Namen, aber die Datei trägt eine andere DOI
        let other = file("Telemedicine in rural primary care a scoping review.pdf", "10.1000/other");
        assert!(suggest(&records[..1], &[other]).is_empty());
        assert!(suggest(&records, &[file("meyer2020.pdf", "")]).is_empty());
    }

    #[test]
    fn zotero_items_are_matched_by_doi_or_title_and_used_once() {
        let records = vec![
            row("Telemedicine in rural primary care", "Meyer, A", Some(2020), "https://doi.org/10.1000/TELE.1"),
            row("Community health workers and vaccination uptake", "Okafor, C", Some(2019), ""),
            row("Community health workers and vaccination uptake.", "Okafor, C", Some(2019), ""),
            row("Community health workers and vaccination uptake", "Okafor, C", Some(2010), ""),
        ];
        let item = |key: &str, title: &str, doi: &str, year: i64, pdf: Option<&str>| ZoteroItem {
            key: key.into(),
            title: title.into(),
            doi: doi.into(),
            year: Some(year),
            first_author: String::new(),
            attachment_key: pdf.map(str::to_string),
        };
        let items = vec![
            item("AAAAAAA1", "Ganz anderer Titel in Zotero", "10.1000/tele.1", 2020, Some("PPPPPPP1")),
            item("AAAAAAA2", "Community Health Workers and Vaccination Uptake", "", 2019, None),
        ];
        let found = match_zotero(&records, &items);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].record_id, found[0].basis), (records[0].id, "doi"));
        assert_eq!(found[0].attachment_key.as_deref(), Some("PPPPPPP1"));
        assert_eq!((found[1].record_id, found[1].basis, &found[1].item_key), (records[1].id, "title", &items[1].key));
    }
}
