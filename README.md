# open-review-tool

Open-Source-Werkzeug für das Screening in Scoping Reviews (JBI) und Systematic Reviews, als native Mac-App.
Kernideen: ein unveränderliches, signiertes Event-Log für alle Entscheidungen, ein Stufen-Workflow mit Pilot,
Verblindung, Klärung und Snapshots, eine Auswertung der Übereinstimmung mit Konfidenzintervallen und
Zusammenarbeit ohne Server über Austauschdateien.

> Status (19.11.2026): Mac-App mit Rust und Tauri 2. Der vollständige Ablauf eines Scoping Reviews wird nach
> `docs/umsetzungsplan.md` in Meilensteinen gebaut; fertig sind 0 (Fundament), 1 (Vorlage und Ablauf),
> 2 (Kriterien und Screening-Ansicht), 3 (Suche, Import, Dubletten), 4 (Volltext, ohne das Markieren) und
> 5 (Bericht, Teil 1).
> Enthalten: Identität mit Schlüssel im Schlüsselbund, Reviews mit Rollen, Review-Typ als Vorlage mit Stationen,
> Gates und Runden, Kriterien mit Kürzeln und Erläuterungen, Suchen mit Versionen und Peer-Review-Vermerk, Import
> von RIS, BibTeX und PubMed (nbib) mit Suchversion und Herkunft, exakte und unscharfe Dubletten, Stufen mit drei
> Auswahlregeln, verblindetes Screening in drei Spalten mit Filtern, Volltexte aus einem Ordner oder aus Zotero
> mit Prüfsumme und Vorschau in der App, Pflichtgrund im Volltext, Konflikte und Konsens (auch als
> Dritt-Entscheid), Übereinstimmung, CSV-Export, Pakete und Rückmeldungen, Bericht mit Flussdiagramm (SVG),
> ausgeschlossenen Volltexten mit Grund, Eingeschlossenen als RIS, Suchanhang und Methodentext zur Auswahl.
> Noch nicht enthalten: Markieren im Volltext, Rückschreiben nach Zotero, Zitationssuche, Charting, Checkliste
> PRISMA-ScR, signierte und notarisierte App.

## So arbeitet ein Team

Es gibt keinen Server. Jede Person hat die App und eine eigene Kopie des Reviews; Entscheidungen reisen als Dateien
(per Mail, Teams, Cloud-Ordner oder USB-Stick).

1. **Mitmachen:** Jede Person speichert in der App ihre Identitätsdatei (`.ortid`) und schickt sie der Person, die
   das Review leitet.
2. **Anlegen:** Die Leitung legt das Review an, hält die Suchen fest, importiert die Treffer (RIS, BibTeX oder
   PubMed), entscheidet mögliche Dubletten, fügt Mitglieder über deren `.ortid` hinzu und öffnet eine Stufe.
3. **Verteilen:** Für jedes Mitglied speichert die Leitung ein Paket (`.ortpkg`). Bei einer verblindeten Stufe
   enthält das Paket nur die eigenen Stimmen der Person. Was nicht in der Datei steht, kann niemand sehen; die
   Verblindung hängt also nicht an der Oberfläche.
4. **Screenen:** Das Mitglied liest das Paket ein, screent (Tasten `i`, `e`, `m`, Ausschlussgrund mit `1` bis `9`)
   und speichert eine Rückmeldung (`.ortvotes`) mit seinen signierten Entscheidungen.
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
| `crates/ort-core/` | Fachkern ohne Oberfläche: Event-Log (`event.rs`, `store.rs`), Register der Ereignistypen (`catalog.rs`), Projektionen, Vorlagen und Ablauf (`workflow.rs`), Kriterien (`criteria.rs`), Suchen (`search.rs`), Dubletten (`duplicates.rs`), Volltexte (`fulltext.rs`), Bericht (`report.rs`), Übereinstimmung, Import, Fachlogik (`service.rs`), Austausch (`exchange.rs`) |
| `crates/ort-core/templates/` | Mitgelieferte Vorlagen: „JBI Scoping Review“ und „Freier Ablauf“ |
| `crates/ort-core/tests/` | Tests für Log, Projektionen, Referenzwerte, Import, Austausch, Vorlagen und Gates und den Ablauf mit vier Personen |
| `app/src-tauri/` | Tauri-Hülle: Befehle für die Oberfläche, Identität und Schlüsselbund |
| `app/src/` | Oberfläche mit React und TypeScript |
| `docs/` | Anforderungsanalyse mit Stufenmodell, JBI-Anforderungen und Umsetzungsplan |
| `.github/workflows/ci.yml` | Prüft jeden Push: Formatierung, Clippy, Tests, Typprüfung, Build der Mac-App |

