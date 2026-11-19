//! Import von RIS, BibTeX und dem PubMed-Format (nbib) sowie der Schlüssel für exakte Dubletten.
//!
//! Verhalten wie im früheren `importers.py` (rispy, bibtexparser): Listen werden mit „; “ verbunden,
//! Leerraum wird zusammengefasst, DOIs werden kleingeschrieben und ohne `https://doi.org/` gespeichert.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::error::{invalid, Result};
use crate::tr;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Item {
    pub title: String,
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    pub authors: String,
    pub year: Option<i64>,
    pub journal: String,
    pub doi: String,
    pub pmid: String,
    pub url: String,
    pub keywords: String,
    pub raw: Value,
}

pub fn detect_format(filename: &str, text: &str) -> Result<&'static str> {
    let name = filename.to_lowercase();
    if name.ends_with(".bib") || name.ends_with(".bibtex") {
        return Ok("bibtex");
    }
    // PubMed speichert sein Format auch als .txt; der Inhalt entscheidet deshalb vor der Endung.
    let head: String = text.trim_start_matches('\u{feff}').trim_start().chars().take(200).collect();
    if name.ends_with(".nbib") || head.starts_with("PMID- ") {
        return Ok("nbib");
    }
    if name.ends_with(".ris") || name.ends_with(".txt") {
        return Ok("ris");
    }
    if head.starts_with('@') {
        return Ok("bibtex");
    }
    if text.lines().any(|l| l.starts_with("TY  - ")) {
        return Ok("ris");
    }
    Err(invalid(tr!("Format nicht erkannt. Unterstützt werden RIS, BibTeX und das PubMed-Format (nbib).")))
}

pub fn parse(format: &str, text: &str) -> Result<Vec<Item>> {
    match format {
        "ris" => Ok(parse_ris(text)),
        "bibtex" => parse_bibtex(text),
        "nbib" => Ok(parse_nbib(text)),
        other => Err(invalid(tr!("Unbekanntes Format: {}", other))),
    }
}

fn spaces() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").unwrap())
}

fn clean_text(value: &str) -> String {
    spaces().replace_all(value, " ").trim().to_string()
}

pub fn year_of(value: &str) -> Option<i64> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\b(1[5-9]\d\d|20\d\d)\b").unwrap());
    re.captures(value).and_then(|c| c[1].parse().ok())
}

pub fn normalize_doi(value: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^(https?://(dx\.)?doi\.org/|doi:\s*)").unwrap());
    let doi = clean_text(value).to_lowercase();
    re.replace(&doi, "").chars().take(255).collect()
}

/// Schlüssel für exakte Dubletten: DOI, sonst normalisierter Titel plus Jahr. Leer = keine Prüfung.
pub fn dedup_key(item: &Item) -> String {
    if !item.doi.is_empty() {
        return format!("doi:{}", item.doi).chars().take(300).collect();
    }
    let title: String =
        item.title.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect();
    if title.chars().count() < 10 {
        return String::new();
    }
    let year = item.year.map(|y| y.to_string()).unwrap_or_default();
    format!("ty:{title}|{year}").chars().take(300).collect()
}

// ---------------------------------------------------------------- RIS

/// Tags, die mehrfach vorkommen dürfen und als Liste gesammelt werden.
const RIS_LISTS: &[&str] = &["AU", "A1", "A2", "A3", "A4", "KW", "UR", "L1", "L2", "N1"];

pub fn parse_ris(text: &str) -> Vec<Item> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let tag_re = RE.get_or_init(|| Regex::new(r"^([A-Z][A-Z0-9])  -(?: (.*))?$").unwrap());
    let mut out = Vec::new();
    let mut current: Option<BTreeMap<String, Vec<String>>> = None;
    let mut last_tag: Option<String> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim_start_matches('\u{feff}').trim_end();
        if let Some(caps) = tag_re.captures(line) {
            let tag = caps[1].to_string();
            let value = caps.get(2).map_or("", |m| m.as_str()).trim().to_string();
            match tag.as_str() {
                "TY" => {
                    if let Some(entry) = current.take() {
                        out.push(ris_item(entry));
                    }
                    let mut entry = BTreeMap::new();
                    entry.insert("TY".to_string(), vec![value]);
                    current = Some(entry);
                }
                "ER" => {
                    if let Some(entry) = current.take() {
                        out.push(ris_item(entry));
                    }
                }
                _ => {
                    if let Some(entry) = current.as_mut() {
                        entry.entry(tag.clone()).or_default().push(value);
                    }
                }
            }
            last_tag = Some(tag);
        } else if !line.trim().is_empty() {
            // Fortsetzungszeile ohne Tag gehört zum letzten Feld.
            if let (Some(entry), Some(tag)) = (current.as_mut(), last_tag.as_ref()) {
                if let Some(last) = entry.get_mut(tag).and_then(|v| v.last_mut()) {
                    last.push(' ');
                    last.push_str(line.trim());
                }
            }
        }
    }
    if let Some(entry) = current.take() {
        out.push(ris_item(entry));
    }
    out
}

