"""Simulation: Überdeckung der 95-%-Bootstrap-Intervalle aus agreement.py.

Szenario wie Titel/Abstract-Screening: 10 % relevante Records, zwei Reviewer mit
Sensitivität 0.9 und Spezifität 0.95 (unabhängige Fehler), 500 Records pro Review.
Der "wahre" Wert wird aus einer sehr großen Stichprobe (1 Mio. Records) bestimmt.
"""
import time
import numpy as np
import agreement as ag

PREV, SENS, SPEC, N, REPS, B = 0.10, 0.90, 0.95, 500, 300, 1000


def sample(rng, n):
    truth = rng.random(n) < PREV
    def rate():
        return np.where(truth, rng.random(n) < SENS, rng.random(n) >= SPEC)
    return rate().astype(int), rate().astype(int)


def kappa(x, y):
    p_o = np.mean(x == y)
    p_e = np.mean(x) * np.mean(y) + np.mean(1 - x) * np.mean(1 - y)
    return (p_o - p_e) / (1 - p_e)


def votes(x, y):
    return [(i, "A", "inc" if x[i] else "exc") for i in range(len(x))] + \
           [(i, "B", "inc" if y[i] else "exc") for i in range(len(y))]


rng = np.random.default_rng(1)
X, Y = sample(rng, 1_000_000)
true = {"cohen": kappa(X, Y), "percent": float(np.mean(X == Y)), "alpha": None}
true["alpha"] = ag.krippendorff_alpha(np.stack([X, Y], axis=1).astype(np.int16), 2)

t0 = time.time()
hits = {m: 0 for m in true}
widths = {m: [] for m in true}
for r in range(REPS):
    x, y = sample(rng, N)
    v = votes(x, y)
    for m in true:
        e = ag.bootstrap(v, m, n_boot=B, seed=r, pair=("A", "B") if m == "cohen" else None)
        hits[m] += e.ci_low <= true[m] <= e.ci_high
        widths[m].append(e.ci_high - e.ci_low)

print(f"Szenario: Prävalenz {PREV}, Sens {SENS}, Spez {SPEC}, n={N}, {REPS} Wiederholungen, B={B}")
for m in true:
    print(f"{m:8s} wahr={true[m]:.4f}  Überdeckung={hits[m]/REPS:.3f}  mittlere Breite={np.mean(widths[m]):.3f}")
print(f"Laufzeit {time.time()-t0:.0f} s")
