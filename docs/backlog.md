# Backlog

Offene Wünsche, nach Thema geordnet und in jedem Thema nach Aufwand aufsteigend. Der Aufwand ist grob
geschätzt: **klein** (bis etwa ein Tag), **mittel** (einige Tage), **groß** (ein eigener Meilenstein).

## Funktionen

### KI-Unterstützung

| Aufwand | Wunsch | Anmerkung |
| --- | --- | --- |
| mittel | Belegstelle eines KI-Vorschlags im Volltext anspringen und markieren | Der Vorschlag nennt Zitat und Seite; gesucht wird noch von Hand. |
| mittel | KI-Vorschläge zur Extraktion für gescannte PDFs und sehr lange Volltexte | Gelesen wird die Textebene des PDFs, höchstens 400.000 Zeichen. Scans bräuchten Texterkennung oder ein Modell, das Seiten als Bild liest. |
| groß | Decision Model in der App für Screening-Entscheidungen | Noch zu klären: Was soll das Modell leisten (Reihenfolge der Einträge, Vorschlag, Stoppregel)? |
| offen | Decision Model über eine API, z. B. JEV | Noch zu klären: welche API und welches Format. Hängt am Punkt darüber. |
| offen | MCP für die App erstellen, so dass man auch von außen mit einem KI Modell auf die Inhalte der App zugreifen und Daten in der App eintragen kann | Die App datenmäßig für den Zugriff von "außen" öffnen und so bspw. Claude ermöglichen Anpassungen an den Daten und Inhalten vorzunehmen, muss deaktivierbar sein - auf individueller Ebene pro User und auf Projektebene durch den Projektleiter, MCP Konfigurierbar machen, berechtigungen vergeben können, bspw. nur lesen |

### Volltexte

| Aufwand | Wunsch | Anmerkung |
| --- | --- | --- |
| klein | Markierten Text in ein Feld des Formulars übernehmen | Markierungen mit „Zitat kopieren“ gibt es; das Feld füllt sich noch nicht von selbst. |
| mittel | Extraktion aus Volltexten im Tool | Die Eingabe im Formular neben dem Volltext gibt es schon. Noch zu klären: Was fehlt außer dem Punkt darüber? |
| mittel | Flächen markieren (Abbildungen, gescannte Seiten) und Markierungen über den Seitenwechsel | Bisher nur ausgewählter Text auf einer Seite. |

### Export

| Aufwand | Wunsch | Anmerkung |
| --- | --- | --- |
| mittel | Screening und Extraktion als Excel | Je Stufe gibt es CSV und JSON. |
| mittel | Screening und Extraktion für gängige Statistikprogramme | Noch zu klären: welche (R, SPSS, Stata). |

### Protokoll

| Aufwand | Wunsch | Anmerkung |
| --- | --- | --- |
| mittel | Textbausteine für Protokolle | |
| mittel | Aus einer eingelesenen OSF-Registrierung auch Kriterien, Suchen und Formular vorbefüllen | „Aus OSF einlesen“ füllt bisher die 65 Felder der Registrierung. Kriterien und Suchen stehen dort als freier Text; noch zu klären: von Hand zuordnen oder mit KI-Vorschlag. |
| mittel | Protokoll aus einer Datei (PDF, Word) einlesen | Viele Protokolle hängen bei OSF nur als Datei an; die App nennt die Dateien, lädt sie aber nicht. |

### Suche

| Aufwand | Wunsch | Anmerkung |
| --- | --- | --- |
| mittel | Suchergebnisse dokumentieren mit Trefferanzahl je Such- und Substring | auch das finale Ergebnis der gefundenen Einträge sollte man separat ausweisen können |

### UI

| Aufwand | Wunsch | Anmerkung |
| --- | --- | --- |
| offen | Einfaches Interface und Expertenversion | Das Nutzerinterface ist mitlerweile sehr vollgepackt und wird echt unübersichtlich, daher sollten wir auch eine "einfache" Version einbauen, in der unnötige Informationen nicht dargestellt werden. Hinweise sollten sich grundsätzlich hinter Tooltips befinden (mit Indikatorsymbol) |

### Dokumentation

| Aufwand | Wunsch | Anmerkung |
| --- | --- | --- |
| mittel | Deduplizierungen in anderen Anwendungen, bspw. Zotero eintragen können | Aktuell werden nur Dedups aus dem Tool gezählt, aber man kann auch in Zotero Dedups hinzufügen, ggf. als Quelle nennen |

### Rückmeldung

| Aufwand | Wunsch | Anmerkung |
| --- | --- | --- |
| mittel | Issue aus der App heraus anlegen, ohne den Browser | „Fehler melden“ öffnet bisher einen Entwurf im Mailprogramm oder das ausgefüllte Formular auf GitHub. Direktes Anlegen bräuchte eine Anmeldung bei GitHub in der App. |

## Doku und Material

| Aufwand | Wunsch |
| --- | --- |
| klein | Abgrenzung von bestehenden Tools |
| klein | User Stories schreiben |
| klein | User Journey beschreiben |
| mittel | Demo-Projekte auf Englisch anlegen |
| groß | Anleitungsvideos |
| groß | Werbevideo |