Technik: Rust, SQLite (eingebunden über rusqlite), Ed25519, kanonisches JSON nach RFC 8785 für die Hashes,
Tauri 2 mit der WebView von macOS, React 18.

## Wie es geprüft ist

- **Kern:** 117 Tests, dazu 2 für das Lesen der Zotero-Antworten in der App. Darunter die Referenzwerte der früheren SQL-Tests (Cohen's Kappa gegen scikit-learn,
  Fleiss' Kappa am Wikipedia-Beispiel, Krippendorffs Alpha am publizierten Beispiel mit 0,743, PABAK, Prozent,
  sechs `final_decision`-Szenarien, Maybe-Regel). Die Werte erzeugt `crates/ort-core/tests/fixtures/make_agreement_fixtures.py`.
- **Austausch:** veränderte Datei, nachträglich neu gehashte Datei ohne Signatur, Außenstehende, Lücken in einer
  Kette, doppeltes Einlesen, Paket für eine andere Person, untergeschobene Records, selbst vergebene Rollen.
- **Vorlagen und Gates:** Pilot unter 75 % sperrt Titel/Abstract, eine neue Runde oder ein Übersteuern mit
  Begründung öffnet es; `liberal` und `single_verified` gegen Handrechnung; die Prüfstichprobe lässt sich aus Seed
  und ID nachrechnen; Dritt-Entscheid wird auch beim Einlesen erzwungen; derselbe Durchlauf mit beiden Vorlagen.
- **Kriterien und Gründe:** Volltext-Ausschluss ohne Hauptgrund oder mit unbekanntem Kürzel wird in der App und
  beim Einlesen abgewiesen, ebenso ein solcher Konsens; eine Stufe prüft gegen ihre eigene Kriterienversion;
  Filter nach Konflikt, Person und Stimmenzahl sind verblindet auch bei der Leitung gesperrt.
- **Suche und Dubletten:** Ohne Suche und mit offenen Dublettenpaaren ist der Pilot gesperrt; derselbe Artikel
  mit und ohne DOI wird vorgeschlagen, Titel mit anderer Zahl nicht; eine entschiedene Dublette fehlt in neuen
  Stufen, offene Stufen bleiben; ein Import mit unbekannter Suchversion wird beim Einlesen abgewiesen; Treffer je
  Quelle und Dublettenzahl aus dem Log stimmen auf jedem Rechner mit den Tabellen überein.
- **Volltexte:** Die Volltext-Stufe enthält nur beschaffte Einträge, ein Nachzügler läuft allein als zweite
  Runde; die Zahlen stehen an der Station. Eine ersetzte Datei fällt an der Prüfsumme auf, eine fehlende wird
  gemeldet; nichts außerhalb des verknüpften Ordners lässt sich zuordnen, auch nicht über ein eingelesenes
  Ereignis mit `../` im Pfad. Jede Person verknüpft den Ordner an ihrem eigenen Ort; im Log steht kein
  lokaler Pfad. Status darf setzen, wer Schreibrecht hat; Zotero verknüpft nur die Leitung.
- **Bericht:** Jede Ebene des Flussdiagramms wird nachgerechnet (Eingang gleich geprüft plus nicht zugeteilt
  plus nicht beschafft; geprüft gleich eingeschlossen plus ausgeschlossen plus offen; Ausschlüsse gleich Summe
  der Gründe), vor dem Volltext, während der verblindeten Stufe, im Konflikt und nach dem Abschluss, bei der
  Leitung und bei der Person mit Leserecht. Entscheidungen einer verblindeten Stufe stehen in keiner Zahl und
  in keiner Datei. Der RIS-Export wird vom eigenen Import wieder gelesen.
