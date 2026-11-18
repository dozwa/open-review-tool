# open-review-tool

Open-Source-Werkzeug für das Screening in Scoping Reviews (JBI) und Systematic Reviews, als native Mac-App.
Kernideen: ein unveränderliches, signiertes Event-Log für alle Entscheidungen, ein Stufen-Workflow mit Pilot,
Verblindung, Klärung und Snapshots, eine Auswertung der Übereinstimmung mit Konfidenzintervallen und
Zusammenarbeit ohne Server über Austauschdateien.

> Status (18.11.2026): Umbau von der Web-Version (Django, PostgreSQL, Docker) zur Mac-App mit Rust und Tauri 2.
> Enthalten: Identität mit Schlüssel im Schlüsselbund, Reviews mit Rollen, Import von RIS und BibTeX, Stufen,
> verblindetes Screening, Konflikte und Konsens, Übereinstimmung, CSV-Export, Pakete und Rückmeldungen.
> Noch nicht enthalten: Zotero-Anbindung, Volltexte, Suchdokumentation, Reporting, signierte und notarisierte App.

## So arbeitet ein Team

Es gibt keinen Server. Jede Person hat die App und eine eigene Kopie des Reviews; Entscheidungen reisen als Dateien
(per Mail, Teams, Cloud-Ordner oder USB-Stick).

1. **Mitmachen:** Jede Person speichert in der App ihre Identitätsdatei (`.ortid`) und schickt sie der Person, die
   das Review leitet.
2. **Anlegen:** Die Leitung legt das Review an, importiert RIS oder BibTeX, fügt Mitglieder über deren `.ortid` hinzu
   und öffnet eine Stufe.
3. **Verteilen:** Für jedes Mitglied speichert die Leitung ein Paket (`.ortpkg`). Bei einer verblindeten Stufe
   enthält das Paket nur die eigenen Stimmen der Person. Was nicht in der Datei steht, kann niemand sehen; die
   Verblindung hängt also nicht an der Oberfläche.
4. **Screenen:** Das Mitglied liest das Paket ein, screent (Tasten `i`, `e`, `m`) und speichert eine Rückmeldung
   (`.ortvotes`) mit seinen signierten Entscheidungen.
5. **Zusammenführen:** Die Leitung liest die Rückmeldungen ein, entblindet, klärt Konflikte, schließt die Stufe mit
   Snapshot und verteilt neue Pakete.

Beim Einlesen prüft die App jede Datei: Prüfsumme und Ed25519-Signatur jedes Ereignisses, lückenlose Ketten, die
Rolle der Person, ob die Stufe noch offen ist, und Prüfsummen über Records und Zuweisungen. Eine veränderte Datei
wird ganz abgewiesen; fachlich unzulässige Einträge, z. B. Stimmen nach dem Abschluss, einzeln mit Grund.

**Grenze:** Wer das Review leitet, hat alle Daten auf dem eigenen Rechner. Screent diese Person selbst mit, blendet
nur die Oberfläche fremde Stimmen aus, solange die Stufe verblindet ist.

## Installieren

Bis es signierte Versionen gibt: die App aus dem CI-Artefakt `open-review-tool-macos` laden oder selbst bauen
(siehe unten). macOS blockiert eine heruntergeladene, unsignierte App beim ersten Öffnen; freigeben unter
Systemeinstellungen → Datenschutz & Sicherheit → „Dennoch öffnen“. Eine selbst gebaute App startet ohne diesen Schritt.

Daten liegen unter `~/Library/Application Support/org.openreviewtool.desktop/`: `identity.json` (Name, öffentlicher
Schlüssel) und `reviews/<id>.ortreview` (eine SQLite-Datei je Review). Der private Schlüssel liegt im
macOS-Schlüsselbund unter „open-review-tool“. Zur Sicherung genügt eine Kopie dieses Ordners; für den Schlüssel
die Schlüsselbund-Sicherung von macOS.

## Aufbau

