//! Unscharfe Dubletten: Kandidaten als Projektion über die Einträge, Entscheidungen je Paar im Log.
//!
//! Der exakte Abgleich beim Import (`import::dedup_key`) bleibt. Er übersieht denselben Artikel, wenn
//! eine Quelle die DOI liefert und die andere nicht, oder wenn Titel leicht abweichen. Solche Paare
//! schlägt [`candidates`] vor; die Leitung entscheidet jedes mit `DuplicatesResolved`. Kandidaten
//! stehen nicht im Log, nur die Entscheidungen.
//!
//! Wer schon vor dem Import in einer anderen Anwendung (etwa Zotero) Dubletten entfernt hat, vermerkt
//! Anwendung und Anzahl mit `ExternalDuplicatesRecorded` ([`externals`]). Diese Einträge hat die App nie
//! gesehen; sie zählen im Bericht zu den identifizierten und zu den entfernten.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::event::Event;
use crate::projection::order_key;
use crate::store::RecordKey;

/// Ab dieser Titelähnlichkeit in Prozent gilt ein Paar mit gleichem Erstautor als Kandidat.
pub const SIMILAR_WITH_AUTHOR: i64 = 85;
/// Ab dieser Titelähnlichkeit gilt ein Paar mit gleichem Jahr auch ohne Autorenvergleich als Kandidat.
pub const SIMILAR_TITLE_ONLY: i64 = 95;
/// Kürzere normalisierte Titel („Editorial“) sagen nichts über Gleichheit.
const MIN_TITLE: usize = 10;
/// So viele Nachbarn in der Sortierung nach Titel werden verglichen.
const WINDOW: usize = 3;
/// Bis zu dieser Zahl von Einträgen mit demselben Erstautor wird jedes Paar verglichen, darüber nur
/// Nachbarn in der Sortierung nach Titel. Sonst wächst der Aufwand bei häufigen Namen quadratisch.
const FULL_GROUP: usize = 300;
const GROUP_WINDOW: usize = 10;

/// Entscheidungen über Paare, aus `DuplicatesResolved` berechnet.
#[derive(Clone, Debug, Default)]
pub struct Resolutions {
    /// Entfernter Eintrag → Eintrag, der an seiner Stelle bleibt.
    pub removed: BTreeMap<Uuid, Uuid>,
    /// Paare (kleinere ID zuerst), die ausdrücklich keine Dubletten sind.
    pub distinct: BTreeSet<(Uuid, Uuid)>,
}

/// Vermerk über Dubletten, die vor dem Import in einer anderen Anwendung entfernt wurden.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct External {
    pub id: Uuid,
    pub tool: String,
    pub removed: i64,
    pub note: String,
    /// Zeitpunkt des letzten Vermerks.
    pub at: String,
}

/// Die geltenden Vermerke in der Reihenfolge, in der sie zuerst angelegt wurden. Je `entry` gilt der
/// letzte Vermerk in der Ordnung über Personen hinweg; die Anzahl 0 nimmt ihn zurück.
pub fn externals(events: &[Event]) -> Vec<External> {
    let mut sorted: Vec<&Event> = events.iter().filter(|e| e.event_type == "ExternalDuplicatesRecorded").collect();
    sorted.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
    let mut out: Vec<External> = Vec::new();
    for e in sorted {
        let text = |key: &str| e.payload.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        let Ok(id) = Uuid::parse_str(&text("entry")) else { continue };
        let entry = External {
            id,
            tool: text("tool"),
            removed: e.payload.get("removed").and_then(Value::as_i64).unwrap_or(0),
            note: text("note"),
            at: e.occurred_at.clone(),
        };
        match out.iter_mut().find(|x| x.id == id) {
            Some(found) => *found = entry,
            None => out.push(entry),
        }
    }
    out.retain(|x| x.removed > 0);
    out
}

fn pair_key(a: Uuid, b: Uuid) -> (Uuid, Uuid) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

