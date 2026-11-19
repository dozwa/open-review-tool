//! Lesender Zugriff auf Zotero über dessen lokale Schnittstelle (`http://127.0.0.1:23119/api`).
//!
//! Zotero bietet sie ab Version 7; sie muss in den Einstellungen eingeschaltet sein. Gelesen werden
//! Bibliotheken, Collections, Einträge und der Pfad eines PDF-Anhangs. Geschrieben wird nichts. Die
//! Zuordnung zu den Einträgen des Reviews rechnet der Kern (`ort_core::fulltext::match_zotero`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use ort_core::fulltext::{ZoteroItem, ZoteroLink};
use ort_core::import::{normalize_doi, year_of};
use serde::Serialize;
use serde_json::Value;

const BASE: &str = "http://127.0.0.1:23119/api";

#[derive(Clone, Debug, Serialize)]
pub struct Collection {
    pub key: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Library {
    pub library_type: String,
    pub library_id: i64,
    pub name: String,
    pub collections: Vec<Collection>,
}

fn get(path: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).redirects(0).build();
    match agent.get(&format!("{BASE}{path}")).set("Zotero-API-Version", "3").call() {
        Ok(response) => response.into_string().map_err(|e| format!("Antwort von Zotero nicht lesbar: {e}")),
        Err(ureq::Error::Status(403, _)) => Err("In Zotero ist die lokale Schnittstelle ausgeschaltet. Schalte sie in den Zotero-Einstellungen unter „Erweitert“ ein: „Anderen Anwendungen auf diesem Computer erlauben, mit Zotero zu kommunizieren“.".into()),
        Err(ureq::Error::Status(404, _)) => Err("Zotero kennt diese Bibliothek, Collection oder Datei nicht (mehr).".into()),
        Err(ureq::Error::Status(code, _)) => Err(format!("Zotero meldet einen Fehler ({code}).")),
        Err(ureq::Error::Transport(_)) => Err("Zotero antwortet nicht. Starte Zotero (ab Version 7) auf diesem Rechner und versuche es erneut.".into()),
    }
}

fn get_json(path: &str) -> Result<Value, String> {
    serde_json::from_str(&get(path)?).map_err(|e| format!("Antwort von Zotero nicht lesbar: {e}"))
}

