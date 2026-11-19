# open-review-tool

Open-Source-Werkzeug für das Screening in Scoping Reviews (JBI) und Systematic Reviews, als native Mac-App.
Kernideen: ein unveränderliches, signiertes Event-Log für alle Entscheidungen, ein Stufen-Workflow mit Pilot,
Verblindung, Klärung und Snapshots, eine Auswertung der Übereinstimmung mit Konfidenzintervallen und
Zusammenarbeit ohne Server über Austauschdateien.

> Status (19.11.2026): Mac-App mit Rust und Tauri 2. Der vollständige Ablauf eines Scoping Reviews wird nach
> `docs/umsetzungsplan.md` in Meilensteinen gebaut; fertig sind 0 (Fundament), 1 (Vorlage und Ablauf),
> 2 (Kriterien und Screening-Ansicht), 3 (Suche, Import, Dubletten), 4 (Volltext, ohne das Markieren),
> 5 (Bericht, Teil 1), 6 (Zitationssuche, Bündeln, Quellenart), 7 (Formulare und Charting), 8 (Bericht,
> Teil 2 und Update, ohne das Rückschreiben nach Zotero) und 9 (Vorlagen „Rapid Review“ und
> „State-of-the-Art-Review“).
> Enthalten: Identität mit Schlüssel im Schlüsselbund, Reviews mit Rollen, Review-Typ als Vorlage mit Stationen,
> Gates und Runden, Kriterien mit Kürzeln und Erläuterungen, Suchen mit Versionen und Peer-Review-Vermerk, Import
> von RIS, BibTeX und PubMed (nbib) mit Suchversion und Herkunft, exakte und unscharfe Dubletten, Stufen mit drei
> Auswahlregeln, verblindetes Screening in drei Spalten mit Filtern, Volltexte aus einem Ordner oder aus Zotero
> mit Prüfsumme und Vorschau in der App, Pflichtgrund im Volltext, Konflikte und Konsens (auch als
> Dritt-Entscheid), Übereinstimmung, CSV-Export, Pakete und Rückmeldungen, Bericht mit Flussdiagramm (SVG),
> ausgeschlossenen Volltexten mit Grund, Eingeschlossenen als RIS, Suchanhang und Methodentext zur Auswahl,
> Zitationssuche als weitere Runde mit eigenem Zweig im Diagramm, Bündeln von Einträgen zu Quellen mit Quellenart,
> Formulare mit Versionen, verblindetes Charting doppelt oder einfach mit Prüfung, Abgleich je Feld, Tabelle und
> Häufigkeiten als CSV, Registrierung des Protokolls mit Liste der Abweichungen, Checkliste PRISMA-ScR mit Stand
> je Punkt, Methodentext zum Charting, Tabelle der eingeschlossenen Quellen, Update als weitere Runden,
> Leitlinien zum Nachschlagen mit „?“ auf jedem Bildschirm, geführte Vorbereitung der Registrierung nach dem
> OSF-Formular, Deutsch und Englisch für Oberfläche, Meldungen, Vorlagen, Leitlinien und Bericht.
> Noch nicht enthalten: Markieren im Volltext, Rückschreiben nach Zotero, signierte und notarisierte App.

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
| `crates/ort-core/` | Fachkern ohne Oberfläche: Event-Log (`event.rs`, `store.rs`), Register der Ereignistypen (`catalog.rs`), Projektionen, Vorlagen und Ablauf (`workflow.rs`), Kriterien (`criteria.rs`), Suchen (`search.rs`), Dubletten (`duplicates.rs`), Volltexte (`fulltext.rs`), Bericht (`report.rs`), Sprachen (`i18n.rs`, Kataloge in `i18n/`), Leitlinien (`guidance.rs`), Vorbereitung der Registrierung (`registration.rs`), Bündel und Quellenart (`sources.rs`), Übereinstimmung, Import, Fachlogik (`service.rs`), Austausch (`exchange.rs`) |
| `crates/ort-core/guidance/`, `crates/ort-core/registration/` | Leitlinien je Vorlage als Zusammenfassung mit Verweis auf das Original; Felder des Registrierungsformulars von OSF |
| `crates/ort-core/templates/` | Mitgelieferte Vorlagen: „JBI Scoping Review“, „Rapid Review“, „State-of-the-Art-Review“ und „Freier Ablauf“ |
| `crates/ort-core/tests/` | Tests für Log, Projektionen, Referenzwerte, Import, Austausch, Vorlagen und Gates und den Ablauf mit vier Personen |
| `app/src-tauri/` | Tauri-Hülle: Befehle für die Oberfläche, Identität und Schlüsselbund |
| `app/src/` | Oberfläche mit React und TypeScript |
| `docs/` | Anforderungsanalyse mit Stufenmodell, JBI-Anforderungen und Umsetzungsplan |
| `.github/workflows/ci.yml` | Prüft jeden Push: Formatierung, Clippy, Tests, Typprüfung, Build der Mac-App |