fn ris_item(entry: BTreeMap<String, Vec<String>>) -> Item {
    let first = |tags: &[&str]| -> String {
        for tag in tags {
            if let Some(values) = entry.get(*tag) {
                let joined = values.iter().filter(|v| !v.is_empty()).cloned().collect::<Vec<_>>().join("; ");
                if !joined.is_empty() {
                    return clean_text(&joined);
                }
            }
        }
        String::new()
    };
    let mut raw = Map::new();
    for (tag, values) in &entry {
        let value = if RIS_LISTS.contains(&tag.as_str()) || values.len() > 1 {
            Value::Array(values.iter().cloned().map(Value::String).collect())
        } else {
            Value::String(values.first().cloned().unwrap_or_default())
        };
        raw.insert(tag.clone(), value);
    }
    Item {
        title: first(&["TI", "T1", "TT", "CT", "BT"]),
        abstract_text: first(&["AB", "N2"]),
        authors: first(&["AU", "A1"]),
        year: year_of(&first(&["PY", "Y1", "DA"])),
        journal: first(&["JO", "JF", "JA", "J2", "T2"]),
        doi: normalize_doi(&first(&["DO"])),
        pmid: String::new(),
        url: first(&["UR"]),
        keywords: first(&["KW"]),
        raw: Value::Object(raw),
    }
}

// ---------------------------------------------------------------- PubMed (nbib)

/// PubMed-Format: Tag mit bis zu vier Zeichen, aufgefüllt auf vier, dann „- “. Fortsetzungszeilen
/// beginnen mit sechs Leerzeichen. Ein Eintrag beginnt mit `PMID`.
pub fn parse_nbib(text: &str) -> Vec<Item> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let tag_re = RE.get_or_init(|| Regex::new(r"^([A-Z]{2,4}) {0,2}- ?(.*)$").unwrap());
    let mut out = Vec::new();
    let mut current: Option<BTreeMap<String, Vec<String>>> = None;
    let mut last_tag: Option<String> = None;
    for raw_line in text.lines() {
        let line = raw_line.trim_start_matches('\u{feff}').trim_end();
        let tagged = if line.starts_with(' ') { None } else { tag_re.captures(line) };
        if let Some(caps) = tagged {
            let tag = caps[1].to_string();
            let value = caps[2].trim().to_string();
            if tag == "PMID" {
                if let Some(entry) = current.take() {
                    out.push(nbib_item(entry));
                }
                current = Some(BTreeMap::new());
            }
            if let Some(entry) = current.as_mut() {
                entry.entry(tag.clone()).or_default().push(value);
            }
            last_tag = Some(tag);
        } else if !line.trim().is_empty() {
            if let (Some(entry), Some(tag)) = (current.as_mut(), last_tag.as_ref()) {
                if let Some(last) = entry.get_mut(tag).and_then(|v| v.last_mut()) {
                    last.push(' ');
                    last.push_str(line.trim());
                }
            }
        }
    }
    if let Some(entry) = current.take() {
        out.push(nbib_item(entry));
    }
    out
}

fn nbib_item(entry: BTreeMap<String, Vec<String>>) -> Item {
    let all = |tag: &str| -> Vec<String> {
        entry.get(tag).into_iter().flatten().map(|v| clean_text(v)).filter(|v| !v.is_empty()).collect()
    };
    let first = |tags: &[&str]| -> String {
        tags.iter().map(|t| all(t).join("; ")).find(|v| !v.is_empty()).unwrap_or_default()
    };
    // DOI steht in LID oder AID mit dem Zusatz „[doi]“.
    let doi = ["LID", "AID"]
        .iter()
        .flat_map(|t| all(t))
        .find_map(|v| v.strip_suffix("[doi]").map(|d| normalize_doi(d.trim())))
        .unwrap_or_default();
    let mut keywords = all("MH");
    keywords.extend(all("OT"));
    let mut raw = Map::new();
    for (tag, values) in &entry {
        let value = match values.as_slice() {
            [single] => Value::String(single.clone()),
            many => Value::Array(many.iter().cloned().map(Value::String).collect()),
        };
        raw.insert(tag.clone(), value);
    }
    Item {
        title: first(&["TI", "BTI", "TT"]),
        abstract_text: first(&["AB"]),
        authors: first(&["FAU", "AU"]),
        year: year_of(&first(&["DP", "DEP"])),
        journal: first(&["JT", "TA"]),
        doi,
        pmid: first(&["PMID"]).chars().take(32).collect(),
        url: String::new(),
        keywords: keywords.join("; "),
        raw: Value::Object(raw),
    }
}

// ---------------------------------------------------------------- BibTeX

