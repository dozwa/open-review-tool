//! Übereinstimmungsmaße einer Stufe und Bootstrap-Konfidenzintervalle.
//!
//! Eingabe sind Voten (record_id, rater, decision), z. B. aus
//! [`StageState::stage_votes`](crate::projection::StageState::stage_votes). Die Formeln entsprechen
//! den früheren SQL-Funktionen (`percent_agreement`, `fleiss_kappa`, `krippendorff_alpha`,
//! `cohen_kappa_pairs`) und `eventlog/agreement.py`; die Referenzwerte stehen in
//! `tests/fixtures/agreement.json`.
//!
//! Kappa nie allein berichten: Bei stark unausgeglichenen Kategorien (Kappa-Paradox) gehören
//! Prozent-Übereinstimmung und PABAK dazu.

use std::collections::{BTreeMap, BTreeSet};

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::Serialize;

/// Records als Einheiten, jede mit ihren Voten (Rater-Index, Kategorie-Index).
struct Encoded {
    raters: Vec<String>,
    k: usize,
    units: Vec<Vec<(usize, usize)>>,
}

fn encode(votes: &[(String, String, String)]) -> Encoded {
    let raters: Vec<String> = votes.iter().map(|v| v.1.clone()).collect::<BTreeSet<_>>().into_iter().collect();
    let cats: Vec<String> = votes.iter().map(|v| v.2.clone()).collect::<BTreeSet<_>>().into_iter().collect();
    let ri: BTreeMap<&String, usize> = raters.iter().enumerate().map(|(i, r)| (r, i)).collect();
    let ci: BTreeMap<&String, usize> = cats.iter().enumerate().map(|(i, c)| (c, i)).collect();
    let mut units: BTreeMap<&String, Vec<(usize, usize)>> = BTreeMap::new();
    for (record, rater, decision) in votes {
        units.entry(record).or_default().push((ri[rater], ci[decision]));
    }
    Encoded { k: cats.len(), raters, units: units.into_values().collect() }
}