impl Resolutions {
    /// Je Paar gilt die letzte Entscheidung in der Ordnung über Personen hinweg.
    pub fn build(events: &[Event]) -> Resolutions {
        let mut sorted: Vec<&Event> = events.iter().filter(|e| e.event_type == "DuplicatesResolved").collect();
        sorted.sort_by(|a, b| order_key(a).cmp(&order_key(b)));
        // Paar → behaltener Eintrag bei Dublette, sonst `None`
        let mut decisions: BTreeMap<(Uuid, Uuid), Option<Uuid>> = BTreeMap::new();
        let id = |pair: &Value, key: &str| pair.get(key).and_then(Value::as_str).and_then(|s| Uuid::parse_str(s).ok());
        for e in sorted {
            for pair in e.payload.get("pairs").and_then(Value::as_array).into_iter().flatten() {
                let (Some(a), Some(b)) = (id(pair, "a"), id(pair, "b")) else { continue };
                let keep = match pair.get("decision").and_then(Value::as_str) {
                    Some("duplicate") => id(pair, "keep").filter(|k| *k == a || *k == b),
                    _ => None,
                };
                decisions.insert(pair_key(a, b), keep);
            }
        }
        let mut out = Resolutions::default();
        for ((a, b), keep) in decisions {
            match keep {
                None => {
                    out.distinct.insert((a, b));
                }
                Some(keep) => {
                    let loser = if keep == a { b } else { a };
                    // Kein Kreis: Der behaltene Eintrag darf nicht selbst auf den entfernten zurückführen.
                    if out.kept(&keep) != loser {
                        out.removed.insert(loser, keep);
                    }
                }
            }
        }
        out
    }

    pub fn is_removed(&self, id: &Uuid) -> bool {
        self.removed.contains_key(id)
    }

    /// Eintrag, der am Ende der Kette bleibt; der Eintrag selbst, wenn er nicht entfernt ist.
    pub fn kept(&self, id: &Uuid) -> Uuid {
        let mut current = *id;
        for _ in 0..=self.removed.len() {
            match self.removed.get(&current) {
                Some(next) => current = *next,
                None => break,
            }
        }
        current
    }
}

/// Mögliches Dublettenpaar; `a` hat die kleinere ID.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Candidate {
    pub a: Uuid,
    pub b: Uuid,
    /// Ähnlichkeit der normalisierten Titel in ganzen Prozent.
    pub similarity: i64,
}

/// Titel in Kleinbuchstaben, nur Buchstaben und Ziffern.
pub fn normalize_title(title: &str) -> Vec<char> {
    title.chars().flat_map(char::to_lowercase).filter(|c| c.is_alphanumeric()).collect()
}

/// Nachname des ersten Autors, normalisiert. Versteht „Nachname, Vorname“ und „Vorname Nachname“.
pub fn first_author(authors: &str) -> String {
    let first = authors.split(';').next().unwrap_or_default().trim();
    let surname = match first.split_once(',') {
        Some((surname, _)) => surname,
        None => first.rsplit(char::is_whitespace).next().unwrap_or_default(),
    };
    surname.chars().flat_map(char::to_lowercase).filter(|c| c.is_alphanumeric()).collect()
}

/// Ähnlichkeit zweier Zeichenfolgen in ganzen Prozent: Anteil der Zeichen, die nach dem
/// Levenshtein-Abstand übereinstimmen. `None`, wenn sie unter `min_percent` liegt; dann bricht die
/// Rechnung früh ab.
pub fn similarity(a: &[char], b: &[char], min_percent: i64) -> Option<i64> {
    let longest = a.len().max(b.len());
    if longest == 0 {
        return None;
    }
    let allowed = longest * (100 - min_percent.clamp(0, 100)) as usize / 100;
    if a.len().abs_diff(b.len()) > allowed {
        return None;
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        let mut row_min = current[0];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current[j + 1] = (previous[j] + cost).min(previous[j + 1] + 1).min(current[j] + 1);
            row_min = row_min.min(current[j + 1]);
        }
        if row_min > allowed {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let distance = previous[b.len()];
    (distance <= allowed).then(|| ((longest - distance) * 100 / longest) as i64)
}

struct Entry<'a> {
    key: &'a RecordKey,
    title: Vec<char>,
    author: String,
    /// Ziffern des Titels. „Teil 1“ und „Teil 2“ sind sich ähnlich, aber nicht dasselbe.
    digits: Vec<char>,
}

impl Entry<'_> {
    /// Ob ein ähnlicher Titel hier überhaupt für eine Dublette sprechen kann.
    fn comparable(&self, other: &Entry) -> bool {
        self.digits == other.digits && !ids_conflict(self.key, other.key)
    }
}

fn years_close(a: Option<i64>, b: Option<i64>, tolerance: i64) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= tolerance,
        _ => true,
    }
}

/// Zwei verschiedene DOIs oder PMIDs heißen: verschiedene Veröffentlichungen.
fn ids_conflict(a: &RecordKey, b: &RecordKey) -> bool {
    let differ = |x: &str, y: &str| !x.is_empty() && !y.is_empty() && x != y;
    differ(&a.doi, &b.doi) || differ(&a.pmid, &b.pmid)
}

