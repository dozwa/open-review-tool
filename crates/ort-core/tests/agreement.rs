//! Übereinstimmungsmaße gegen unabhängige Referenzen (scikit-learn, numpy, publizierte Beispiele).
//! Die Referenzwerte erzeugt `fixtures/make_agreement_fixtures.py`.

use ort_core::agreement::{bootstrap, cohen_pairs, fleiss_kappa, krippendorff_alpha, percent_agreement, Metric};
use serde_json::Value;

const EPS: f64 = 1e-12;

fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/agreement.json")).unwrap()
}

fn votes(v: &Value) -> Vec<(String, String, String)> {
    v["votes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t[0].as_str().unwrap().into(), t[1].as_str().unwrap().into(), t[2].as_str().unwrap().into()))
        .collect()
}

fn to_include(v: &[(String, String, String)]) -> Vec<(String, String, String)> {
    v.iter().map(|(r, a, d)| (r.clone(), a.clone(), if d == "maybe" { "include".into() } else { d.clone() })).collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

#[test]
fn fleiss_wikipedia_example() {
    let f = &fixtures()["fleiss_wikipedia"];
    let k = fleiss_kappa(&votes(f), 14).kappa.unwrap();
    assert!(close(k, f["kappa"].as_f64().unwrap()), "{k}");
    assert_eq!((k * 1000.0).round() / 1000.0, 0.210);
}

#[test]
fn random_three_raters_match_references() {
    let f = &fixtures()["random_b"];
    let v = votes(f);
    assert!(close(fleiss_kappa(&v, 3).kappa.unwrap(), f["fleiss"].as_f64().unwrap()));
    assert!(close(krippendorff_alpha(&v).alpha.unwrap(), f["alpha"].as_f64().unwrap()));
    assert!(close(percent_agreement(&v).percent_agreement.unwrap(), f["percent"].as_f64().unwrap()));

    let pairs = cohen_pairs(&v);
    assert_eq!(pairs.len(), 3);
    for p in &pairs {
        let key = format!("{}-{}", p.rater_a, p.rater_b);
        assert!(close(p.kappa.unwrap(), f["cohen"][&key].as_f64().unwrap()), "Cohen {key}");
    }
    for p in cohen_pairs(&to_include(&v)) {
        let key = format!("{}-{}", p.rater_a, p.rater_b);
        assert!(close(p.kappa.unwrap(), f["cohen_maybe_include"][&key].as_f64().unwrap()), "Cohen Maybe→Include {key}");
        assert!(close(p.pabak.unwrap(), f["pabak_maybe_include"][&key].as_f64().unwrap()), "PABAK {key}");
    }
}

#[test]
fn krippendorff_published_example() {
    let f = &fixtures()["krippendorff_example"];
    let v = votes(f);
    let a = krippendorff_alpha(&v);
    assert_eq!(a.n_items, 11);
    assert!(close(a.alpha.unwrap(), f["alpha"].as_f64().unwrap()));
    assert_eq!((a.alpha.unwrap() * 1000.0).round() / 1000.0, 0.743);
    let fl = fleiss_kappa(&v, 4);
    assert_eq!((fl.n_items, fl.n_items_excluded), (8, 3), "Fleiss meldet ausgeschlossene Records");
}

#[test]
fn bootstrap_is_reproducible_and_contains_estimate() {
    let v = votes(&fixtures()["random_b"]);
    let e1 = bootstrap(&v, &Metric::Alpha, 500, 7, 0.95);
    let e2 = bootstrap(&v, &Metric::Alpha, 500, 7, 0.95);
    assert_eq!(e1, e2);
    let (lo, hi, value) = (e1.ci_low.unwrap(), e1.ci_high.unwrap(), e1.value.unwrap());
    assert!(lo <= value && value <= hi, "{value} in [{lo}, {hi}]");
    assert_eq!(e1.n_items, 150);
    let other = bootstrap(&v, &Metric::Alpha, 500, 8, 0.95);
    assert_ne!(e1.ci_low, other.ci_low, "anderer Seed, andere Stichproben");
    let c = bootstrap(&v, &Metric::Cohen("r0".into(), "r1".into()), 200, 1, 0.95);
    assert!(c.ci_low.unwrap() <= c.value.unwrap() && c.value.unwrap() <= c.ci_high.unwrap());
}

#[test]
fn empty_input_gives_no_numbers() {
    let p = percent_agreement(&[]);
    assert_eq!((p.n_items, p.percent_agreement), (0, None));
    assert_eq!(krippendorff_alpha(&[]).alpha, None);
    assert_eq!(bootstrap(&[], &Metric::Percent, 10, 1, 0.95).ci_low, None);
}
