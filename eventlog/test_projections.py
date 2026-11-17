"""Tests für final_decision und agreement-Projektionen.

Startet eine Wegwerf-PostgreSQL-Instanz, spielt Ereignisse ein und vergleicht
die SQL-Ergebnisse mit unabhängigen Referenzen (scikit-learn für Cohen's Kappa,
eigene numpy-Implementierung und das Fleiss-Beispiel von Wikipedia für Fleiss' Kappa).
"""
import json, os, random, shutil, subprocess, sys, tempfile
import numpy as np
from sklearn.metrics import cohen_kappa_score

PG = "/usr/lib/postgresql/16/bin"
HERE = os.path.dirname(os.path.abspath(__file__))
REVIEW = "00000000-0000-0000-0000-000000000001"


def uid(prefix: int, i: int) -> str:
    return f"00000000-0000-0000-{prefix:04d}-{i:012d}"


class Log:
    def __init__(self):
        self.seq = 0
        self.sql = []

    def append(self, typ, actor, stage, payload):
        self.sql.append(
            f"SELECT append_event('{REVIEW}',{self.seq},'{typ}','{actor}',"
            f"{('NULL' if stage is None else repr(stage))}::uuid,$j${json.dumps(payload)}$j$::jsonb);")
        self.seq += 1


def run_psql(sock, port, sql, user_run):
    path = os.path.join(sock, "run.sql")
    with open(path, "w") as f:
        f.write(sql)
    os.chmod(path, 0o644)
    out = subprocess.run(user_run + [f"{PG}/psql -h {sock} -p {port} -U pg -d postgres -v ON_ERROR_STOP=1 -At -F '|' -f {path}"],
                         capture_output=True, text=True)
    if out.returncode != 0:
        print(out.stdout, out.stderr)
        raise SystemExit("psql failed")
    return out.stdout


def fleiss_ref(table):
    t = np.asarray(table, dtype=float)
    N, _ = t.shape
    n = t.sum(axis=1)[0]
    p_j = t.sum(axis=0) / (N * n)
    P_i = (np.sum(t * t, axis=1) - n) / (n * (n - 1))
    P_bar, P_e = P_i.mean(), np.sum(p_j ** 2)
    return (P_bar - P_e) / (1 - P_e)