pub fn parse_bibtex(text: &str) -> Result<Vec<Item>> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        i += 1;
        let start = i;
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
        let kind: String = chars[start..i].iter().collect::<String>().to_lowercase();
        skip_ws(&chars, &mut i);
        if i >= chars.len() || (chars[i] != '{' && chars[i] != '(') {
            continue;
        }
        let close = if chars[i] == '{' { '}' } else { ')' };
        i += 1;
        if matches!(kind.as_str(), "comment" | "preamble" | "string") {
            skip_balanced(&chars, &mut i, close);
            continue;
        }
        let key_start = i;
        while i < chars.len() && chars[i] != ',' && chars[i] != close {
            i += 1;
        }
        let key: String = chars[key_start..i].iter().collect::<String>().trim().to_string();
        let mut fields: BTreeMap<String, String> = BTreeMap::new();
        loop {
            skip_ws(&chars, &mut i);
            if i < chars.len() && chars[i] == ',' {
                i += 1;
                skip_ws(&chars, &mut i);
            }
            if i >= chars.len() {
                return Err(invalid(tr!("BibTeX-Eintrag {} ist nicht abgeschlossen.", key)));
            }
            if chars[i] == close {
                i += 1;
                break;
            }
            let name_start = i;
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' && chars[i] != close {
                i += 1;
            }
            let name: String = chars[name_start..i].iter().collect::<String>().to_lowercase();
            skip_ws(&chars, &mut i);
            if i >= chars.len() || chars[i] != '=' {
                return Err(invalid(tr!("BibTeX-Eintrag {}: nach dem Feld „{}“ fehlt „=“.", key, name)));
            }
            i += 1;
            let value = read_value(&chars, &mut i, close)?;
            fields.insert(name, value);
        }
        out.push(bibtex_item(&kind, &key, &fields));
    }
    Ok(out)
}

fn skip_ws(chars: &[char], i: &mut usize) {
    while *i < chars.len() && chars[*i].is_whitespace() {
        *i += 1;
    }
}

fn skip_balanced(chars: &[char], i: &mut usize, close: char) {
    let open = if close == '}' { '{' } else { '(' };
    let mut depth = 1;
    while *i < chars.len() && depth > 0 {
        if chars[*i] == open {
            depth += 1;
        } else if chars[*i] == close {
            depth -= 1;
        }
        *i += 1;
    }
}

/// Wert in Klammern, Anführungszeichen oder als bloßes Wort; Teile mit `#` werden verbunden.
fn read_value(chars: &[char], i: &mut usize, close: char) -> Result<String> {
    let mut value = String::new();
    loop {
        skip_ws(chars, i);
        if *i >= chars.len() {
            return Err(invalid(tr!("BibTeX: Feldwert ist nicht abgeschlossen.")));
        }
        match chars[*i] {
            '{' => {
                *i += 1;
                let mut depth = 1;
                while *i < chars.len() {
                    let c = chars[*i];
                    if c == '{' {
                        depth += 1;
                    } else if c == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    value.push(c);
                    *i += 1;
                }
                if *i >= chars.len() {
                    return Err(invalid(tr!("BibTeX: geschweifte Klammer nicht geschlossen.")));
                }
                *i += 1;
            }
            '"' => {
                *i += 1;
                let mut depth = 0;
                while *i < chars.len() && !(chars[*i] == '"' && depth == 0) {
                    match chars[*i] {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    value.push(chars[*i]);
                    *i += 1;
                }
                if *i >= chars.len() {
                    return Err(invalid(tr!("BibTeX: Anführungszeichen nicht geschlossen.")));
                }
                *i += 1;
            }
            _ => {
                while *i < chars.len()
                    && !matches!(chars[*i], ',' | '#')
                    && chars[*i] != close
                    && !chars[*i].is_whitespace()
                {
                    value.push(chars[*i]);
                    *i += 1;
                }
            }
        }
        skip_ws(chars, i);
        if *i < chars.len() && chars[*i] == '#' {
            *i += 1;
            continue;
        }
        return Ok(value);
    }
}

fn bibtex_item(kind: &str, key: &str, fields: &BTreeMap<String, String>) -> Item {
    let clean = |name: &str| clean_text(&fields.get(name).cloned().unwrap_or_default().replace(['{', '}'], ""));
    let first = |names: &[&str]| names.iter().map(|n| clean(n)).find(|v| !v.is_empty()).unwrap_or_default();
    let mut raw = Map::new();
    raw.insert("ENTRYTYPE".into(), Value::String(kind.into()));
    raw.insert("ID".into(), Value::String(key.into()));
    for (name, value) in fields {
        raw.insert(name.clone(), Value::String(value.clone()));
    }
    let year_text = first(&["year", "date"]);
    Item {
        title: clean("title"),
        abstract_text: clean("abstract"),
        authors: clean("author").replace(" and ", "; "),
        year: year_of(&year_text),
        journal: first(&["journal", "journaltitle", "booktitle"]),
        doi: normalize_doi(&clean("doi")),
        pmid: clean("pmid").chars().take(32).collect(),
        url: clean("url"),
        keywords: clean("keywords"),
        raw: Value::Object(raw),
    }
}