fn counts(unit: &[(usize, usize)], k: usize) -> Vec<f64> {
    let mut c = vec![0.0; k];
    for &(_, cat) in unit {
        c[cat] += 1.0;
    }
    c
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Percent {
    pub n_items: usize,
    pub percent_agreement: Option<f64>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Fleiss {
    pub n_items: usize,
    pub n_items_excluded: usize,
    pub raters_per_item: usize,
    pub p_bar: Option<f64>,
    pub p_e: Option<f64>,
    pub kappa: Option<f64>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Alpha {
    pub n_items: usize,
    pub n_pairable_values: usize,
    pub d_o: Option<f64>,
    pub d_e: Option<f64>,
    pub alpha: Option<f64>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CohenPair {
    pub rater_a: String,
    pub rater_b: String,
    pub n_items: usize,
    pub p_o: Option<f64>,
    pub p_e: Option<f64>,
    pub kappa: Option<f64>,
    pub pabak: Option<f64>,
}

fn percent_units(units: &[&[(usize, usize)]], k: usize) -> Percent {
    let mut n = 0;
    let mut agree = 0;
    for unit in units {
        if unit.len() >= 2 {
            n += 1;
            if counts(unit, k).iter().filter(|&&c| c > 0.0).count() == 1 {
                agree += 1;
            }
        }
    }
    Percent { n_items: n, percent_agreement: (n > 0).then(|| agree as f64 / n as f64) }
}

fn fleiss_units(units: &[&[(usize, usize)]], k: usize, n: usize) -> Fleiss {
    let items: Vec<Vec<f64>> = units.iter().filter(|u| u.len() == n).map(|u| counts(u, k)).collect();
    let excluded = units.iter().filter(|u| u.len() >= 2 && u.len() != n).count();
    let mut out = Fleiss {
        n_items: items.len(),
        n_items_excluded: excluded,
        raters_per_item: n,
        p_bar: None,
        p_e: None,
        kappa: None,
    };
    if items.is_empty() || n < 2 {
        return out;
    }
    let nf = n as f64;
    let total = items.len() as f64 * nf;
    let p_bar = items.iter().map(|c| (c.iter().map(|x| x * x).sum::<f64>() - nf) / (nf * (nf - 1.0))).sum::<f64>()
        / items.len() as f64;
    let p_e: f64 = (0..k).map(|j| items.iter().map(|c| c[j]).sum::<f64>() / total).map(|p| p * p).sum();
    out.p_bar = Some(p_bar);
    out.p_e = Some(p_e);
    out.kappa = (p_e < 1.0).then(|| (p_bar - p_e) / (1.0 - p_e));
    out
}

fn alpha_units(units: &[&[(usize, usize)]], k: usize) -> Alpha {
    let pairable: Vec<Vec<f64>> = units.iter().filter(|u| u.len() >= 2).map(|u| counts(u, k)).collect();
    let n: f64 = pairable.iter().map(|c| c.iter().sum::<f64>()).sum();
    let mut out = Alpha { n_items: pairable.len(), n_pairable_values: n as usize, d_o: None, d_e: None, alpha: None };
    if pairable.is_empty() {
        return out;
    }
    let disagree: f64 = pairable
        .iter()
        .map(|c| {
            let m: f64 = c.iter().sum();
            (m * m - c.iter().map(|x| x * x).sum::<f64>()) / (m - 1.0)
        })
        .sum();
    let sq_c: f64 = (0..k).map(|j| pairable.iter().map(|c| c[j]).sum::<f64>()).map(|x| x * x).sum();
    let denom = n * n - sq_c;
    out.d_o = Some(disagree / n);
    out.d_e = Some(denom / (n * (n - 1.0)));
    out.alpha = (denom > 0.0).then(|| 1.0 - (n - 1.0) * disagree / denom);
    out
}

/// Cohen's Kappa und PABAK für ein Paar. `k_present` = Zahl der Kategorien in der ganzen Stufe.
fn cohen_units(
    units: &[&[(usize, usize)]],
    k: usize,
    k_present: usize,
    a: usize,
    b: usize,
) -> (usize, Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    let mut pairs = Vec::new();
    for unit in units {
        let x = unit.iter().find(|v| v.0 == a).map(|v| v.1);
        let y = unit.iter().find(|v| v.0 == b).map(|v| v.1);
        if let (Some(x), Some(y)) = (x, y) {
            pairs.push((x, y));
        }
    }
    let n = pairs.len();
    if n == 0 {
        return (0, None, None, None, None);
    }
    let nf = n as f64;
    let p_o = pairs.iter().filter(|(x, y)| x == y).count() as f64 / nf;
    let p_e: f64 = (0..k)
        .map(|c| {
            let ca = pairs.iter().filter(|p| p.0 == c).count() as f64;
            let cb = pairs.iter().filter(|p| p.1 == c).count() as f64;
            ca * cb
        })
        .sum::<f64>()
        / (nf * nf);
    let kappa = (p_e < 1.0).then(|| (p_o - p_e) / (1.0 - p_e));
    let kf = k_present as f64;
    let pabak = (k_present > 1).then(|| (kf * p_o - 1.0) / (kf - 1.0));
    (n, Some(p_o), Some(p_e), kappa, pabak)
}

fn refs(e: &Encoded) -> Vec<&[(usize, usize)]> {
    e.units.iter().map(Vec::as_slice).collect()
}

/// Anteil der Records mit mindestens zwei Voten, bei denen alle übereinstimmen.
pub fn percent_agreement(votes: &[(String, String, String)]) -> Percent {
    let e = encode(votes);
    percent_units(&refs(&e), e.k)
}

/// Übereinstimmung als ganze Zahlen: (einstimmige Records, Records mit mindestens zwei Voten).
/// Dieselbe Zählung wie [`percent_agreement`], für Gates im Log, wo Gleitkommazahlen verboten sind.
pub fn agreement_counts(votes: &[(String, String, String)]) -> (i64, i64) {
    let mut units: BTreeMap<&String, BTreeSet<&String>> = BTreeMap::new();
    let mut sizes: BTreeMap<&String, i64> = BTreeMap::new();
    for (record, _, decision) in votes {
        units.entry(record).or_default().insert(decision);
        *sizes.entry(record).or_default() += 1;
    }
    let rated: Vec<&&String> = sizes.iter().filter(|(_, n)| **n >= 2).map(|(r, _)| r).collect();
    let agree = rated.iter().filter(|r| units[**r].len() == 1).count();
    (agree as i64, rated.len() as i64)
}

/// Fleiss' Kappa über alle Records mit genau `n` Voten; meldet, wie viele Records herausfallen.
pub fn fleiss_kappa(votes: &[(String, String, String)], n: usize) -> Fleiss {
    let e = encode(votes);
    fleiss_units(&refs(&e), e.k, n)
}

/// Krippendorffs Alpha (nominal) über alle Records mit mindestens zwei Voten, beliebige Votenzahl.
pub fn krippendorff_alpha(votes: &[(String, String, String)]) -> Alpha {
    let e = encode(votes);
    alpha_units(&refs(&e), e.k)
}

/// Cohen's Kappa und PABAK (Byrt et al. 1993) für jedes Rater-Paar auf gemeinsamen Records.
pub fn cohen_pairs(votes: &[(String, String, String)]) -> Vec<CohenPair> {
    let e = encode(votes);
    let units = refs(&e);
    let mut out = Vec::new();
    for a in 0..e.raters.len() {
        for b in (a + 1)..e.raters.len() {
            let (n, p_o, p_e, kappa, pabak) = cohen_units(&units, e.k, e.k, a, b);
            if n > 0 {
                out.push(CohenPair {
                    rater_a: e.raters[a].clone(),
                    rater_b: e.raters[b].clone(),
                    n_items: n,
                    p_o,
                    p_e,
                    kappa,
                    pabak,
                });
            }
        }
    }
    out
}

#[derive(Clone, Debug)]
pub enum Metric {
    Percent,
    Alpha,
    Fleiss(usize),
    Cohen(String, String),
    Pabak(String, String),
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Estimate {
    pub value: Option<f64>,
    pub ci_low: Option<f64>,
    pub ci_high: Option<f64>,
    pub level: f64,
    pub n_items: usize,
    pub n_boot: usize,
    pub seed: u64,
}

/// Perzentil-Bootstrap über Records (ein Record wird mit allen seinen Voten gezogen).
/// Der Seed wird mitgeliefert, damit das Ergebnis reproduzierbar ist.
pub fn bootstrap(
    votes: &[(String, String, String)],
    metric: &Metric,
    n_boot: usize,
    seed: u64,
    level: f64,
) -> Estimate {
    let e = encode(votes);
    let units = refs(&e);
    let index = |name: &str| e.raters.iter().position(|r| r == name);
    let f = |sample: &[&[(usize, usize)]]| -> Option<f64> {
        match metric {
            Metric::Percent => percent_units(sample, e.k).percent_agreement,
            Metric::Alpha => alpha_units(sample, e.k).alpha,
            Metric::Fleiss(n) => fleiss_units(sample, e.k, *n).kappa,
            Metric::Cohen(a, b) => cohen_units(sample, e.k, e.k, index(a)?, index(b)?).3,
            Metric::Pabak(a, b) => cohen_units(sample, e.k, e.k, index(a)?, index(b)?).4,
        }
    };
    let value = f(&units);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut reps = Vec::with_capacity(n_boot);
    if !units.is_empty() {
        let mut sample = Vec::with_capacity(units.len());
        for _ in 0..n_boot {
            sample.clear();
            for _ in 0..units.len() {
                sample.push(units[rng.gen_range(0..units.len())]);
            }
            if let Some(v) = f(&sample) {
                if v.is_finite() {
                    reps.push(v);
                }
            }
        }
    }
    reps.sort_by(|a, b| a.total_cmp(b));
    let alpha = (1.0 - level) / 2.0;
    Estimate {
        value,
        ci_low: quantile(&reps, alpha),
        ci_high: quantile(&reps, 1.0 - alpha),
        level,
        n_items: units.len(),
        n_boot: reps.len(),
        seed,
    }
}

/// Quantil mit linearer Interpolation (wie `numpy.quantile` im Standardmodus).
fn quantile(sorted: &[f64], q: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let pos = (sorted.len() - 1) as f64 * q;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    Some(sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantile_matches_numpy_linear() {
        let v = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(quantile(&v, 0.5), Some(2.5));
        assert_eq!(quantile(&v, 0.0), Some(1.0));
        assert!((quantile(&v, 0.1).unwrap() - 1.3).abs() < 1e-12);
    }
}
