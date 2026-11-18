//! Import von RIS und BibTeX, portiert aus backend/tests/test_importers.py.

use ort_core::import::{dedup_key, detect_format, parse, Item};

const RIS: &str = "TY  - JOUR
AU  - Miller, Anna
AU  - Chen, Li
TI  - Deep learning for headache classification
AB  - We evaluate neural networks.
PY  - 2021/03/01
JO  - Journal of Tests
DO  - https://doi.org/10.1000/ABC123
KW  - headache
KW  - deep learning
UR  - https://example.org/a
ER  -
";

const BIBTEX: &str = "@article{smith2020,
  title = {A {Study} of things},
  author = {Smith, John and Doe, Jane},
  year = {2020},
  journal = {Journal of Things},
  doi = {10.1000/XYZ}
}
";

#[test]
fn ris() {
    let items = parse("ris", RIS).unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.title, "Deep learning for headache classification");
    assert_eq!(item.abstract_text, "We evaluate neural networks.");
    assert_eq!(item.authors, "Miller, Anna; Chen, Li");
    assert_eq!(item.year, Some(2021));
    assert_eq!(item.journal, "Journal of Tests");
    assert_eq!(item.doi, "10.1000/abc123");
    assert_eq!(item.keywords, "headache; deep learning");
    assert_eq!(item.url, "https://example.org/a");
    assert_eq!(dedup_key(item), "doi:10.1000/abc123");
}

#[test]
fn ris_continuation_lines_and_missing_er() {
    let text = "TY  - JOUR\nTI  - A long title\n  that continues here\nAB  - x\n\nTY  - JOUR\nTI  - Second\n";
    let items = parse("ris", text).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].title, "A long title that continues here");
    assert_eq!(items[1].title, "Second");
}

#[test]
fn bibtex() {
    let items = parse("bibtex", BIBTEX).unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.title, "A Study of things");
    assert_eq!(item.authors, "Smith, John; Doe, Jane");
    assert_eq!(item.year, Some(2020));
    assert_eq!(item.journal, "Journal of Things");
    assert_eq!(item.doi, "10.1000/xyz");
}

#[test]
fn bibtex_quotes_numbers_comments_and_concatenation() {
    let text = r#"@comment{ignore {me}}
@Book{k1, title = "The {RNA} world", year = 1999, author = "A and B", note = "x" # " y",}
@inproceedings(k2, title = {Proc}, booktitle = {Conf}, date = {2021-05-01})
"#;
    let items = parse("bibtex", text).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].title, "The RNA world");
    assert_eq!(items[0].year, Some(1999));
    assert_eq!(items[0].authors, "A; B");
    assert_eq!(items[0].raw["note"], "x y");
    assert_eq!(items[1].journal, "Conf");
    assert_eq!(items[1].year, Some(2021));
}

#[test]
fn bibtex_unclosed_entry_is_an_error() {
    assert!(parse("bibtex", "@article{x, title = {open").is_err());
}

#[test]
fn dedup_key_without_doi() {
    let item = |title: &str, year| Item { title: title.into(), year, ..Item::default() };
    let a = item("Rule-based triage of Migraine patients", Some(2019));
    let b = item("Rule-Based Triage of migraine patients.", Some(2019));
    assert_eq!(dedup_key(&a), dedup_key(&b));
    assert_ne!(dedup_key(&a), "");
    assert_eq!(dedup_key(&item("Kurz", Some(2019))), "");
}

#[test]
fn format_detection() {
    assert_eq!(detect_format("a.bib", "").unwrap(), "bibtex");
    assert_eq!(detect_format("a.ris", "").unwrap(), "ris");
    assert_eq!(detect_format("export", RIS).unwrap(), "ris");
    assert_eq!(detect_format("export", BIBTEX).unwrap(), "bibtex");
    assert!(detect_format("bild.png", "kein Literaturformat").is_err());
}
