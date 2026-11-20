// Beispielbericht für die Browser-Vorschau. Das Diagramm stammt aus dem Kern (`report::diagram_svg`).
// Begriffe und Namen der Ebenen folgen der Sprache; Diagramm und Texte sind ein deutscher Abzug.

import { core } from "./core";

export const mockReport = {
  "flow": {
    "complete": false,
    "exact_duplicates": 58,
    "external_duplicates": 0,
    "external": [],
    "identified": 442,
    "included": 5,
    "included_sources": 5,
    "source_types": [],
    "other_identified": 0,
    "levels": [
      {
        "assessed": 380,
        "blinded": 0,
        "excluded": 366,
        "fulltext": false,
        "included": 14,
        "input": 380,
        "input_from": null,
        "key": "title_abstract",
        "label": core("Titel/Abstract"),
        "not_assigned": 0,
        "open": 0,
        "reasons": [
          {
            "code": "",
            "n": 366,
            "text": core("ohne Hauptgrund")
          }
        ],
        "retrieval": null,
        "other_assessed": 0,
        "other_included": 0,
        "rounds": 1
      },
      {
        "assessed": 12,
        "blinded": 0,
        "excluded": 4,
        "fulltext": true,
        "included": 5,
        "input": 14,
        "input_from": "title_abstract",
        "key": "fulltext",
        "label": core("Volltext-Screening"),
        "not_assigned": 0,
        "open": 3,
        "reasons": [
          {
            "code": "E1",
            "n": 1,
            "text": "Keine Erwachsenen mit Migräne oder Kopfschmerz"
          },
          {
            "code": "E2",
            "n": 2,
            "text": "Keine digitale Intervention"
          },
          {
            "code": "E4",
            "n": 1,
            "text": "Kein Volltext in Deutsch oder Englisch"
          }
        ],
        "retrieval": {
          "open": 0,
          "sought": 14,
          "unavailable": 1,
          "waiting": 1
        },
        "other_assessed": 0,
        "other_included": 0,
        "rounds": 1
      }
    ],
    "record_term": core("Eintrag"),
    "records_term": core("Einträge"),
    "resolved_duplicates": 4,
    "source_term": core("Quelle"),
    "sources": [
      {
        "n_duplicates": 0,
        "n_imports": 1,
        "n_records": 212,
        "origin": "database",
        "search_id": null,
        "search_version": null,
        "source": "MEDLINE"
      },
      {
        "n_duplicates": 0,
        "n_imports": 1,
        "n_records": 187,
        "origin": "database",
        "search_id": null,
        "search_version": null,
        "source": "Embase"
      },
      {
        "n_duplicates": 0,
        "n_imports": 1,
        "n_records": 43,
        "origin": "database",
        "search_id": null,
        "search_version": null,
        "source": "CINAHL"
      }
    ],
    "sources_term": core("Quellen"),
    "unique": 380
  },
  "problems": [],
  "svg": "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 714 514\" width=\"714\" height=\"514\" font-family=\"Helvetica, Arial, sans-serif\" font-size=\"13\" fill=\"currentColor\" role=\"img\" aria-label=\"Flussdiagramm der Auswahl\">\n<defs><marker id=\"pfeil\" markerWidth=\"8\" markerHeight=\"8\" refX=\"7\" refY=\"4\" orient=\"auto\"><path d=\"M0,0 L8,4 L0,8 z\" fill=\"currentColor\"/></marker></defs>\n<rect x=\"12\" y=\"12\" width=\"330\" height=\"92\" rx=\"4\" fill=\"none\" stroke=\"currentColor\"/>\n<text x=\"22\" y=\"35\" font-weight=\"bold\">Identifiziert: 442 Einträge</text>\n<text x=\"22\" y=\"53\">MEDLINE: 212</text>\n<text x=\"22\" y=\"71\">Embase: 187</text>\n<text x=\"22\" y=\"89\">CINAHL: 43</text>\n<rect x=\"392\" y=\"21\" width=\"310\" height=\"74\" rx=\"4\" fill=\"none\" stroke=\"currentColor\"/>\n<text x=\"402\" y=\"44\" font-weight=\"bold\">Vor der Auswahl entfernt: 62</text>\n<text x=\"402\" y=\"62\">Dubletten, beim Import erkannt: 58</text>\n<text x=\"402\" y=\"80\">Dubletten, einzeln entschieden: 4</text>\n<line x1=\"342\" y1=\"58\" x2=\"390\" y2=\"58\" stroke=\"currentColor\" marker-end=\"url(#pfeil)\"/>\n<line x1=\"177\" y1=\"104\" x2=\"177\" y2=\"132\" stroke=\"currentColor\" marker-end=\"url(#pfeil)\"/>\n<rect x=\"12\" y=\"134\" width=\"330\" height=\"38\" rx=\"4\" fill=\"none\" stroke=\"currentColor\"/>\n<text x=\"22\" y=\"157\" font-weight=\"bold\">Titel/Abstract: 380 geprüft</text>\n<rect x=\"392\" y=\"134\" width=\"310\" height=\"38\" rx=\"4\" fill=\"none\" stroke=\"currentColor\"/>\n<text x=\"402\" y=\"157\" font-weight=\"bold\">Ausgeschlossen: 366</text>\n<line x1=\"342\" y1=\"153\" x2=\"390\" y2=\"153\" stroke=\"currentColor\" marker-end=\"url(#pfeil)\"/>\n<line x1=\"177\" y1=\"172\" x2=\"177\" y2=\"209\" stroke=\"currentColor\" marker-end=\"url(#pfeil)\"/>\n<rect x=\"12\" y=\"211\" width=\"330\" height=\"38\" rx=\"4\" fill=\"none\" stroke=\"currentColor\"/>\n<text x=\"22\" y=\"234\" font-weight=\"bold\">Volltext gesucht: 14</text>\n<rect x=\"392\" y=\"202\" width=\"310\" height=\"56\" rx=\"4\" fill=\"none\" stroke=\"currentColor\"/>\n<text x=\"402\" y=\"225\" font-weight=\"bold\">Nicht beschafft: 1</text>\n<text x=\"402\" y=\"243\">nicht beschaffbar: 1</text>\n<line x1=\"342\" y1=\"230\" x2=\"390\" y2=\"230\" stroke=\"currentColor\" marker-end=\"url(#pfeil)\"/>\n<line x1=\"177\" y1=\"249\" x2=\"177\" y2=\"313\" stroke=\"currentColor\" marker-end=\"url(#pfeil)\"/>\n<rect x=\"12\" y=\"315\" width=\"330\" height=\"74\" rx=\"4\" fill=\"none\" stroke=\"currentColor\"/>\n<text x=\"22\" y=\"338\" font-weight=\"bold\">Volltext-Screening: 12 geprüft</text>\n<text x=\"22\" y=\"356\">beschafft, noch nicht zugeteilt: 1</text>\n<text x=\"22\" y=\"374\">noch offen: 3</text>\n<rect x=\"392\" y=\"288\" width=\"310\" height=\"128\" rx=\"4\" fill=\"none\" stroke=\"currentColor\"/>\n<text x=\"402\" y=\"311\" font-weight=\"bold\">Ausgeschlossen: 4</text>\n<text x=\"402\" y=\"329\">E1 Keine Erwachsenen mit Migräne oder</text>\n<text x=\"414\" y=\"347\">Kopfschmerz: 1</text>\n<text x=\"402\" y=\"365\">E2 Keine digitale Intervention: 2</text>\n<text x=\"402\" y=\"383\">E4 Kein Volltext in Deutsch oder</text>\n<text x=\"414\" y=\"401\">Englisch: 1</text>\n<line x1=\"342\" y1=\"352\" x2=\"390\" y2=\"352\" stroke=\"currentColor\" marker-end=\"url(#pfeil)\"/>\n<line x1=\"177\" y1=\"389\" x2=\"177\" y2=\"444\" stroke=\"currentColor\" marker-end=\"url(#pfeil)\"/>\n<rect x=\"12\" y=\"446\" width=\"330\" height=\"56\" rx=\"4\" fill=\"none\" stroke=\"currentColor\"/>\n<text x=\"22\" y=\"469\" font-weight=\"bold\">Eingeschlossen: 5 Quellen</text>\n<text x=\"22\" y=\"487\">vorläufig, die Auswahl läuft noch</text>\n</svg>\n",
  "methods": "# Methoden: Auswahl der Quellen\n\nDer Ablauf folgte der Vorlage „JBI Scoping Review“ (Version 1). Stand: Die Auswahl ist noch nicht abgeschlossen; die Zahlen sind vorläufig.\n\n## Suche und Dubletten\n\nAus 3 Quellen wurden 442 Einträge übernommen (MEDLINE, Embase, CINAHL). Beim Import wurden 58 als exakte Dubletten erkannt (gleiche DOI, sonst gleicher Titel und gleiches Jahr). Mögliche weitere Dubletten wurden paarweise geprüft; 4 wurden entfernt. Es blieben 380 Einträge. Suchstrings, Datum und Grenzen je Quelle stehen im Suchanhang.\n\n## Pilot Titel/Abstract\n\nVor der Auswahl bewertete das Team in 2 Runden eine Zufallsstichprobe. Für den Übergang waren mindestens 75 % Übereinstimmung verlangt. Jeder Eintrag wurde von mindestens 2 Personen unabhängig bewertet; die Stimmen der anderen waren bis zum Entblinden nicht sichtbar. Entschieden war ein Eintrag bei übereinstimmenden Stimmen. „Vielleicht“ zählte als Einschluss. Abweichungen wurden im Gespräch geklärt. Es galten die Kriterien in Version 2.\n\nRunde 1: 25 Einträge, 3 Personen beteiligt. Übereinstimmung vor der Klärung: 68,0 % (17 von 25 mehrfach bewerteten Einträgen; 95-%-Intervall 48,0 bis 84,0), Krippendorffs Alpha 0,41 (95-%-Intervall 0,12 bis 0,66; Perzentil-Bootstrap mit 1000 Ziehungen, Startwert 20261117).\n\nRunde 2: 25 Einträge, 3 Personen beteiligt. Übereinstimmung vor der Klärung: 84,0 % (21 von 25 mehrfach bewerteten Einträgen; 95-%-Intervall 68,0 bis 96,0), Krippendorffs Alpha 0,71 (95-%-Intervall 0,44 bis 0,92; Perzentil-Bootstrap mit 1000 Ziehungen, Startwert 20261117).\n\n## Titel/Abstract\n\nJeder Eintrag wurde von mindestens 2 Personen unabhängig bewertet; die Stimmen der anderen waren bis zum Entblinden nicht sichtbar. Entschieden war ein Eintrag bei übereinstimmenden Stimmen. „Vielleicht“ zählte als Einschluss. Abweichungen wurden im Gespräch geklärt. Es galten die Kriterien in Version 2.\n\n380 Einträge, 3 Personen beteiligt. Übereinstimmung vor der Klärung: 93,4 % (355 von 380 mehrfach bewerteten Einträgen; 95-%-Intervall 90,8 bis 95,8), Krippendorffs Alpha 0,58 (95-%-Intervall 0,43 bis 0,71; Perzentil-Bootstrap mit 1000 Ziehungen, Startwert 20261117). Über 25 Einträge wurde in der Klärung entschieden.\n\nGeprüft: 380, eingeschlossen: 14, ausgeschlossen: 366.\n\n## Volltext-Screening\n\nJeder Eintrag wurde von mindestens 2 Personen unabhängig bewertet; die Stimmen der anderen waren bis zum Entblinden nicht sichtbar. Entschieden war ein Eintrag bei übereinstimmenden Stimmen. „Vielleicht“ zählte als Einschluss. Abweichungen wurden im Gespräch geklärt. Jeder Ausschluss trägt genau einen Hauptgrund aus den Ausschlusskriterien. Nannten die Stimmen verschiedene Gründe und gab es keine Klärung, zählt der Grund, der in den Kriterien zuerst steht. Es galten die Kriterien in Version 2.\n\n12 Einträge, 2 Personen beteiligt. Übereinstimmung vor der Klärung: 77,8 % (7 von 9 mehrfach bewerteten Einträgen; 95-%-Intervall 44,4 bis 100,0), Krippendorffs Alpha 0,53 (95-%-Intervall -0,13 bis 1,00; Perzentil-Bootstrap mit 1000 Ziehungen, Startwert 20261117). Die Stufe ist noch offen.\n\nFür 14 Einträge wurde der Volltext gesucht; 1 war nicht beschaffbar, bei 1 läuft die Beschaffung oder Prüfung noch. Geprüft: 12, eingeschlossen: 5, ausgeschlossen: 4, noch offen: 3.\n\nAusschlussgründe:\n\n- E1 Keine Erwachsenen mit Migräne oder Kopfschmerz: 1\n- E2 Keine digitale Intervention: 2\n- E4 Kein Volltext in Deutsch oder Englisch: 1\n\n## Ergebnis\n\n5 Einträge wurden eingeschlossen (vorläufig).\n",
  "search_appendix": "# Suchanhang\n\n## MEDLINE (Ovid)\n\n- Herkunft: Datenbank\n- Tag der Suche: 03.11.2026\n- Grenzen: ab 2015, Deutsch und Englisch\n- Treffer der gesamten Suche laut Quelle: 212\n- Importiert: 212 (davon 0 beim Import als Dublette erkannt)\n- Peer-Review am 05.11.2026: Bibliothekarin der Einrichtung, PRESS: ohne Befund\n\nSuchstring (Version 2):\n\n```\n(exp Migraine Disorders/ OR migrain*.ti,ab. OR headache*.ti,ab.) AND (telemedicine/ OR mhealth.ti,ab. OR app.ti,ab. OR digital*.ti,ab.)\n```\n\n## Embase (Ovid)\n\n- Herkunft: Datenbank\n- Tag der Suche: 03.11.2026\n- Grenzen: ab 2015, Deutsch und Englisch\n- Importiert: 187 (davon 51 beim Import als Dublette erkannt)\n\nSuchstring (Version 1):\n\n```\n('migraine'/exp OR migrain*:ti,ab OR headache*:ti,ab) AND ('telemedicine'/exp OR mhealth:ti,ab OR app:ti,ab OR digital*:ti,ab)\n```\n",
  "n_excluded_rows": 5
};