def main():
    tmp = tempfile.mkdtemp(prefix="pgt")
    os.chmod(tmp, 0o777)
    run = ["su", "nobody", "-s", "/bin/bash", "-c"] if os.geteuid() == 0 else ["bash", "-c"]
    port = 5498
    subprocess.run(run + [f"{PG}/initdb -D {tmp}/data -A trust -U pg >/dev/null"], check=True)
    subprocess.run(run + [f"{PG}/pg_ctl -D {tmp}/data -o '-k {tmp} -p {port} -c listen_addresses=' -l {tmp}/log -w start >/dev/null"], check=True)
    failures = 0
    try:
        log = Log()
        setup = open(os.path.join(HERE, "schema.sql")).read() + open(os.path.join(HERE, "projections.sql")).read()
        setup += f"\nINSERT INTO review (review_id, title) VALUES ('{REVIEW}','Test');\n"
        sys_actor = uid(9, 0)

        # A) Fleiss-Beispiel (Wikipedia): 10 Subjekte, 14 Rater, 5 Kategorien
        fleiss_table = [[0,0,0,0,14],[0,2,6,4,2],[0,0,3,5,6],[0,3,9,2,0],[2,2,8,1,1],
                        [7,7,0,0,0],[3,2,6,3,0],[2,5,3,2,2],[6,5,2,1,0],[0,2,2,3,7]]
        stA = uid(1, 1)
        log.append("StageInstanceOpened", sys_actor, stA, {"min_reviewers": 14})
        for r, row in enumerate(fleiss_table):
            rater = 0
            for k, cnt in enumerate(row):
                for _ in range(cnt):
                    log.append("VoteCast", uid(2, rater), stA, {"record_id": f"A{r}", "decision": f"c{k+1}"})
                    rater += 1

        # B) Cohen: 3 Reviewer, 150 Records, include/maybe/exclude, korreliert
        rng = random.Random(42)
        stB = uid(1, 2)
        log.append("StageInstanceOpened", sys_actor, stB, {"min_reviewers": 3})
        cats = ["include", "maybe", "exclude"]
        B = {0: [], 1: [], 2: []}
        for r in range(150):
            truth = rng.choices(cats, weights=[0.2, 0.15, 0.65])[0]
            for a in range(3):
                d = truth if rng.random() < 0.75 else rng.choice(cats)
                B[a].append(d)
                log.append("VoteCast", uid(3, a), stB, {"record_id": f"B{r}", "decision": d})

        # C) final_decision-Szenarien, min_reviewers = 2
        stC = uid(1, 3)
        a, b = uid(4, 1), uid(4, 2)
        log.append("StageInstanceOpened", sys_actor, stC, {"min_reviewers": 2})
        for rec, va, vb in [("r1","include","include"), ("r2","include",None), ("r3","include","exclude"),
                            ("r4","include","exclude"), ("r5","include","exclude"), ("r6","maybe","include")]:
            log.append("VoteCast", a, stC, {"record_id": rec, "decision": va})
            if vb:
                log.append("VoteCast", b, stC, {"record_id": rec, "decision": vb})
        log.append("ConsensusRecorded", a, stC, {"record_id": "r4", "decision": "exclude", "method": "discussion"})
        log.append("VoteRevised", b, stC, {"record_id": "r5", "decision": "include"})
        # "nicht entscheidbar": r7 markiert + ein Votum -> Klärung; r8 markiert, dann Konsens
        log.append("RecordFlaggedUndecidable", a, stC, {"record_id": "r7", "reason": "abstract_missing"})
        log.append("VoteCast", b, stC, {"record_id": "r7", "decision": "include"})
        log.append("RecordFlaggedUndecidable", a, stC, {"record_id": "r8", "reason": "language"})
        log.append("ConsensusRecorded", b, stC, {"record_id": "r8", "decision": "exclude", "method": "third_reviewer"})

        # D) Maybe-Regel aus dem Protokoll: maybe_as = include; ein späteres Opened darf nichts ändern
        stD = uid(1, 4)
        log.append("StageInstanceOpened", sys_actor, stD, {"min_reviewers": 2, "maybe_as": "include"})
        log.append("StageInstanceOpened", sys_actor, stD, {"min_reviewers": 2, "maybe_as": None})
        for rec, va, vb in [("d1", "maybe", "include"), ("d2", "exclude", "exclude"), ("d3", "maybe", "exclude")]:
            log.append("VoteCast", a, stD, {"record_id": rec, "decision": va})
            log.append("VoteCast", b, stD, {"record_id": rec, "decision": vb})

        # K) Krippendorff-Beispiel (12 Units, 4 Coder, fehlende Werte; erwartet alpha = 0.743)
        stK = uid(1, 5)
        log.append("StageInstanceOpened", sys_actor, stK, {"min_reviewers": 4})
        K = [[1,1,None,1],[2,2,3,2],[3,3,3,3],[3,3,3,3],[2,2,2,2],[1,2,3,4],
             [4,4,4,4],[1,1,2,1],[2,2,2,2],[None,5,5,5],[None,None,1,1],[None,None,3,None]]
        for u, row in enumerate(K):
            for coder, val in enumerate(row):
                if val is not None:
                    log.append("VoteCast", uid(5, coder), stK, {"record_id": f"K{u+1:02d}", "decision": f"v{val}"})

        queries = f"""
\\echo ##D_FINAL
SELECT record_id, coalesce(decision,'-'), status FROM final_decision WHERE stage_instance_id = '{stD}' ORDER BY 1;
\\echo ##D_PCT
SELECT percent_agreement FROM percent_agreement('{stD}');
\\echo ##D_PCT_RAW
SELECT percent_agreement FROM percent_agreement('{stD}', NULL);
\\echo ##ALPHA_K
SELECT n_items, alpha FROM krippendorff_alpha('{stK}');
\\echo ##FLEISS_K
SELECT n_items, n_items_excluded FROM fleiss_kappa('{stK}');
\\echo ##ALPHA_B
SELECT alpha FROM krippendorff_alpha('{stB}');
\\echo ##PCT_B
SELECT percent_agreement FROM percent_agreement('{stB}');
\\echo ##FLEISS
SELECT kappa FROM fleiss_kappa('{stA}');
\\echo ##FLEISS_B
SELECT kappa FROM fleiss_kappa('{stB}');
\\echo ##COHEN
SELECT rater_a, rater_b, kappa FROM cohen_kappa_pairs('{stB}') ORDER BY 1,2;
\\echo ##COHEN_MAYBE
SELECT rater_a, rater_b, kappa FROM cohen_kappa_pairs('{stB}', 'include') ORDER BY 1,2;
\\echo ##PABAK
SELECT rater_a, rater_b, pabak FROM cohen_kappa_pairs('{stB}', 'include') ORDER BY 1,2;
\\echo ##FINAL
SELECT record_id, coalesce(decision,'-'), status, n_votes FROM final_decision WHERE stage_instance_id = '{stC}' ORDER BY 1;
\\echo ##PCT
SELECT n_items, percent_agreement FROM percent_agreement('{stC}');
\\echo ##PCT_MAYBE
SELECT n_items, percent_agreement FROM percent_agreement('{stC}', 'include');
"""
        out = run_psql(tmp, port, setup + "\n".join(log.sql) + queries, run)
        sections, cur = {}, None
        for line in out.splitlines():
            if line.startswith("##"):
                cur = line[2:]; sections[cur] = []
            elif cur and line.strip():
                sections[cur].append(line)

        def check(name, cond, detail):
            nonlocal failures
            print(("OK   " if cond else "FAIL ") + name + " — " + detail)
            failures += 0 if cond else 1

        k = float(sections["FLEISS"][0])
        check("Fleiss Wikipedia-Beispiel", abs(k - fleiss_ref(fleiss_table)) < 1e-12 and round(k, 3) == 0.210,
              f"SQL {k:.6f}, Referenz {fleiss_ref(fleiss_table):.6f}")

        tB = np.array([[sum(1 for a in range(3) if B[a][r] == c) for c in cats] for r in range(150)])
        kB = float(sections["FLEISS_B"][0])
        check("Fleiss Zufallsdaten (3 Rater)", abs(kB - fleiss_ref(tB)) < 1e-12, f"SQL {kB:.6f}, Referenz {fleiss_ref(tB):.6f}")

        for label, sec, mapping in [("Cohen", "COHEN", None), ("Cohen Maybe→Include", "COHEN_MAYBE", "include")]:
            for line in sections[sec]:
                ra, rb, kv = line.split("|")
                ia, ib = int(ra[-12:]), int(rb[-12:])
                m = (lambda d: mapping if (mapping and d == "maybe") else d)
                ref = cohen_kappa_score([m(d) for d in B[ia]], [m(d) for d in B[ib]])
                check(f"{label} Paar {ia}-{ib}", abs(float(kv) - ref) < 1e-12, f"SQL {float(kv):.6f}, sklearn {ref:.6f}")

        for line in sections["PABAK"]:
            ra, rb, pv = line.split("|")
            ia, ib = int(ra[-12:]), int(rb[-12:])
            m = lambda d: "include" if d == "maybe" else d
            po = np.mean([m(x) == m(y) for x, y in zip(B[ia], B[ib])])
            check(f"PABAK binär Paar {ia}-{ib}", abs(float(pv) - (2 * po - 1)) < 1e-12,
                  f"SQL {float(pv):.6f}, 2·p_o−1 = {2 * po - 1:.6f}")

        expected = {"r1": ("include", "unanimous", "2"), "r2": ("-", "pending", "1"),
                    "r3": ("-", "disagreement", "2"), "r4": ("exclude", "consensus", "2"),
                    "r5": ("include", "unanimous", "2"), "r6": ("-", "disagreement", "2"),
                    "r7": ("-", "needs_clarification", "1"), "r8": ("exclude", "consensus", "0")}
        got = {l.split("|")[0]: tuple(l.split("|")[1:]) for l in sections["FINAL"]}
        for rec, exp in expected.items():
            check(f"final_decision {rec}", got.get(rec) == exp, f"erwartet {exp}, erhalten {got.get(rec)}")

        got = {l.split("|")[0]: tuple(l.split("|")[1:]) for l in sections["D_FINAL"]}
        expD = {"d1": ("include", "unanimous"), "d2": ("exclude", "unanimous"), "d3": ("-", "disagreement")}
        for rec, exp in expD.items():
            check(f"Maybe-Regel aus Protokoll, final_decision {rec}", got.get(rec) == exp, f"erwartet {exp}, erhalten {got.get(rec)}")
        pd_ = float(sections["D_PCT"][0]); pr_ = float(sections["D_PCT_RAW"][0])
        check("Maybe-Regel: Standard folgt Protokoll", abs(pd_ - 2/3) < 1e-12, f"{pd_:.3f} (erwartet 0.667)")
        check("Maybe-Regel: explizite Abweichung (Sensitivität)", abs(pr_ - 1/3) < 1e-12, f"{pr_:.3f} (erwartet 0.333)")

        nK, aK = sections["ALPHA_K"][0].split("|")
        Kvotes = [(f"K{u+1:02d}", c, f"v{v}") for u, row in enumerate(K) for c, v in enumerate(row) if v is not None]
        import agreement as ag
        M, _, cats = ag._encode(Kvotes)
        refK = ag.krippendorff_alpha(M, len(cats))
        check("Krippendorff-Beispiel (publiziert 0.743)", nK == "11" and round(float(aK), 3) == 0.743 and abs(float(aK) - refK) < 1e-12,
              f"SQL {float(aK):.6f}, Python {refK:.6f}, Units mit >= 2 Werten: {nK} (erwartet 11)")
        nf, nx = sections["FLEISS_K"][0].split("|")
        check("Fleiss meldet ausgeschlossene Records", (nf, nx) == ("8", "3"), f"genutzt {nf}, ausgeschlossen {nx} (erwartet 8, 3)")

        Bvotes = [(f"B{r}", a_, B[a_][r]) for r in range(150) for a_ in range(3)]
        M, raters, cats = ag._encode(Bvotes)
        for name, sqlv, pyv in [("Alpha", sections["ALPHA_B"][0], ag.krippendorff_alpha(M, len(cats))),
                                ("Prozent", sections["PCT_B"][0], ag.percent_agreement(M, len(cats))),
                                ("Fleiss", sections["FLEISS_B"][0], ag.fleiss_kappa(M, len(cats), 3))]:
            check(f"Python = SQL: {name} (Stufe B)", abs(float(sqlv) - pyv) < 1e-12, f"SQL {float(sqlv):.6f}, Python {pyv:.6f}")
        e1 = ag.bootstrap(Bvotes, "alpha", n_boot=500, seed=7)
        e2 = ag.bootstrap(Bvotes, "alpha", n_boot=500, seed=7)
        check("Bootstrap reproduzierbar mit Seed", (e1.ci_low, e1.ci_high) == (e2.ci_low, e2.ci_high),
              f"[{e1.ci_low:.4f}, {e1.ci_high:.4f}]")
        check("Bootstrap-CI enthält Punktschätzer", e1.ci_low <= e1.value <= e1.ci_high,
              f"alpha {e1.value:.4f} in [{e1.ci_low:.4f}, {e1.ci_high:.4f}]")

        n, p = sections["PCT"][0].split("|")
        check("Prozent-Agreement", n == "5" and abs(float(p) - 0.4) < 1e-12, f"n={n}, {float(p):.3f} (erwartet 5, 0.400)")
        n, p = sections["PCT_MAYBE"][0].split("|")
        check("Prozent-Agreement Maybe→Include", n == "5" and abs(float(p) - 0.6) < 1e-12, f"n={n}, {float(p):.3f} (erwartet 5, 0.600)")
    finally:
        subprocess.run(run + [f"{PG}/pg_ctl -D {tmp}/data -m fast stop >/dev/null"])
        shutil.rmtree(tmp, ignore_errors=True)
    print(f"\n{'ALLE TESTS BESTANDEN' if failures == 0 else f'{failures} FEHLER'}")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
