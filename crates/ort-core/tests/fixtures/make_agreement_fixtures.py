"""Erzeugt agreement.json: Voten und unabhängige Referenzwerte für die Rust-Tests.

Referenzen wie in den früheren SQL-Tests (eventlog/test_projections.py):
- Fleiss' Kappa: Beispiel aus Wikipedia, nachgerechnet mit numpy
- Cohen's Kappa: scikit-learn cohen_kappa_score
- PABAK binär: 2·p_o − 1
- Krippendorffs Alpha: publiziertes Beispiel (0,743) und numpy-Formel

Aufruf (braucht numpy und scikit-learn):
    python crates/ort-core/tests/fixtures/make_agreement_fixtures.py
"""
import json
import os
import random

import numpy as np
from sklearn.metrics import cohen_kappa_score

HERE = os.path.dirname(os.path.abspath(__file__))


def fleiss_ref(table):
    t = np.asarray(table, dtype=float)
    N, _ = t.shape
    n = t.sum(axis=1)[0]
    p_j = t.sum(axis=0) / (N * n)
    P_i = (np.sum(t * t, axis=1) - n) / (n * (n - 1))
    P_bar, P_e = P_i.mean(), np.sum(p_j ** 2)
    return float((P_bar - P_e) / (1 - P_e))


def counts(votes):
    recs = sorted({r for r, _, _ in votes})
    cats = sorted({d for _, _, d in votes})
    out = np.zeros((len(recs), len(cats)))
    for r, _, d in votes:
        out[recs.index(r), cats.index(d)] += 1
    return out


def alpha_ref(votes):
    n_uc = counts(votes)
    m = n_uc.sum(axis=1)
    n_uc, m = n_uc[m >= 2], m[m >= 2]
    n = m.sum()
    disagree = np.sum((m * m - np.sum(n_uc * n_uc, axis=1)) / (m - 1))
    n_c = n_uc.sum(axis=0)
    return float(1 - (n - 1) * disagree / (n * n - np.sum(n_c * n_c)))


def percent_ref(votes):
    n_uc = counts(votes)
    m = n_uc.sum(axis=1)
    keep = m >= 2
    return float(((n_uc[keep] == m[keep, None]).any(axis=1)).mean())


def main():
    # A) Fleiss-Beispiel (Wikipedia): 10 Subjekte, 14 Rater, 5 Kategorien
    table = [[0, 0, 0, 0, 14], [0, 2, 6, 4, 2], [0, 0, 3, 5, 6], [0, 3, 9, 2, 0], [2, 2, 8, 1, 1],
             [7, 7, 0, 0, 0], [3, 2, 6, 3, 0], [2, 5, 3, 2, 2], [6, 5, 2, 1, 0], [0, 2, 2, 3, 7]]
    votes_a = []
    for r, row in enumerate(table):
        rater = 0
        for k, cnt in enumerate(row):
            for _ in range(cnt):
                votes_a.append([f"A{r}", f"rater{rater:02d}", f"c{k + 1}"])
                rater += 1

    # B) 3 Reviewer, 150 Records, include/maybe/exclude, korreliert (gleicher Seed wie früher)
    rng = random.Random(42)
    cats = ["include", "maybe", "exclude"]
    B = {0: [], 1: [], 2: []}
    for _ in range(150):
        truth = rng.choices(cats, weights=[0.2, 0.15, 0.65])[0]
        for a in range(3):
            B[a].append(truth if rng.random() < 0.75 else rng.choice(cats))
    votes_b = [[f"B{r:03d}", f"r{a}", B[a][r]] for r in range(150) for a in range(3)]
    tB = np.array([[sum(1 for a in range(3) if B[a][r] == c) for c in cats] for r in range(150)])
    to_inc = lambda d: "include" if d == "maybe" else d
    cohen, cohen_maybe, pabak = {}, {}, {}
    for a in range(3):
        for b in range(a + 1, 3):
            key = f"r{a}-r{b}"
            cohen[key] = float(cohen_kappa_score(B[a], B[b]))
            cohen_maybe[key] = float(cohen_kappa_score([to_inc(d) for d in B[a]], [to_inc(d) for d in B[b]]))
            po = float(np.mean([to_inc(x) == to_inc(y) for x, y in zip(B[a], B[b])]))
            pabak[key] = 2 * po - 1

    # K) Krippendorff-Beispiel: 12 Units, 4 Coder, fehlende Werte, publiziert alpha = 0.743
    K = [[1, 1, None, 1], [2, 2, 3, 2], [3, 3, 3, 3], [3, 3, 3, 3], [2, 2, 2, 2], [1, 2, 3, 4],
         [4, 4, 4, 4], [1, 1, 2, 1], [2, 2, 2, 2], [None, 5, 5, 5], [None, None, 1, 1], [None, None, 3, None]]
    votes_k = [[f"K{u + 1:02d}", f"coder{c}", f"v{v}"] for u, row in enumerate(K) for c, v in enumerate(row)
               if v is not None]

    out = {
        "fleiss_wikipedia": {"votes": votes_a, "n": 14, "kappa": fleiss_ref(table)},
        "random_b": {
            "votes": votes_b,
            "fleiss": fleiss_ref(tB),
            "alpha": alpha_ref(votes_b),
            "percent": percent_ref(votes_b),
            "cohen": cohen,
            "cohen_maybe_include": cohen_maybe,
            "pabak_maybe_include": pabak,
        },
        "krippendorff_example": {"votes": votes_k, "alpha": alpha_ref(votes_k), "published": 0.743, "n_items": 11,
                                 "fleiss_n_items": 8, "fleiss_excluded": 3},
    }
    with open(os.path.join(HERE, "agreement.json"), "w") as f:
        json.dump(out, f, indent=1)
        f.write("\n")
    print("agreement.json geschrieben")


if __name__ == "__main__":
    main()