- **Ablauf:** Owner, zwei Reviewer und eine Person mit Leserecht mit getrennten Dateien, nur über Pakete und
  Rückmeldungen: verblindetes Screening, Paket ohne fremde Stimmen, Entblinden, Konflikte, Konsens, Revision,
  CSV, Abschluss mit Snapshot, abgewiesene Nachzügler, Folgestufe, Bericht, Prüfung aller Ketten.
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
- **Dubletten:** Der Import erkennt exakt gleiche Einträge (DOI oder normalisierter Titel plus Jahr). Ähnliche
  Paare schlägt die App vor; jedes entscheidet die Leitung. Die Vorschläge sind berechnet und stehen nicht im
  Log, nur die Entscheidungen. Eine spätere Version mit anderem Abgleich kann deshalb neue Paare zeigen.
- **Suche:** Ein Import ohne Suchversion bleibt möglich. Die Vorlage „JBI Scoping Review“ verlangt nur, dass
  mindestens eine Suche festgehalten ist, nicht dass jeder Import eine nennt.
- **Volltexte:** Die Dateien liegen nie in der App und nie im Paket. Wer einen Volltext lesen will, braucht
  den Ordner (oder die Zotero-Gruppenbibliothek) auf dem eigenen Rechner. Vorschläge für Zuordnungen sind
  berechnet und stehen nicht im Log. Die DOI in einer PDF findet die App nur, wenn die Metadaten
  unkomprimiert in der Datei stehen. Volltexte aus Zotero prüft „Dateien prüfen“ nicht mit; ihre Prüfsumme
  wird beim Öffnen geprüft. Das Markieren im Volltext (zweiter Schritt im Plan) ist noch nicht gebaut.
- **Volltext-Stufe in Runden:** Später beschaffte Volltexte laufen als weitere Runde der Station. Der Bericht
  führt die Runden zusammen (für einen Eintrag gilt die letzte Runde, in der er zugeteilt war). Eine Stufe,
  die aus dem Volltext-Screening schöpft, nimmt dagegen bisher nur die letzte Runde; das wird mit dem Charting
  (Meilenstein 7) gebraucht und dort gelöst.
- **Bericht:** Das Flussdiagramm hat eine Spalte; der Zweig „andere Quellen“ kommt mit der Zitationssuche
  (Meilenstein 6). Eine Ebene ist eine Screening-Station mit allen Runden, im freien Ablauf jede Stufe für
  sich. Stichproben wie der Pilot stehen im Methodentext, nicht im Diagramm. Einträge in einer verblindeten
  Stufe zählen als offen. Der Bericht ist berechnet und steht nicht im Log; der Methodentext ist ein Entwurf
  ohne Namen. Die PMID steht im RIS-Export im Feld `AN`.
- **Rollenprüfung beim Einlesen:** nach dem Stand nach dem Zusammenführen, nicht zum Zeitpunkt jedes Ereignisses.
- **Gates:** Das Ergebnis wird beim Schließen einer Stufe auf dem Rechner der Leitung berechnet und als
  `GateEvaluated` festgehalten. Schließen ist auch mit nicht erfülltem Gate möglich; gesperrt wird die nächste
  Station. Stationen, die die App noch nicht bearbeiten kann (Zitationssuche, Bündeln, Charting), stehen in der
  Leiste als „folgt“ und sperren nichts.
- **Eine Stimme mit Prüfung (`single_verified`):** Die Stufe ist nie verblindet, weil die zweite Person die
  Ausschlüsse sehen muss. Wer was schon bewertet hat, weiß eine Person erst nach dem nächsten Paket.
- **Gründe:** Ausschlussgründe sind die Kürzel der Ausschlusskriterien. Beim Einlesen werden sie nur in Stufen
  mit Pflichtgrund geprüft, damit ältere Logs mit frei getippten Gründen einlesbar bleiben. Nennen zwei Personen
  verschiedene Hauptgründe für denselben Ausschluss, gilt der Eintrag als ausgeschlossen. Im Bericht zählt dann
  der Grund der Klärung, ohne Klärung der Grund, der in den Kriterien zuerst steht.
- **Reihenfolge über Personen hinweg:** nach Zeitstempel; falsch gehende Uhren können die Reihenfolge von Konsens
  oder Rollenwechseln verschieben.

## Offene Entscheidungen

- Signieren und Notarisieren (Apple Developer ID)
- Windows und Linux (mit Tauri möglich)
- Peters et al. 2022 und Pollock et al. 2023 (JBI) auswerten

## Lizenz

MIT, siehe `LICENSE`.