Technik: Rust, SQLite (eingebunden über rusqlite), Ed25519, kanonisches JSON nach RFC 8785 für die Hashes,
Tauri 2 mit der WebView von macOS, React 18.

## Wie es geprüft ist

- **Kern:** 152 Tests, dazu 2 für das Lesen der Zotero-Antworten in der App. Darunter die Referenzwerte der früheren SQL-Tests (Cohen's Kappa gegen scikit-learn,
  Fleiss' Kappa am Wikipedia-Beispiel, Krippendorffs Alpha am publizierten Beispiel mit 0,743, PABAK, Prozent,
  sechs `final_decision`-Szenarien, Maybe-Regel). Die Werte erzeugt `crates/ort-core/tests/fixtures/make_agreement_fixtures.py`.
- **Austausch:** veränderte Datei, nachträglich neu gehashte Datei ohne Signatur, Außenstehende, Lücken in einer
  Kette, doppeltes Einlesen, Paket für eine andere Person, untergeschobene Records, selbst vergebene Rollen.
- **Vorlagen und Gates:** Pilot unter 75 % sperrt Titel/Abstract, eine neue Runde oder ein Übersteuern mit
  Begründung öffnet es; `liberal` und `single_verified` gegen Handrechnung; die Prüfstichprobe lässt sich aus Seed
  und ID nachrechnen; Dritt-Entscheid wird auch beim Einlesen erzwungen; derselbe Durchlauf mit allen vier Vorlagen, ohne dass im Test ein Schlüssel einer Station steht.
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
- **Zitationssuche und Bündel:** Nach einem Import mit der Herkunft „Zitationssuche“ bekommt die zweite Runde
  von Titel/Abstract und Volltext nur die neuen Einträge; eine dritte Runde ohne Neues wird abgewiesen. Der
  Zweig „andere Quellen“ steht im Diagramm, und es geht weiter auf. Zwei gebündelte Einträge zählen in
  Bericht, Diagramm und Quellen-CSV als eine Quelle; aufgelöst zählen sie wieder einzeln. Bündeln und
  Quellenart darf nur die Leitung, über die App und an ihr vorbei.
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
- **Runden:** In einer Vorlage mit festen Runden legt eine Station nichts doppelt vor: Eine weitere Runde
  bekommt nur Einträge, die in keiner früheren waren (später beschaffte Volltexte, Treffer der Zitationssuche).
  Eine Runde über dieselben Einträge zu wiederholen geht damit nicht; der Pilot zieht als Stichprobe jedes Mal
  neu. Eine Station, die aus einer anderen schöpft, nimmt die Eingeschlossenen aller Runden; für einen Eintrag
  gilt die letzte Runde, in der er entschieden ist. Im freien Ablauf bleibt alles wie bisher.
- **Zitationssuche:** Sie wird wie jede Suche festgehalten (Herkunft „Zitationssuche“) und ihr Import läuft
  durch dieselben Stationen. Das Gate der Station verlangt eine solche Suche oder einen solchen Import und
  dass kein Eintrag ohne Runde ist. Wer keine Zitationssuche macht, übersteuert das Gate mit Begründung; das
  ist die dokumentierte Grenze. Ob eine weitere Suche noch Neues bringt, entscheidet das Team.
- **Andere Quellen im Diagramm:** PRISMA 2020 führt andere Wege als eigene Spalte bis zum Einschluss. Hier
  laufen diese Einträge durch dieselben Stufen wie alle anderen; das Diagramm zeigt sie deshalb als eigenen
  Kasten, der in den Fluss mündet, und nennt in jeder Ebene, wie viele der Geprüften daher stammen. Einträge
  ohne Angabe der Herkunft zählen zu Datenbanken und Registern.
- **Bündel und Quellenart:** Ein Bündel hat einen Haupteintrag; seine Quellenart gilt für die Quelle. Ein
  Eintrag gehört zu höchstens einem Bündel: Die App weist einen zweiten Versuch ab, beim Zusammenführen
  gewinnt das spätere Bündel, und ein Bündel mit weniger als zwei Einträgen ist aufgelöst. Gebündelt werden
  kann auch ein nicht eingeschlossener Eintrag; er zählt nicht mit. Die Quellenarten kommen aus der Vorlage,
  im freien Ablauf sind sie freier Text. Vorschläge für Bündel gibt es nicht; die Liste ist nach Erstautor
  sortiert. Der RIS-Export enthält weiter alle eingeschlossenen Einträge, die Quellen-CSV die Zuordnung.
  Reviews, die vor diesem Stand angelegt wurden, haben die Gates für Zitationssuche und Bündeln nicht, weil
  ihre Vorlage im Log steht.
