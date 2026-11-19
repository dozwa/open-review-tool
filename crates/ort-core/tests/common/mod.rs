#![allow(dead_code)]

use ort_core::exchange::{create_from_package, export_package, export_return, merge, Bundle, MergeReport};
use ort_core::service::OpenStage;
use ort_core::{Identity, Review};
use tempfile::TempDir;

pub const RIS: &str = "TY  - JOUR
AU  - Miller, Anna
AU  - Chen, Li
TI  - Deep learning for headache classification
AB  - We evaluate neural networks on clinical vignettes.
PY  - 2021
JO  - Journal of Tests
DO  - 10.1000/abc123
ER  -

TY  - JOUR
AU  - Novak, Petra
TI  - Rule-based triage of migraine patients in primary care
AB  - A prospective cohort study.
PY  - 2019
JO  - Primary Care Research
ER  -

TY  - JOUR
AU  - Okafor, Chidi
TI  - Knowledge graphs in neurology: a scoping review
PY  - 2022
JO  - Neuro Informatics
DO  - 10.1000/kg2022
ER  -

TY  - JOUR
AU  - Miller, A.
TI  - Deep Learning for Headache Classification.
PY  - 2021
DO  - https://doi.org/10.1000/ABC123
ER  -
";

pub const BIBTEX: &str = "@article{novak2019,
  title = {Rule-based triage of {Migraine} patients in primary care},
  author = {Novak, Petra and Svoboda, Jan},
  year = {2019},
  journal = {Primary Care Research}
}
@article{lee2020,
  title = {Large language models for symptom extraction},
  author = {Lee, Min},
  year = {2020},
  doi = {10.1000/llm2020}
}
";

/// Drei Einträge ohne DOI, die der exakte Abgleich nicht erkennt: zwei sind dieselben Artikel wie in
/// [`RIS`] und [`BIBTEX`], der dritte hat nur einen ähnlichen Titel.
pub const SECOND_SOURCE: &str = "TY  - JOUR
AU  - Miller, Anna
TI  - Deep learning for headache classification
PY  - 2021
ER  -

TY  - JOUR
AU  - Okafor, C.
TI  - Knowledge graphs in neurology - a scoping review
PY  - 2022
ER  -

TY  - JOUR
AU  - Lee, Min
TI  - Large language models for symptoms extraction
PY  - 2021
ER  -
";

/// Kriterien in der Form mit Kürzeln: ein Einschluss- und zwei Ausschlusskriterien.
pub fn criteria(change_note: &str) -> serde_json::Value {
    serde_json::json!({
        "items": [
            {"code": "I1", "kind": "include", "text": "Erwachsene mit Kopfschmerz", "note": "auch Migräne"},
            {"code": "E1", "kind": "exclude", "text": "Falsche Population"},
            {"code": "E2", "kind": "exclude", "text": "Keine Primärstudie"},
        ],
        "guidance": "Im Zweifel einschließen.",
        "include_keywords": "headache, migraine",
        "exclude_keywords": ["review"],
        "change_note": change_note,
    })
}

/// Eine Suche in einer Datenbank; mit `search_id` in `extra` eine neue Version davon.
pub fn search(source: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut data = serde_json::json!({
        "source": source, "platform": "Ovid", "origin": "database",
        "query": "(headache OR migraine) AND adult", "searched_on": "2026-11-19",
        "limits": "ab 2015, Englisch und Deutsch",
    });
    for (key, value) in extra.as_object().into_iter().flatten() {
        data[key] = value.clone();
    }
    data
}

pub fn person(name: &str) -> Identity {
    Identity::generate(name).unwrap()
}

/// Über JSON schicken, wie beim echten Austausch.
pub fn wire(bundle: &Bundle) -> Bundle {
    Bundle::from_json(&bundle.to_json().unwrap()).unwrap()
}

/// Owner, zwei Reviewer und eine Person mit Leserecht, jede mit eigener Datei.
pub struct Team {
    pub dir: TempDir,
    pub owner: Review,
    pub r1: Review,
    pub r2: Review,
    pub viewer: Review,
}

impl Team {
    /// Wie die frühere Fixture: 4 RIS-Einträge (1 DOI-Dublette), 2 BibTeX-Einträge (1 Titel-Dublette).
    pub fn new() -> Team {
        Team::with_template(None)
    }

    /// Dasselbe Team mit einer mitgelieferten Vorlage; ohne Angabe wie ein Review von früher.
    pub fn with_template(template: Option<&str>) -> Team {
        let dir = TempDir::new().unwrap();
        let owner =
            Review::create_with(&dir.path().join("owner.sqlite"), person("owner"), "Testreview", template).unwrap();
        let (r1, r2, viewer) = (person("r1"), person("r2"), person("viewer"));
        owner.set_member(&r1.public(), "reviewer").unwrap();
        owner.set_member(&r2.public(), "reviewer").unwrap();
        owner.set_member(&viewer.public(), "viewer").unwrap();
        let b = owner.import_file("suche.ris", RIS, "Test").unwrap();
        assert_eq!((b.status.as_str(), b.n_records, b.n_duplicates), ("done", 4, 1), "{}", b.error);
        let b = owner.import_file("suche.bib", BIBTEX, "Test").unwrap();
        assert_eq!((b.status.as_str(), b.n_records, b.n_duplicates), ("done", 2, 1), "{}", b.error);
        let join = |id: Identity, file: &str| {
            let pkg = wire(&export_package(&owner, &id.actor_id).unwrap());
            create_from_package(&dir.path().join(file), id, &pkg).unwrap().0
        };
        let r1 = join(r1, "r1.sqlite");
        let r2 = join(r2, "r2.sqlite");
        let viewer = join(viewer, "viewer.sqlite");
        Team { dir, owner, r1, r2, viewer }
    }

    pub fn open_stage(&self, extra: impl FnOnce(&mut OpenStage)) -> ort_core::service::StageView {
        let mut params = OpenStage { name: "Titel/Abstract".into(), ..OpenStage::default() };
        extra(&mut params);
        self.owner.open_stage(&params).unwrap()
    }
}

/// Owner schickt einer Person ein frisches Paket.
pub fn send_package(owner: &Review, to: &Review) -> MergeReport {
    let pkg = wire(&export_package(owner, &to.me().actor_id).unwrap());
    merge(to, &pkg).unwrap()
}

/// Eine Person schickt ihre Rückmeldung an den Owner.
pub fn send_return(from: &Review, owner: &Review) -> MergeReport {
    merge(owner, &wire(&export_return(from).unwrap())).unwrap()
}
