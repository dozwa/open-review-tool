"""Agreement-Statistiken mit Bootstrap-Konfidenzintervallen (Anwendungsschicht).

Eingabe: Votenliste [(record_id, rater_id, decision), ...], z. B. aus der
SQL-Funktion vote_matrix(stage, maybe_as). Punktschätzer sind identisch zu den
SQL-Projektionen; die Intervalle entstehen per Perzentil-Bootstrap über Records
(Cluster-Bootstrap: ein Record wird mit allen seinen Voten gezogen).
Der Seed wird mit ausgegeben, damit das Ergebnis reproduzierbar ist.
"""
from __future__ import annotations
from collections import defaultdict
from dataclasses import dataclass
import numpy as np


def _encode(votes):
    recs = sorted({r for r, _, _ in votes})
    raters = sorted({a for _, a, _ in votes})
    cats = sorted({d for _, _, d in votes})
    ri = {r: i for i, r in enumerate(recs)}
    ai = {a: i for i, a in enumerate(raters)}
    ci = {c: i for i, c in enumerate(cats)}
    # Matrix records x raters, -1 = kein Votum
    M = np.full((len(recs), len(raters)), -1, dtype=np.int16)
    for r, a, d in votes:
        M[ri[r], ai[a]] = ci[d]
    return M, raters, cats


def _counts(M, k):
    """n_uc: Anzahl Voten pro Record und Kategorie."""
    out = np.zeros((M.shape[0], k))
    for c in range(k):
        out[:, c] = (M == c).sum(axis=1)
    return out


def percent_agreement(M, k):
    n = _counts(M, k)
    m = n.sum(axis=1)
    keep = m >= 2
    if not keep.any():
        return np.nan
    return float(((n[keep] == m[keep, None]).any(axis=1)).mean())


def fleiss_kappa(M, k, n_raters):
    n = _counts(M, k)
    n = n[n.sum(axis=1) == n_raters]
    if len(n) == 0:
        return np.nan
    p_j = n.sum(axis=0) / (len(n) * n_raters)
    P_i = (np.sum(n * n, axis=1) - n_raters) / (n_raters * (n_raters - 1))
    P_e = np.sum(p_j ** 2)
    return float((P_i.mean() - P_e) / (1 - P_e)) if P_e < 1 else np.nan


def krippendorff_alpha(M, k):
    n_uc = _counts(M, k)
    m = n_uc.sum(axis=1)
    keep = m >= 2
    n_uc, m = n_uc[keep], m[keep]
    if len(m) == 0:
        return np.nan
    n = m.sum()
    disagree = np.sum((m * m - np.sum(n_uc * n_uc, axis=1)) / (m - 1))
    n_c = n_uc.sum(axis=0)
    denom = n * n - np.sum(n_c * n_c)
    return float(1 - (n - 1) * disagree / denom) if denom > 0 else np.nan


def cohen_kappa(M, k, a, b):
    both = (M[:, a] >= 0) & (M[:, b] >= 0)
    x, y = M[both, a], M[both, b]
    if len(x) == 0:
        return np.nan
    p_o = np.mean(x == y)
    p_e = sum(np.mean(x == c) * np.mean(y == c) for c in range(k))
    return float((p_o - p_e) / (1 - p_e)) if p_e < 1 else np.nan


def pabak(M, k_present, a, b):
    both = (M[:, a] >= 0) & (M[:, b] >= 0)
    p_o = np.mean(M[both, a] == M[both, b])
    return float((k_present * p_o - 1) / (k_present - 1))


@dataclass
class Estimate:
    metric: str
    value: float
    ci_low: float
    ci_high: float
    n_items: int
    n_boot: int
    seed: int


def bootstrap(votes, metric: str, n_boot: int = 2000, seed: int = 20261117,
              level: float = 0.95, n_raters: int | None = None, pair: tuple | None = None):
    """Perzentil-Bootstrap über Records. metric in
    {'percent', 'fleiss', 'alpha', 'cohen', 'pabak'}; für 'cohen'/'pabak' pair=(rater_a, rater_b)."""
    M, raters, cats = _encode(votes)
    k = len(cats)
    if metric == "percent":
        f = lambda X: percent_agreement(X, k)
    elif metric == "fleiss":
        f = lambda X: fleiss_kappa(X, k, n_raters)
    elif metric == "alpha":
        f = lambda X: krippendorff_alpha(X, k)
    elif metric in ("cohen", "pabak"):
        a, b = raters.index(pair[0]), raters.index(pair[1])
        f = (lambda X: cohen_kappa(X, k, a, b)) if metric == "cohen" else (lambda X: pabak(X, k, a, b))
    else:
        raise ValueError(metric)
    point = f(M)
    rng = np.random.default_rng(seed)
    idx = rng.integers(0, M.shape[0], size=(n_boot, M.shape[0]))
    reps = np.array([f(M[i]) for i in idx])
    reps = reps[~np.isnan(reps)]
    lo, hi = np.quantile(reps, [(1 - level) / 2, 1 - (1 - level) / 2])
    return Estimate(metric, point, float(lo), float(hi), int(M.shape[0]), len(reps), seed)