- **Bericht:** Eine Ebene ist eine Screening-Station mit allen Runden, im freien Ablauf jede Stufe für
  sich. Stichproben wie der Pilot stehen im Methodentext, nicht im Diagramm. Einträge in einer verblindeten
  Stufe zählen als offen. Der Bericht ist berechnet und steht nicht im Log; der Methodentext ist ein Entwurf
  ohne Namen. Die PMID steht im RIS-Export im Feld `AN`.
- **Rollenprüfung beim Einlesen:** nach dem Stand nach dem Zusammenführen, nicht zum Zeitpunkt jedes Ereignisses.
- **Gates:** Das Ergebnis wird beim Schließen einer Stufe auf dem Rechner der Leitung berechnet und als
  `GateEvaluated` festgehalten. Schließen ist auch mit nicht erfülltem Gate möglich; gesperrt wird die nächste
  Station.
- **Protokoll und Abweichungen:** Die Liste ist eine Projektion. Als Abweichung zählt jede neue Version von
  Kriterien, Suchen und Ablauf seit dem ersten Eintrag der Registrierung im Review (nicht seit dem Datum der
  Registrierung), dazu jede Formularänderung nach der Bestätigung und jedes übersteuerte Gate. Ohne Registrierung
  fehlen die ersten drei. Eine wiederholte Suche (Update) steht ebenfalls in der Liste, weil sie eine neue
  Suchversion ist. Geänderte Regeln einer einzelnen Stufe stehen nicht darin.
- **Checkliste:** Die Punkte der PRISMA-ScR stehen in eigenen Worten in der App. Der Stand je Punkt sagt nur, ob
  das Review Material dafür hergibt; ob der Bericht den Punkt erfüllt, prüft das Team. Ein eigener Haken je Punkt
  wird nicht gespeichert. Die für 2026 angekündigte neue Fassung ist nicht eingearbeitet.
- **Update:** Es gibt keinen eigenen Knopf. Ein Update ist eine neue Suchversion, ein Import dazu und je Station
  eine weitere Runde, die nur das Neue bekommt. Die Charting-Station der JBI-Vorlage gilt erst wieder als fertig,
  wenn jede eingeschlossene Quelle in einer Runde liegt. Das Flussdiagramm zählt Update und erste Suche zusammen.
- **Charting:** Jeder Wert ist ein eigenes Ereignis, geschrieben beim Verlassen des Feldes. Eine Formular-Stufe
  arbeitet immer mit der neuesten Formularversion; ein Feld behält seinen Typ über alle Versionen. Beim Einlesen
  wird geprüft, ob ein Wert zum Feld passt, nicht ob beim Abschluss alle Pflichtfelder gefüllt waren; fehlende
  Pflichtfelder hält das Gate der Station auf. Wiederholbare Gruppen (Arm, Zeitpunkt) gibt es noch nicht. Tabelle
  und Häufigkeiten gelten je Stufe, nicht über alle Runden einer Station. Reviews, die vor diesem Stand mit der
  JBI-Vorlage angelegt wurden, haben für die Charting-Stationen kein Gate im Log; das Charting geht dort trotzdem.
- **Sprachen:** Die App spricht Deutsch und Englisch (britisch). Die Sprache ist eine Einstellung der Person
  (Auswahl in den Einstellungen, sonst die Sprache des Systems) und steht nie im Log. Übersetzt wird, was
  die App selbst sagt: Oberfläche, Fehlermeldungen, Texte der mitgelieferten Vorlagen, Leitlinien,
  Registrierungsformular, Bericht. Was Menschen schreiben (Kriterien, Suchstrings, Notizen, Formularfelder, Namen
  von Stufen), bleibt, wie es geschrieben wurde. Die mitgelieferten Vorlagen stehen immer in der deutschen Fassung
  im Log und werden erst bei der Anzeige übersetzt; zwei Personen können dasselbe Review in verschiedenen Sprachen
  öffnen. Bericht und Vorbereitung der Registrierung haben eine eigene Ausgabesprache, damit ein Team deutsch
  arbeiten und englisch berichten kann. Der deutsche Text im Quelltext ist der Schlüssel; eine weitere Sprache
  braucht `crates/ort-core/i18n/<code>.json`, `app/src/i18n/<code>.ts`, je einen Ordner unter `guidance/` und
  `registration/` und je einen Eintrag in den Sprachlisten (`i18n::LANGUAGES`, `LANGUAGES` in
  `app/src/i18n/index.ts`). Fehlt ein englischer Text, schlägt im Kern ein Test fehl und in der Oberfläche die
  Typprüfung. Nicht übersetzt sind Spaltenköpfe der CSV-Dateien (englische Schlüssel) und der Projekttext selbst
  (README, Doku, Kommentare bleiben deutsch). Eine Meldung zu einem fehlgeschlagenen Import steht in der Sprache
  in der Tabelle, die beim Import galt.