/// Kandidaten unter den Einträgen, die weder exakt noch entschieden Dublette sind. Paare, die als
/// „keine Dublette“ entschieden sind, fehlen. Sortiert nach Ähnlichkeit, die sichersten zuerst.
pub fn candidates(keys: &[RecordKey], resolutions: &Resolutions) -> Vec<Candidate> {
    let entries: Vec<Entry> = keys
        .iter()
        .filter(|k| k.duplicate_of.is_none() && !resolutions.is_removed(&k.id))
        .map(|key| {
            let title = normalize_title(&key.title);
            let digits = title.iter().copied().filter(char::is_ascii_digit).collect();
            Entry { key, title, author: first_author(&key.authors), digits }
        })
        .filter(|e| e.title.len() >= MIN_TITLE)
        .collect();
    let mut found: BTreeMap<(Uuid, Uuid), i64> = BTreeMap::new();
    let mut offer = |x: &Entry, y: &Entry, similarity: i64| {
        let pair = pair_key(x.key.id, y.key.id);
        if x.key.id != y.key.id && !ids_conflict(x.key, y.key) && !resolutions.distinct.contains(&pair) {
            let best = found.entry(pair).or_insert(similarity);
            *best = (*best).max(similarity);
        }
    };

    // 1. Gleicher Titel, Jahr höchstens eins auseinander, Erstautor nicht widersprüchlich.
    let mut by_title: BTreeMap<&[char], Vec<&Entry>> = BTreeMap::new();
    for e in &entries {
        by_title.entry(&e.title).or_default().push(e);
    }
    for group in by_title.values() {
        for (i, x) in group.iter().enumerate() {
            for y in &group[i + 1..] {
                let authors_differ = !x.author.is_empty() && !y.author.is_empty() && x.author != y.author;
                if years_close(x.key.year, y.key.year, 1) && !authors_differ {
                    offer(x, y, 100);
                }
            }
        }
    }

    // 2. Gleicher Erstautor, Jahr höchstens eins auseinander, Titel ähnlich.
    let mut by_author: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
    for e in entries.iter().filter(|e| !e.author.is_empty()) {
        by_author.entry(&e.author).or_default().push(e);
    }
    for group in by_author.values_mut() {
        let reach = if group.len() > FULL_GROUP {
            group.sort_by(|x, y| x.title.cmp(&y.title));
            GROUP_WINDOW
        } else {
            group.len()
        };
        for (i, x) in group.iter().enumerate() {
            for y in group.iter().skip(i + 1).take(reach) {
                if years_close(x.key.year, y.key.year, 1) && x.comparable(y) {
                    if let Some(s) = similarity(&x.title, &y.title, SIMILAR_WITH_AUTHOR) {
                        offer(x, y, s);
                    }
                }
            }
        }
    }

    // 3. Nachbarn in der Sortierung nach Titel, vorwärts und rückwärts gelesen, bei gleichem Jahr.
    let mut forward: Vec<&Entry> = entries.iter().collect();
    forward.sort_by(|x, y| x.title.cmp(&y.title));
    let mut backward: Vec<&Entry> = entries.iter().collect();
    backward.sort_by(|x, y| x.title.iter().rev().cmp(y.title.iter().rev()));
    for order in [forward, backward] {
        for (i, x) in order.iter().enumerate() {
            for y in order.iter().skip(i + 1).take(WINDOW) {
                if x.key.year == y.key.year && x.comparable(y) {
                    if let Some(s) = similarity(&x.title, &y.title, SIMILAR_TITLE_ONLY) {
                        offer(x, y, s);
                    }
                }
            }
        }
    }

    let mut out: Vec<Candidate> =
        found.into_iter().map(|((a, b), similarity)| Candidate { a, b, similarity }).collect();
    out.sort_by(|x, y| y.similarity.cmp(&x.similarity).then(x.a.cmp(&y.a)).then(x.b.cmp(&y.b)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(n: u128, title: &str, authors: &str, year: Option<i64>, doi: &str) -> RecordKey {
        RecordKey {
            id: Uuid::from_u128(n),
            title: title.into(),
            authors: authors.into(),
            year,
            doi: doi.into(),
            pmid: String::new(),
            duplicate_of: None,
        }
    }

    fn chars(s: &str) -> Vec<char> {
        normalize_title(s)
    }

    #[test]
    fn similarity_is_the_share_of_matching_characters() {
        assert_eq!(similarity(&chars("abcdefghij"), &chars("abcdefghij"), 80), Some(100));
        assert_eq!(similarity(&chars("abcdefghij"), &chars("abcdefghix"), 80), Some(90));
        assert_eq!(similarity(&chars("abcdefghij"), &chars("abcdefgh"), 80), Some(80));
        assert_eq!(similarity(&chars("abcdefghij"), &chars("zzzzzzzzzz"), 80), None);
        assert_eq!(similarity(&chars("abcdefghij"), &chars("abc"), 80), None);
        assert_eq!(similarity(&[], &[], 80), None);
    }

    #[test]
    fn first_author_reads_both_name_orders() {
        assert_eq!(first_author("Müller, Anna; Schmidt, Ben"), "müller");
        assert_eq!(first_author("Anna Müller; Ben Schmidt"), "müller");
        assert_eq!(first_author("van der Berg, C."), "vanderberg");
        assert_eq!(first_author(""), "");
    }

    #[test]
    fn same_article_with_and_without_doi_is_a_candidate() {
        let keys = [
            key(1, "Migraine treatment in adults: a trial", "Lee, K.", Some(2020), "10.1/x"),
            key(2, "Migraine Treatment in Adults - A Trial.", "Lee, Kim", Some(2021), ""),
            key(3, "Migraine treatment in adults: a trial", "Lee, K.", Some(2020), "10.1/other"),
            key(4, "Something entirely different about sleep", "Lee, K.", Some(2020), ""),
        ];
        let found = candidates(&keys, &Resolutions::default());
        let pairs: Vec<(Uuid, Uuid, i64)> = found.iter().map(|c| (c.a, c.b, c.similarity)).collect();
        // 1 und 3 haben verschiedene DOIs; 2 passt zu beiden.
        assert_eq!(
            pairs,
            [(Uuid::from_u128(1), Uuid::from_u128(2), 100), (Uuid::from_u128(2), Uuid::from_u128(3), 100)]
        );
    }

    #[test]
    fn similar_titles_need_the_same_author_or_a_near_match() {
        let keys = [
            key(1, "Effects of yoga on chronic low back pain in adults", "Chen, L.", Some(2019), ""),
            key(2, "Effect of yoga on chronic low back pain in older adults", "Chen, L.", Some(2019), ""),
            key(3, "Effect of yoga on chronic low back pain in older adults", "Okafor, N.", Some(2015), ""),
            key(4, "Effects of yoga on chronic low back pain in adults.", "", Some(2019), ""),
        ];
        let found = candidates(&keys, &Resolutions::default());
        let pairs: BTreeSet<(u128, u128)> = found.iter().map(|c| (c.a.as_u128(), c.b.as_u128())).collect();
        // 1–2: gleicher Erstautor und ähnlich; 1–4: gleicher Titel; 2–4: unter 95 % ohne Autor; 3: anderes Jahr.
        assert_eq!(pairs, BTreeSet::from([(1, 2), (1, 4)]));
        assert!(found.iter().find(|c| c.b.as_u128() == 2).unwrap().similarity < 100);
    }

    #[test]
    fn titles_that_differ_in_a_number_are_not_candidates() {
        let keys = [
            key(1, "Guideline for migraine prophylaxis: part 1 of the series", "Roy, S.", Some(2020), ""),
            key(2, "Guideline for migraine prophylaxis: part 2 of the series", "Roy, S.", Some(2020), ""),
        ];
        assert!(candidates(&keys, &Resolutions::default()).is_empty());
    }

    #[test]
    fn short_titles_and_resolved_records_are_left_out() {
        let mut exact = key(3, "A long enough title about headache", "A, B", Some(2020), "");
        exact.duplicate_of = Some(Uuid::from_u128(1));
        let keys = [
            key(1, "A long enough title about headache", "A, B", Some(2020), ""),
            key(2, "A long enough title about headache", "A, B", Some(2020), ""),
            exact,
            key(4, "Editorial", "", Some(2020), ""),
            key(5, "Editorial", "", Some(2020), ""),
        ];
        assert_eq!(candidates(&keys, &Resolutions::default()).len(), 1);
        let mut resolutions = Resolutions::default();
        resolutions.distinct.insert((Uuid::from_u128(1), Uuid::from_u128(2)));
        assert!(candidates(&keys, &resolutions).is_empty());
        let mut resolutions = Resolutions::default();
        resolutions.removed.insert(Uuid::from_u128(2), Uuid::from_u128(1));
        assert!(candidates(&keys, &resolutions).is_empty());
    }

    #[test]
    fn chains_end_at_the_kept_record_and_never_loop() {
        let id = Uuid::from_u128;
        let mut resolutions = Resolutions::default();
        resolutions.removed.insert(id(1), id(2));
        resolutions.removed.insert(id(2), id(3));
        assert_eq!(resolutions.kept(&id(1)), id(3));
        assert_eq!(resolutions.kept(&id(3)), id(3));
        resolutions.removed.insert(id(3), id(1));
        // Ein Kreis kann über `build` nicht entstehen; `kept` hält trotzdem an.
        let _ = resolutions.kept(&id(1));
    }
}