fn prefix(library_type: &str, library_id: i64) -> String {
    match library_type {
        "group" => format!("/groups/{library_id}"),
        _ => "/users/0".to_string(),
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn collections(prefix: &str) -> Result<Vec<Collection>, String> {
    let list = get_json(&format!("{prefix}/collections"))?;
    let mut out: Vec<Collection> = list
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| Collection { key: text(c, "key").to_string(), name: text(&c["data"], "name").to_string() })
        .filter(|c| !c.key.is_empty())
        .collect();
    out.sort_by_key(|c| c.name.to_lowercase());
    Ok(out)
}

/// Eigene Bibliothek und Gruppenbibliotheken mit ihren Collections.
pub fn libraries() -> Result<Vec<Library>, String> {
    let mut out = vec![Library {
        library_type: "user".into(),
        library_id: 0,
        name: "Meine Bibliothek".into(),
        collections: collections("/users/0")?,
    }];
    for group in get_json("/users/0/groups")?.as_array().into_iter().flatten() {
        let Some(id) = group.get("id").and_then(Value::as_i64) else { continue };
        out.push(Library {
            library_type: "group".into(),
            library_id: id,
            name: text(&group["data"], "name").to_string(),
            collections: collections(&prefix("group", id))?,
        });
    }
    Ok(out)
}

/// Einträge aus zwei Antworten von Zotero: den Einträgen der Bibliothek oder Collection und allen
/// Anhängen der Bibliothek. Ein PDF-Anhang ohne übergeordneten Eintrag zählt selbst als Eintrag.
pub fn parse_items(items: &Value, attachments: &Value) -> Vec<ZoteroItem> {
    let is_pdf =
        |data: &Value| text(data, "itemType") == "attachment" && text(data, "contentType") == "application/pdf";
    let mut pdf_of: BTreeMap<&str, &str> = BTreeMap::new();
    for a in attachments.as_array().into_iter().flatten().filter(|a| is_pdf(&a["data"])) {
        let parent = text(&a["data"], "parentItem");
        if !parent.is_empty() {
            pdf_of.entry(parent).or_insert(text(a, "key"));
        }
    }
    let mut out = Vec::new();
    for item in items.as_array().into_iter().flatten() {
        let (key, data) = (text(item, "key"), &item["data"]);
        let kind = text(data, "itemType");
        let attachment_key = match kind {
            "note" | "annotation" => continue,
            "attachment" if !is_pdf(data) || !text(data, "parentItem").is_empty() => continue,
            "attachment" => Some(key.to_string()),
            _ => pdf_of.get(key).map(|k| k.to_string()),
        };
        let mut doi = text(data, "DOI").to_string();
        if doi.is_empty() {
            // Eintragsarten ohne DOI-Feld tragen sie oft als Zeile „DOI: …“ im Feld Extra.
            let extra = text(data, "extra").lines().find_map(|l| l.trim().strip_prefix("DOI:").map(str::trim));
            doi = extra.unwrap_or_default().to_string();
        }
        let author = data.get("creators").and_then(Value::as_array).and_then(|list| list.first());
        let first_author = author.map(|c| match text(c, "lastName") {
            "" => text(c, "name"),
            last => last,
        });
        out.push(ZoteroItem {
            key: key.to_string(),
            title: text(data, "title").to_string(),
            doi: if doi.is_empty() { doi } else { normalize_doi(&doi) },
            year: year_of(text(data, "date")),
            first_author: first_author.unwrap_or_default().to_string(),
            attachment_key,
        });
    }
    out
}

/// Alle Einträge der verknüpften Bibliothek oder Collection, mit dem Schlüssel ihres PDF-Anhangs.
pub fn items(link: &ZoteroLink) -> Result<Vec<ZoteroItem>, String> {
    let prefix = prefix(&link.library_type, link.library_id);
    let path = match &link.collection_key {
        Some(key) => format!("{prefix}/collections/{key}/items"),
        None => format!("{prefix}/items"),
    };
    let attachments = get_json(&format!("{prefix}/items?itemType=attachment"))?;
    Ok(parse_items(&get_json(&path)?, &attachments))
}

/// Macht aus `file:///Users/a/Mein%20Text.pdf` den Pfad `/Users/a/Mein Text.pdf`.
pub fn path_from_file_url(url: &str) -> Option<PathBuf> {
    let rest = url.trim().strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%').then(|| rest.get(i + 1..i + 3)).flatten();
        match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok().map(PathBuf::from)
}

/// Pfad eines Anhangs auf diesem Rechner, wie Zotero ihn meldet.
pub fn file_path(link: &ZoteroLink, attachment_key: &str) -> Result<PathBuf, String> {
    if !ort_core::catalog::is_zotero_key(attachment_key) {
        return Err("Ungültiger Zotero-Schlüssel.".into());
    }
    let prefix = prefix(&link.library_type, link.library_id);
    let url = get(&format!("{prefix}/items/{attachment_key}/file/view/url"))?;
    path_from_file_url(&url).ok_or_else(|| "Zotero hat für diesen Anhang keine lokale Datei.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn items_get_their_pdf_and_notes_are_left_out() {
        let items = json!([
            {"key": "AAAAAAA1", "data": {"itemType": "journalArticle", "title": "Erster Artikel", "DOI": "https://doi.org/10.1000/ABC",
                "date": "2020-03-01", "creators": [{"lastName": "Meyer", "firstName": "A"}]}},
            {"key": "AAAAAAA2", "data": {"itemType": "report", "title": "Bericht", "extra": "Notiz\nDOI: 10.1000/rep", "date": "März 2019",
                "creators": [{"name": "WHO"}]}},
            {"key": "NNNNNNN1", "data": {"itemType": "note", "parentItem": "AAAAAAA1"}},
            {"key": "PPPPPPP1", "data": {"itemType": "attachment", "contentType": "application/pdf", "parentItem": "AAAAAAA1"}},
            {"key": "PPPPPPP3", "data": {"itemType": "attachment", "contentType": "application/pdf", "title": "Lose PDF"}},
        ]);
        let attachments = json!([
            {"key": "PPPPPPP1", "data": {"itemType": "attachment", "contentType": "application/pdf", "parentItem": "AAAAAAA1"}},
            {"key": "HHHHHHH1", "data": {"itemType": "attachment", "contentType": "text/html", "parentItem": "AAAAAAA2"}},
            {"key": "PPPPPPP3", "data": {"itemType": "attachment", "contentType": "application/pdf", "title": "Lose PDF"}},
        ]);
        let found = parse_items(&items, &attachments);
        assert_eq!(found.len(), 3);
        assert_eq!(
            (found[0].doi.as_str(), found[0].year, found[0].first_author.as_str()),
            ("10.1000/abc", Some(2020), "Meyer")
        );
        assert_eq!(found[0].attachment_key.as_deref(), Some("PPPPPPP1"));
        assert_eq!(
            (found[1].doi.as_str(), found[1].year, found[1].attachment_key.as_deref()),
            ("10.1000/rep", Some(2019), None)
        );
        assert_eq!((found[2].title.as_str(), found[2].attachment_key.as_deref()), ("Lose PDF", Some("PPPPPPP3")));
    }

    /// Probe gegen ein laufendes Zotero und die Beispieldaten:
    /// `ORT_DATA_DIR=… ORT_KEY_STORE=file cargo test -p open-review-tool live_zotero -- --ignored --nocapture`
    #[test]
    #[ignore = "braucht ein laufendes Zotero mit eingeschalteter lokaler Schnittstelle und ORT_DATA_DIR"]
    fn live_zotero_finds_the_records_of_the_linked_review() {
        let dir = PathBuf::from(std::env::var_os("ORT_DATA_DIR").expect("ORT_DATA_DIR fehlt"));
        let me = crate::identity::IdentityStore::new(&dir).load().unwrap().expect("keine Identität");
        let found = libraries().unwrap();
        println!("{} Bibliotheken, {} Collections in der eigenen", found.len(), found[0].collections.len());
        let mut checked = 0;
        for entry in std::fs::read_dir(dir.join("reviews")).unwrap().flatten() {
            let Ok(review) = ort_core::Review::open(&entry.path(), me.clone()) else { continue };
            let Some(link) = review.fulltext_overview().unwrap().zotero else { continue };
            let all = items(&link).unwrap();
            let with_pdf = all.iter().filter(|i| i.attachment_key.is_some()).count();
            let matched = review.zotero_suggestions(&all).unwrap();
            println!(
                "{} Einträge in Zotero, {with_pdf} mit PDF, {} passen zu offenen Einträgen",
                all.len(),
                matched.len()
            );
            for m in &matched {
                let file = m.attachment_key.as_deref().map(|key| file_path(&link, key).unwrap());
                println!("  {} ({} %): PDF {}", m.basis, m.similarity, file.as_ref().map_or("fehlt", |_| "da"));
                assert!(file.is_none_or(|f| f.is_file()), "Zotero nennt eine Datei, die es nicht gibt");
            }
            checked += matched.len();
        }
        assert!(checked > 0, "kein Eintrag der Beispieldaten in Zotero gefunden");
    }

    #[test]
    fn file_urls_become_paths() {
        let path = path_from_file_url("file:///Users/a/Zotero/storage/ABCD2345/Mein%20Text%20%C3%BC.pdf\n");
        assert_eq!(path, Some(PathBuf::from("/Users/a/Zotero/storage/ABCD2345/Mein Text ü.pdf")));
        assert_eq!(path_from_file_url("https://example.org/x.pdf"), None);
    }
}