- **Einstellungen:** Zwei Ebenen, beide nur auf dem eigenen Rechner und nie im Log oder in einem Paket. Das
  Programm (Zahnrad unten in der Seitenleiste oder ⌘,) hält Sprache, Erscheinungsbild, Akzentfarbe,
  Entscheidungsfarben (auch eine Palette für Farbfehlsichtigkeit), Schriftgröße, Dichte, den Aufbau der
  Screening-Seite und die Ordner, in denen Dialoge für Exporte und Pakete beginnen; es liegt als `settings.json`
  im Datenordner der App. Ein Review (Bereich „Einstellungen“) hat eine Kennfarbe und kann Ausgabesprache,
  Ordner und Screening-Aufbau überschreiben; das liegt in der Review-Datei. Wo die Reviews liegen, wird gezeigt,
  lässt sich aber nicht ändern.
- **Fokusmodus:** Beim Screening und bei der Eingabe im Formular blendet „Fokus“ (Taste F) Seitenleiste, Liste
  und Nebensächliches aus und schaltet das Fenster in den Vollbildmodus; Esc beendet ihn.
- **Leitlinien:** Zu jeder Vorlage außer dem freien Ablauf liegt eine Zusammenfassung ihrer Leitlinie in der App
  (`crates/ort-core/guidance/<sprache>/`), in eigenen Worten, mit Quelle, Abrufdatum und Adresse je Abschnitt. Es ist keine
  wörtliche Kopie: Die Lizenzen der Originale erlauben das nicht durchgehend, und das JBI Manual ändert sich
  laufend. Maßgeblich bleibt das Original; der Verweis braucht Netz, die Zusammenfassung nicht. Das „?“ oben
  rechts zeigt die Einträge zur offenen Station oder zum offenen Bereich. Leitlinien für Systematic, Umbrella und
  qualitative Reviews kommen mit ihren Vorlagen (Meilensteine 10 und 11), weil sie vorher zu lesen sind.
- **Registrierung vorbereiten:** Die App führt durch die 65 Felder des Formulars „Generalized Systematic Review
  Registration“ von OSF (van den Akker et al. 2023) und schlägt vor, was das Review schon weiß: Titel, Team,
  Stationen und Regeln, Kriterien, Suchen, Formularfelder. Vorschläge sind eine Projektion; eigene Texte der
  Leitung stehen als `ProtocolDrafted` im Log, je Feld gilt der letzte. Die App reicht nichts ein und spricht
  nicht mit OSF: Die Texte werden kopiert oder als Markdown gespeichert. Feldnamen, Hinweise und Vorschläge erscheinen in der
  gewählten Ausgabesprache; eigene Texte bleiben, wie sie geschrieben wurden. Namen der Mitwirkenden kommen aus der lokalen Personentabelle und
  stehen nur dann im Log, wenn die Leitung sie in ein Feld schreibt.
- **Rapid Review:** Die Vorlage folgt den Empfehlungen der Cochrane Rapid Reviews Methods Group (Garritty et al.,
  BMJ 2024) mit den vorhandenen Bausteinen. Die doppelt bewertete Stichprobe ist die Pilot-Station (Vorgabe 50
  Einträge, beim Öffnen änderbar; die Leitlinie nennt etwa 20 %). Gesperrt wird über 80 % Übereinstimmung; die
  Leitlinie nennt als Beispiel Kappa ab 0,8, Kappa steht daneben. Danach gilt „eine Stimme mit Prüfung“, also
  strenger als reine Einzelbewertung: Eine zweite Person prüft die Ausschlüsse, alle oder eine Stichprobe. Einen
  eigenen Piloten am Volltext gibt es nicht. Das Verzerrungsrisiko steht als Abschnitt im Extraktionsformular
  (Instrument, Gesamturteil, Begründung); eine eigene Bewertungsstation mit Urteilen je Domäne kommt mit dem
  Systematic Review. Eine Checkliste hat die Vorlage nicht, weil PRISMA 2020 noch nicht eingebaut ist. Die Prüfung
  „mindestens zwei Datenbanken“ und der Peer-Review der Suche sind Hinweise in der Station, kein Gate.
- **State-of-the-Art-Review:** Angelehnt an die sechs Schritte von Barry, Merkebu und Varpio (2022). Einen
  verbindlichen Standard gibt es nicht; die Vorlage gibt nur die Reihenfolge vor, sperrt nichts und verlangt eine
  Stimme je Eintrag. Die Begründung des Zeitraums und die Reflexion der eigenen Perspektive schreibt das Team
  selbst; der Methodentext nennt die Einzelbewertung als Limitation.
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
