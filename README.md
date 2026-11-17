# open-review-tool

Arbeitsstand eines Open-Source-Screening-Tools für Scoping Reviews (JBI) und Systematic Reviews.
Kernideen: ein unveränderliches Event-Log für alle Entscheidungen, ein konfigurierbarer Stufen-Workflow
mit Pilot-Gates und Snapshots, Reporting nach PRISMA 2020 / PRISMA-ScR / PRISMA-S und Zotero-Anbindung.

> Status: Konzept und getesteter Datenkern (Phase 0). Noch keine Oberfläche, kein Import, kein Zotero-Sync.

## Inhalt

| Pfad | Inhalt |
| --- | --- |
| `docs/anforderungsanalyse.md` | Rayyan-Funktionsanalyse (Clean-Room, nur öffentliche Quellen), Stufenmodell, JBI-Anforderungen, Reporting-Modul, Lehren aus Argilla, Umsetzungsplan und Backlog, Quellen. Export des Arbeitsdokuments vom 17.11.2026 |
| `docs/img/` | Diagramme: Standard-Workflow und Umsetzungsplan |
| `eventlog/schema.sql` | Event-Log (PostgreSQL 16): Append-only-Tabelle mit Hash-Kette, optimistischer Nebenläufigkeit, Snapshots |
| `eventlog/projections.sql` | Projektionen: `final_decision`, Maybe-Regel aus dem Protokoll, Prozent-Agreement, Cohen's Kappa + PABAK, Fleiss' Kappa, Krippendorffs Alpha |
| `eventlog/agreement.py` | Gleiche Kennzahlen in Python plus Perzentil-Bootstrap-Konfidenzintervalle |
| `eventlog/test_projections.py` | 33 Tests gegen Referenzen (scikit-learn, publizierte Beispiele) |
| `eventlog/coverage_sim.py` | Simulation der Überdeckung der Bootstrap-Intervalle |
| `eventlog/bench.sh` | Lasttest mit 20.000 Records × 2 Reviewer |

## Tests ausführen

Voraussetzungen: PostgreSQL 16 (Binärdateien unter `/usr/lib/postgresql/16/bin`), Python 3 mit numpy und scikit-learn.
Die Tests starten eine temporäre Datenbankinstanz und räumen sie danach wieder ab.

```bash
cd eventlog
python3 test_projections.py   # 33 Tests
python3 coverage_sim.py       # ca. 1 Minute
./bench.sh                    # ca. 2 Minuten
```

## Ergebnisse bisher (17.11.2026)

- Alle 33 Tests bestanden; Krippendorff-Beispiel 0,743 und Fleiss-Beispiel 0,210 reproduziert, Cohen identisch mit scikit-learn.
- Bootstrap-Intervalle: 94–95 % Überdeckung in der Simulation (Prävalenz 10 %, n = 500).
- Lasttest auf 2 vCPU: die meisten Kennzahlen 0,1–0,15 s bei 20.000 Records, Cohen-Paare 0,9–2,6 s.

## Offene Entscheidungen vor Phase 1

- Reihenfolge: Suchdokumentation (Phase 3) vorziehen, falls ein echter Review sie früher braucht?
- Oberfläche: eigene schlanke Web-Oberfläche (Vorschlag) statt Argilla-Fork
- Betrieb: Team-Server (Zotero Web API, Gruppenbibliothek) oder Einzelplatz (lokale Zotero-API)
- Peters et al. 2022 und Pollock et al. 2023 (JBI) auswerten

## Lizenz

MIT, siehe `LICENSE`.