| Pfad | Inhalt |
| --- | --- |
| `crates/ort-core/` | Fachkern ohne Oberfläche: Event-Log (`event.rs`, `store.rs`), Projektionen, Übereinstimmung, Import, Fachlogik (`service.rs`), Austausch (`exchange.rs`) |
| `crates/ort-core/tests/` | Tests für Log, Projektionen, Referenzwerte, Import, Austausch und den Ablauf mit vier Personen |
| `app/src-tauri/` | Tauri-Hülle: Befehle für die Oberfläche, Identität und Schlüsselbund |
| `app/src/` | Oberfläche mit React und TypeScript |
| `docs/` | Anforderungsanalyse mit Stufenmodell, JBI-Anforderungen und Umsetzungsplan |
| `.github/workflows/ci.yml` | Prüft jeden Push: Formatierung, Clippy, Tests, Typprüfung, Build der Mac-App |

Technik: Rust, SQLite (eingebunden über rusqlite), Ed25519, kanonisches JSON nach RFC 8785 für die Hashes,
Tauri 2 mit der WebView von macOS, React 18.

## Wie es geprüft ist

- **Kern:** 47 Tests. Darunter die Referenzwerte der früheren SQL-Tests (Cohen's Kappa gegen scikit-learn,
  Fleiss' Kappa am Wikipedia-Beispiel, Krippendorffs Alpha am publizierten Beispiel mit 0,743, PABAK, Prozent,
  sechs `final_decision`-Szenarien, Maybe-Regel). Die Werte erzeugt `crates/ort-core/tests/fixtures/make_agreement_fixtures.py`.
- **Austausch:** veränderte Datei, nachträglich neu gehashte Datei ohne Signatur, Außenstehende, Lücken in einer
  Kette, doppeltes Einlesen, Paket für eine andere Person, untergeschobene Records, selbst vergebene Rollen.
- **Ablauf:** Owner, zwei Reviewer und eine Person mit Leserecht mit getrennten Dateien, nur über Pakete und
  Rückmeldungen: verblindetes Screening, Paket ohne fremde Stimmen, Entblinden, Konflikte, Konsens, Revision,
  CSV, Abschluss mit Snapshot, abgewiesene Nachzügler, Folgestufe, Prüfung aller Ketten.
- **App:** Typprüfung, Clippy und unsignierter Build auf macOS in der CI.

Nicht automatisch geprüft: die Oberfläche selbst (Tauri bietet auf macOS keinen WebDriver), große Importe
(20.000 Einträge) und der Schlüsselbund in der CI.

## Lokal entwickeln

Voraussetzungen: Xcode Command Line Tools, Rust (z. B. `brew install rustup && rustup default stable`), Node 22.

```bash
cargo test -p ort-core                       # alle Kerntests
cargo test -p ort-core --test exchange       # eine Testdatei
cargo test -p ort-core gap_in_chain          # ein einzelner Test

cd app
npm install
npm run tauri dev                            # App mit Neuladen bei Änderungen
npm run tauri build -- --bundles app         # target/release/bundle/macos/Open Review Tool.app
```

Eine zweite Person auf demselben Mac, z. B. zum Ausprobieren des Austauschs:

```bash
ORT_DATA_DIR=/tmp/ort-zweite ORT_KEY_STORE=file npm run tauri dev
```

`ORT_KEY_STORE=file` legt den privaten Schlüssel als `identity.key` ins Datenverzeichnis statt in den Schlüsselbund.
Nur für Entwicklung und Tests.

## Abweichungen vom Umsetzungsplan

- **Betrieb:** Mac-App mit Austauschdateien statt Team-Server (Entscheidung vom 18.11.2026, siehe Anforderungsanalyse).
- **Event-Log:** je Person eine Kette für Verwaltung und je Stufe eine für Stimmen, statt einer Kette je Review.
  Grund: Pakete lassen verblindete Stimmen als ganze Ketten weg, ohne Lücken zu reißen.
- **Dubletten:** exakter Abgleich über DOI oder normalisierten Titel plus Jahr. Ein unscharfer Abgleich fehlt noch.
- **Rollenprüfung beim Einlesen:** nach dem Stand nach dem Zusammenführen, nicht zum Zeitpunkt jedes Ereignisses.
- **Reihenfolge über Personen hinweg:** nach Zeitstempel; falsch gehende Uhren können die Reihenfolge von Konsens
  oder Rollenwechseln verschieben.

## Offene Entscheidungen

- Signieren und Notarisieren (Apple Developer ID)
- Windows und Linux (mit Tauri möglich)
- Reihenfolge der nächsten Pakete: Zotero (lokale API), Suchdokumentation oder Reporting zuerst
- Peters et al. 2022 und Pollock et al. 2023 (JBI) auswerten

## Lizenz

MIT, siehe `LICENSE`.
