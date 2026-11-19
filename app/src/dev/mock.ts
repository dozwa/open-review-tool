// Nur für die Vorschau im Browser während der Entwicklung (`npm run dev`, dann http://127.0.0.1:5173).
// Ersetzt die Rust-Befehle durch Beispieldaten, damit sich die Gestaltung ohne App ansehen lässt.
// Wird in der App und im Release-Build nie geladen. Mit `?neu` startet die Vorschau beim Erststart.

import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

import { core } from "./core";
import { formStage, mockForms } from "./mockForms";
import { mockGuidance } from "./mockGuidance";
import { mockReport } from "./mockReport";

// Protokoll, Abweichungen und Checkliste des Berichts
const mockProtocol: { registration: Record<string, unknown> | null; deviations: Record<string, string>[] } = {
  registration: {
    registered_on: "2026-09-14", registry: "OSF", identifier: "osf.io/k7m2x", note: "",
    recorded_at: "2026-10-02T09:12:00Z", corrected: false,
  },
  deviations: [
    { kind: "criteria", at: "2026-11-05T10:00:00Z", title: "Kriterien geändert (Version 2)", note: "Nach dem Pilot: Altersgrenze genauer gefasst" },
    { kind: "gate", at: "2026-11-09T14:30:00Z", title: "„Zitationssuche“ ohne erfülltes Gate freigegeben", note: "Zitationssuche folgt nach dem Volltext-Screening" },
    { kind: "form", at: "2026-11-17T08:45:00Z", title: "Formular „Charting-Formular“ nach der Bestätigung geändert (Version 2)", note: "Feld „Art der Intervention“ fehlte im Pilot" },
  ],
};

const CHECKLIST: [string, string, string, string][] = [
  ["Titel", "Bericht als Scoping Review kennzeichnen", "manual", "Schreibt ihr selbst."],
  ["Zusammenfassung", "Strukturierte Zusammenfassung", "manual", "Schreibt ihr selbst."],
  ["Einleitung", "Begründung des Reviews", "manual", "Schreibt ihr selbst."],
  ["Einleitung", "Ziele und Fragestellung", "manual", "Schreibt ihr selbst. Die Elemente der Fragestellung stehen in den Kriterien."],
  ["Methoden", "Protokoll und Registrierung", "ready", "Registrierung und Abweichungen stehen im Bericht unter „Protokoll“."],
  ["Methoden", "Einschlusskriterien", "ready", "Reiter „Kriterien“, Version 2."],
  ["Methoden", "Informationsquellen mit Tag der letzten Suche", "ready", "Suchanhang: Quellen und Tag der Suche."],
  ["Methoden", "Vollständige Suchstrategie mindestens einer Quelle", "ready", "Suchanhang: Suchstring, Grenzen und Treffer je Quelle."],
  ["Methoden", "Auswahl der Quellen", "ready", "Methodentext, Abschnitte zur Auswahl."],
  ["Methoden", "Ablauf des Chartings", "ready", "Methodentext, Abschnitt „Charting“."],
  ["Methoden", "Erhobene Angaben", "ready", "Die Felder des Formulars stehen im Methodentext."],
  ["Methoden", "Kritische Bewertung der Quellen, falls gemacht", "optional", "In dieser Vorlage nicht vorgesehen. Wer bewertet, beschreibt Vorgehen und Ergebnis im Text."],
  ["Methoden", "Vorgehen bei der Zusammenfassung der Ergebnisse", "ready", "Die Auswertung je Feld steht im Formular und im Methodentext."],
  ["Ergebnisse", "Auswahl der Quellen mit Zahlen und Flussdiagramm", "ready", "Flussdiagramm und ausgeschlossene Volltexte mit Grund."],
  ["Ergebnisse", "Merkmale der Quellen", "open", "Entsteht, sobald Quellen extrahiert und abgeglichen sind."],
  ["Ergebnisse", "Kritische Bewertung innerhalb der Quellen, falls gemacht", "optional", "In dieser Vorlage nicht vorgesehen. Wer bewertet, beschreibt Vorgehen und Ergebnis im Text."],
  ["Ergebnisse", "Ergebnisse je Quelle", "open", "Entsteht, sobald Quellen extrahiert und abgeglichen sind."],
  ["Ergebnisse", "Zusammenfassung der Ergebnisse", "manual", "Schreibt ihr selbst."],
  ["Diskussion", "Zusammenfassung der Evidenz", "manual", "Schreibt ihr selbst."],
  ["Diskussion", "Limitationen", "ready", "Methodentext, Abschnitt „Abweichungen und Limitationen“. Das betrifft das Vorgehen; die Grenzen der Evidenz schreibt ihr selbst."],
  ["Diskussion", "Schlussfolgerungen", "manual", "Schreibt ihr selbst."],
  ["Finanzierung", "Finanzierung des Reviews und der Quellen", "manual", "Schreibt ihr selbst."],
];

/** Was der Bericht seit Teil 2 zusätzlich trägt; der freie Ablauf hat keine Checkliste. */
function reportExtras(jbi: boolean) {
  return {
    protocol: jbi ? mockProtocol : { registration: null, deviations: [] },
    checklist: jbi
      ? { name: "PRISMA-ScR", items: CHECKLIST.map(([section, topic, status, material], i) => ({ number: i + 1, section, topic, status, material })) }
      : null,
    table: jbi ? { n_sources: 3, n_charted: 2, n_fields: 6 } : { n_sources: 0, n_charted: 0, n_fields: 0 },
  };
}


const me = { actor_id: "a1b2c3d4-0000-4000-8000-000000000001", name: "Dorian Zwanzig", public_key: "x" };
const criterion = (code: string, kind: string, element: string, text: string, note = "") => ({ code, kind, element, text, note });
const criteria = {
  version: 2,
  previous_version: 1,
  items: [
    criterion("I1", "include", "population", "Menschen mit Migräne, alle Altersgruppen"),
    criterion("I2", "include", "concept", "Digitale Intervention, z. B. App oder Telemedizin", "Auch Wearables, wenn sie Rückmeldung geben"),
    criterion("I3", "include", "source_type", "Originalstudie, auch qualitativ"),
    criterion("E1", "exclude", "population", "Falsche Population", "Andere Kopfschmerzarten ohne eigene Auswertung für Migräne"),
    criterion("E2", "exclude", "concept", "Keine digitale Intervention"),
    criterion("E3", "exclude", "source_type", "Kein Originalbeitrag", "Editorials, Kommentare, Übersichtsarbeiten"),
    criterion("E4", "exclude", "source_type", "Protokoll ohne Daten"),
  ],
  guidance: "Im Zweifel bei Titel und Abstract einschließen; entschieden wird am Volltext.\nGemischte Stichproben zählen, wenn Migräne getrennt ausgewertet ist.",
  include_keywords: ["migraine", "smartphone", "app", "telemedicine"],
  exclude_keywords: ["editorial", "protocol", "tension-type"],
  change_note: "Qualitative Studien ausdrücklich eingeschlossen",
};
const criteriaSchema = [
  { key: "population", label: "Population" }, { key: "concept", label: "Concept" },
  { key: "context", label: "Context" }, { key: "source_type", label: "Quellenart" },
].map((element) => ({ ...element, label: core(element.label) }));
const stage = (id: string, name: string, kind: string, extra: Record<string, unknown>) => ({
  id, review_id: "r1", name, kind, blind: true, min_reviewers: 2, maybe_as: null, scope: "all",
  sample_size: null, seed: null, n_records: 38, opened_at: "2026-11-12T09:00:00Z",
  unblinded: false, closed: false, blind_active: true, station: kind, round: 1,
  decision_rule: "unanimous", conflict_rule: "discussion", verify: null, reason_required: false, criteria_version: "2", form_id: null, form_version: null, ...extra,
});
const pilotDone = { n_records: 25, unblinded: true, closed: true, blind_active: false, maybe_as: "include" };
const stages = [
  stage("s0", "Pilot Titel/Abstract", "pilot_title_abstract", { ...pilotDone, seed: 42 }),
  stage("s1", "Pilot Titel/Abstract (Runde 2)", "pilot_title_abstract", { ...pilotDone, seed: 77, round: 2 }),
  stage("s2", "Titel/Abstract", "title_abstract", { unblinded: true, blind_active: false, maybe_as: "include" }),
];
// Zum Ansehen der verblindeten Ansicht mit Pflichtgrund: /#/reviews/r1/stages/s3/screen
const fulltextStage = stage("s3", "Volltext-Screening", "fulltext", { n_records: 12, reason_required: true, criteria_version: "1" });

// Ablauf wie ihn der Kern für die Vorlage „JBI Scoping Review“ liefert, mitten im Titel/Abstract-Screening.
const rule = <T,>(value: T, editable = true) => ({ value, editable });
const rules = {
  decision_rule: rule("unanimous"), min_reviewers: rule(2), maybe_as: rule<string | null>("include"),
  conflict_rule: rule("discussion"), blind: rule(true),
};
const gate = (agree: number, n: number) => ({
  passed: agree * 100 >= 75 * n,
  checks: [{ check: "agreement_min", passed: agree * 100 >= 75 * n, numerator: agree, denominator: n, threshold_percent: 75, percent_x100: Math.floor((agree * 10000) / n) }],
});
// Namen, Beschreibungen und Prüfungen der Stationen kommen im Kern aus der Vorlage und folgen der Sprache.
const labelled = <T extends { label: string }>(item: T): T => ({ ...item, label: core(item.label) });
const station = (key: string, kind: string, label: string, description: string, extra: Record<string, unknown> = {}) => {
  const s = {
    key, kind, label, description, tool: null, state: "locked", available: kind === "screening", blocking: false,
    blocked_by: [{ key: "title_abstract", label: "Titel/Abstract" }], checks: [] as { label: string }[], rounds: [], overridden: null, free_rounds: false,
    defaults: kind === "screening" ? { name: label, round: 1, scope: "all", sample_size: null, source_stage: null, rules, reason_required: false } : null,
    ...extra,
  };
  return {
    ...s, label: core(s.label), description: core(s.description), blocked_by: s.blocked_by.map(labelled), checks: s.checks.map(labelled),
    defaults: s.defaults && { ...s.defaults, name: core(s.defaults.name) },
  };
};
const done = { state: "done", available: true, blocking: true, blocked_by: [] };
const jbiStations = [
  station("protocol", "setup", "Protokoll und Team", "Kriterien veröffentlichen und das Team zusammenstellen.", {
    ...done, tool: "protocol",
    checks: [
      { check: "criteria_published", label: "Kriterien veröffentlicht", passed: true, detail: "" },
      { check: "min_writers", label: "Mindestens 2 Personen mit Schreibrecht", passed: true, detail: "4 vorhanden" },
    ],
  }),
  station("search", "setup", "Suche", "Je Quelle eine Suchversion mit Suchstring, Datum und Grenzen festhalten.", {
    ...done, tool: "search", checks: [{ check: "search_documented", label: "Suche dokumentiert", passed: true, detail: "2 Suchen" }],
  }),
  station("import", "setup", "Import und Dubletten", "Treffer je Suche einlesen. Gleiche Einträge werden erkannt, mögliche Dubletten einzeln entschieden.", {
    ...done, tool: "import", overridden: "Die zwei offenen Paare klären wir nach dem Pilot.",
    checks: [
      { check: "records_imported", label: "Treffer importiert", passed: true, detail: "38 Einträge" },
      { check: "no_unresolved_duplicates", label: "Mögliche Dubletten entschieden", passed: false, detail: "2 offen" },
    ],
  }),
  station("pilot_title_abstract", "screening", "Pilot Titel/Abstract", "Das ganze Team bewertet eine Zufallsstichprobe. Weiter geht es ab 75 % Übereinstimmung; sonst Kriterien überarbeiten und eine neue Runde öffnen.", {
    ...done,
    checks: [{ check: "agreement_min", label: "Übereinstimmung mindestens 75 %", passed: true, detail: "84,0 % bei 25 doppelt bewerteten Einträgen" }],
    rounds: [
      { round: 1, stage_id: "s0", name: "Pilot Titel/Abstract", closed: true, gate: gate(17, 25) },
      { round: 2, stage_id: "s1", name: "Pilot Titel/Abstract (Runde 2)", closed: true, gate: gate(21, 25) },
    ],
    defaults: { name: "Pilot Titel/Abstract (Runde 3)", round: 3, scope: "sample", sample_size: 25, source_stage: null, rules: { ...rules, decision_rule: rule("unanimous", false) }, reason_required: false },
  }),
  station("title_abstract", "screening", "Titel/Abstract", "Alle Einträge ohne Dubletten, danach Klärung der Abweichungen.", {
    state: "running", blocking: true, blocked_by: [],
    checks: [
      { check: "all_decided", label: "Alle Einträge entschieden", passed: null, detail: "" },
      { check: "no_open_conflicts", label: "Keine offenen Konflikte", passed: null, detail: "" },
    ],
    rounds: [{ round: 1, stage_id: "s2", name: "Titel/Abstract", closed: false, gate: null }],
  }),
  station("fulltext_retrieval", "setup", "Volltexte beschaffen", "Jeder eingeschlossene Eintrag ist beschafft oder mit Grund nicht beschaffbar.", {
    tool: "fulltext", available: true, blocking: true,
    checks: [{ check: "all_fulltext_status", label: "Jeder Volltext beschafft oder als nicht beschaffbar vermerkt", passed: false, detail: "2 von 6 beschafft, 1 nicht beschaffbar" }],
  }),
  station("fulltext", "screening", "Volltext-Screening", "Eingeschlossene aus Titel/Abstract mit beschafftem Volltext prüfen, danach Klärung. Jeder Ausschluss braucht genau einen Hauptgrund. Später beschaffte Volltexte laufen als weitere Runde.", {
    blocking: true, defaults: { name: "Volltext-Screening", round: 1, scope: "from_stage", sample_size: null, source_stage: "s2", rules, reason_required: true },
  }),
  station("citation_search", "setup", "Zitationssuche", "Vorwärts und rückwärts suchen, bis nichts Neues mehr kommt. Die Suche wird wie jede andere festgehalten, Treffer mit der Herkunft „Zitationssuche“ eingelesen. Neue Einträge laufen als weitere Runde durch Titel/Abstract und Volltext.", {
    tool: "citation", available: true, blocking: true,
    checks: [
      { check: "citation_search_documented", label: "Zitationssuche festgehalten", passed: false, detail: "noch keine Suche und kein Import mit dieser Herkunft" },
      { check: "all_records_assigned", label: "Alle Einträge einer Runde zugeteilt", passed: true, detail: "0 ohne Runde" },
    ],
  }),
  station("bundling", "setup", "Quellen bündeln", "Begleitpublikationen derselben Studie zu einer Quelle zusammenfassen und jeder Quelle eine Quellenart geben.", {
    tool: "bundling", available: true, blocking: true,
    checks: [{ check: "source_types_set", label: "Jede eingeschlossene Quelle hat eine Quellenart", passed: false, detail: "3 von 4 ohne Quellenart" }],
  }),
  // In der Vorschau läuft der Pilot schon, damit Tabelle und Eingabe etwas zeigen: /#/reviews/r1/forms/s4
  station("charting_pilot", "form", "Charting-Pilot", "Je Quellenart zwei Quellen unabhängig extrahieren, das Formular abstimmen und bestätigen.", {
    state: "running", available: true, blocking: true, blocked_by: [],
    checks: [{ check: "form_confirmed", label: "Formular von der Leitung bestätigt", passed: false, detail: "" }],
    rounds: [{ round: 1, stage_id: "s4", name: "Charting-Pilot", closed: false, gate: null }],
    defaults: { name: "Charting-Pilot (Runde 2)", round: 2, scope: "sample", sample_size: 2, source_stage: null, rules, reason_required: false },
  }),
  station("charting", "form", "Charting", "Alle Quellen mit dem abgestimmten Formular extrahieren und abgleichen. Später dazugekommene Quellen laufen als weitere Runde.", {
    available: true, blocking: true, blocked_by: [{ key: "charting_pilot", label: "Charting-Pilot" }],
    checks: [
      { check: "all_sources_assigned", label: "Alle Quellen einer Runde zugeteilt", passed: false, detail: "3 ohne Runde" },
      { check: "all_extracted", label: "Alle Quellen extrahiert", passed: null, detail: "" },
      { check: "no_open_differences", label: "Keine offenen Abweichungen", passed: null, detail: "" },
    ],
    defaults: { name: "Charting", round: 1, scope: "all", sample_size: null, source_stage: null, rules, reason_required: false },
  }),
  station("report", "report", "Auswertung und Bericht", "Flussdiagramm, Tabellen und Methodentext.", { available: true }),
];
const terms = { record: core("Eintrag"), records: core("Einträge"), source: core("Quelle"), sources: core("Quellen"), extraction: core("Charting") };
const jbiWorkflow = {
  published: true, version: 1, template_key: "jbi_scoping", template_name: core("JBI Scoping Review"), review_type: "scoping", terms,
  criteria_schema: criteriaSchema, stations: jbiStations,
};
// Ein Review aus der Zeit vor den Vorlagen: freier Ablauf, nichts gesperrt.
const freeWorkflow = {
  published: false, version: 0, template_key: "free", template_name: core("Freier Ablauf"), review_type: "free", terms,
  criteria_schema: [], stations: [
    station("protocol", "setup", "Kriterien und Team", "Kriterien festhalten und Personen hinzufügen.", { state: "open", available: true, tool: "protocol", blocked_by: [] }),
    station("import", "setup", "Import", "Treffer einlesen; Dubletten werden erkannt und gezählt.", { state: "open", available: true, tool: "import", blocked_by: [] }),
    station("screening", "screening", "Screening", "Beliebig viele Stufen, auch nebeneinander.", {
      state: "open", blocked_by: [], free_rounds: true,
      defaults: { name: "Screening", round: 1, scope: "all", sample_size: null, source_stage: null, rules: { ...rules, min_reviewers: rule(1), maybe_as: rule(null), blind: rule(false) }, reason_required: false },
    }),
  ],
};
const templates = [
  { key: "jbi_scoping", name: "JBI Scoping Review", review_type: "scoping", description: "Ablauf nach dem JBI Manual, Kapitel 10: Pilot mit mindestens 75 % Übereinstimmung, zwei unabhängige Stimmen, Klärung, Volltext, Charting und Bericht nach PRISMA-ScR.", stations: jbiStations.map((s) => s.label) },
  { key: "rapid_review", name: "Rapid Review", review_type: "rapid", description: "Abgekürzter Ablauf nach den Empfehlungen der Cochrane Rapid Reviews Methods Group (2024): doppelte Bewertung einer Stichprobe mit mindestens 80 % Übereinstimmung, danach eine Stimme je Eintrag mit Prüfung der Ausschlüsse, Extraktion durch eine Person mit Prüfung. Jede Abkürzung steht als Limitation im Methodentext.", stations: ["Protokoll und Team", "Suche", "Import und Dubletten", "Pilot und doppelte Bewertung", "Titel/Abstract", "Volltexte beschaffen", "Volltext-Screening", "Studien bündeln", "Extraktions-Pilot", "Datenextraktion und Bewertung", "Auswertung und Bericht"] },
  { key: "state_of_the_art", name: "State-of-the-Art-Review", review_type: "state_of_the_art", description: "Übersicht zum aktuellen Stand eines Feldes, angelehnt an die sechs Schritte von Barry, Merkebu und Varpio (2022): Frage und Zeitraum festlegen, suchen, auswählen, auswerten. Eine Stimme je Eintrag, keine Abschlussregeln. Einen verbindlichen Berichtsstandard gibt es nicht.", stations: ["Frage, Feld und Zeitraum", "Suche", "Import", "Titel/Abstract", "Volltexte beschaffen", "Volltext-Auswahl", "Auswertung", "Bericht"] },
  { key: "free", name: "Freier Ablauf", review_type: "free", description: "Ohne feste Reihenfolge und ohne Abschlussregeln: Stufen lassen sich jederzeit öffnen, eine Stimme je Eintrag genügt. Für Literaturübersichten und zum Ausprobieren.", stations: ["Kriterien und Team", "Import", "Screening"] },
].map((template) => ({ ...template, name: core(template.name), description: core(template.description), stations: template.stations.map(core) }));
const members = [
  { actor_id: me.actor_id, name: me.name, role: "owner", is_me: true },
  { actor_id: "b0000000-0000-4000-8000-000000000002", name: "Anna Berger", role: "reviewer", is_me: false },
  { actor_id: "c0000000-0000-4000-8000-000000000003", name: "Ben Krüger", role: "reviewer", is_me: false },
  { actor_id: "d0000000-0000-4000-8000-000000000004", name: "Clara Weiß", role: "reviewer", is_me: false },
  { actor_id: "e0000000-0000-4000-8000-000000000005", name: "Dora Lang", role: "viewer", is_me: false },
];
const record = (i: number, title: string, abstract: string) => ({
  id: `rec-${i}`, title, abstract, authors: "Okafor C.; Rossi M.; Tanaka H.", year: 2023,
  journal: "Cephalalgia Digital", doi: `10.5555/mig.2023.10${i}`, url: "", keywords: "migraine; smartphone app; cohort study",
  duplicate_of: null,
});
const records = [
  record(1, "Smartphone app–based self-management for adults with episodic migraine: a randomised controlled trial",
    "A randomised controlled trial evaluating a smartphone app in adults with episodic migraine. Outcomes were monthly headache days, medication use and quality of life over 12 weeks. The app combined a headache diary, relaxation training and personalised trigger feedback; the control group received usual care. Mean monthly headache days fell by 2.1 in the intervention group compared with 0.8 under usual care."),
  record(2, "Telemedicine consultation for adolescents with migraine: a prospective cohort study",
    "A prospective cohort study evaluating telemedicine consultation in adolescents with migraine. Outcomes were school absence and headache days over 24 weeks."),
  record(3, "Wearable biofeedback for patients with tension-type headache: editorial",
    "An editorial commentary on the use of wearable biofeedback devices in patients with tension-type headache. No original data are reported."),
  record(4, "Digital headache diaries in primary care: a qualitative interview study",
    "Semi-structured interviews with 24 adults with migraine about keeping a digital headache diary in a smartphone app."),
  record(5, "Protocol for a trial of app-delivered relaxation training in chronic migraine",
    "Study protocol. Participants will be randomised to app-delivered relaxation training or waiting list. No results are reported."),
  record(6, "Acupuncture versus sham for episodic migraine: a multicentre trial",
    "A multicentre randomised trial comparing acupuncture with sham acupuncture in adults with episodic migraine."),
  record(7, "Remote monitoring of medication overuse with a smartphone app", ""),
  record(8, "Web-based cognitive behavioural therapy for children with migraine: pilot study",
    "A pilot study of a web-based programme for children aged 8 to 12 years with migraine and their parents."),
];
// Stand der Liste: eigene Stimme und, entblindet sichtbar, Stimmenzahl und Status je Eintrag.
const listState: Record<string, { mine: string | null; primary: string | null; n: number; status: string }> = {
  "rec-1": { mine: "include", primary: null, n: 2, status: "unanimous" },
  "rec-2": { mine: "include", primary: null, n: 2, status: "disagreement" },
  "rec-3": { mine: "exclude", primary: "E3", n: 2, status: "unanimous" },
  "rec-4": { mine: null, primary: null, n: 1, status: "pending" },
  "rec-5": { mine: null, primary: null, n: 1, status: "pending" },
  "rec-6": { mine: null, primary: null, n: 0, status: "pending" },
  "rec-7": { mine: null, primary: null, n: 1, status: "needs_clarification" },
  "rec-8": { mine: null, primary: null, n: 0, status: "pending" },
};
const query = `(migraine[tiab] OR "Migraine Disorders"[mh])
AND (app[tiab] OR "mobile health"[tiab] OR telemedicine[tiab] OR digital[tiab])`;
const version = (n: number, extra: Record<string, unknown>) => ({
  version: n, previous_version: n > 1 ? n - 1 : null, source: "MEDLINE", platform: "PubMed", origin: "database", query,
  searched_on: "2026-11-02", limits: "ab 2015, Englisch und Deutsch", hit_limit: null, hits: 35, change_note: "",
  published_at: "", peer_reviews: [], n_imports: 0, n_records: 0, n_duplicates: 0, ...extra,
});
let searches = [
  {
    id: "q1",
    versions: [
      version(1, { hits: 31, query: query.replace(" OR digital[tiab]", "") }),
      version(2, {
        change_note: "Freitext „digital“ ergänzt", n_imports: 1, n_records: 35,
        peer_reviews: [{ reviewed_on: "2026-11-03", note: "Bibliothekarin der Einrichtung, PRESS: ohne Befund", reviewer: null, recorded_by: "p0" }],
      }),
    ],
  },
  { id: "q2", versions: [version(1, { source: "Scopus", platform: "Elsevier", hits: 6, n_imports: 1, n_records: 5, n_duplicates: 2, query: "TITLE-ABS-KEY(migraine AND (app OR telemedicine OR digital))" })] },
];
const dupRecord = (id: string, extra: Record<string, unknown>) => ({
  id, title: "Smartphone-based headache diary for adults with episodic migraine: a randomised trial", abstract: "",
  authors: "Keller, Jana; Moreau, Luc", year: 2023, journal: "Cephalalgia Reports", doi: "", url: "", keywords: "", duplicate_of: null, ...extra,
});
const sourceTypes = [
  { key: "primary_study", label: "Primärstudie" },
  { key: "review", label: "Review oder Evidenzsynthese" },
  { key: "guideline", label: "Leitlinie oder Policy-Dokument" },
  { key: "grey", label: "Graue Literatur" },
  { key: "opinion", label: "Meinung, Kommentar oder Editorial" },
  { key: "other", label: "Sonstiges" },
].map(labelled);
const sourceRecord = (id: string, title: string, authors: string, year: number, journal: string) =>
  ({ id, title, abstract: "", authors, year, journal, doi: "", url: "", keywords: "", duplicate_of: null, included: true });
// Eingeschlossene Einträge; `study` und `primary` beschreiben die Bündel.
let sourceRecords = [
  { ...sourceRecord("e1", "App-based relaxation training for adults with migraine: a randomised trial", "Hartmann, Eva; Lindqvist, Per", 2022, "Headache Care"), study: "b1", primary: true, type: "primary_study" },
  { ...sourceRecord("e2", "App-based relaxation training for adults with migraine: twelve-month follow-up", "Hartmann, Eva; Lindqvist, Per", 2023, "Headache Care"), study: "b1", primary: false, type: "" },
  { ...sourceRecord("e3", "Smartphone-based headache diary for adults with episodic migraine: a randomised trial", "Keller, Jana; Moreau, Luc", 2023, "Cephalalgia Reports"), study: "", primary: true, type: "" },
  { ...sourceRecord("e4", "Telemedicine follow-up in specialised headache clinics", "Okonkwo, Ada", 2021, "Digital Neurology"), study: "", primary: true, type: "" },
  { ...sourceRecord("e5", "Digital self-management of migraine: recommendations of a working group", "Sauer, Timo", 2024, ""), study: "", primary: true, type: "" },
];
let studyLabels: Record<string, string> = { b1: "Hartmann-Studie" };
function mockSources() {
  const groups = new Map<string, typeof sourceRecords>();
  for (const r of sourceRecords) groups.set(r.study || r.id, [...(groups.get(r.study || r.id) ?? []), r]);
  const sources = [...groups.values()].map((group) => {
    const records = [...group].sort((x, y) => Number(y.primary) - Number(x.primary));
    return {
      id: records[0].id, study_id: records[0].study || null, label: studyLabels[records[0].study] ?? "",
      source_type: records.find((r) => r.type)?.type ?? "", records,
    };
  });
  const count = (key: string) => sources.filter((s) => s.source_type === key).length;
  return {
    sources, source_types: sourceTypes, n_records: sourceRecords.length, n_sources: sources.length, n_without_type: count(""),
    by_type: sourceTypes.map((t) => ({ ...t, n: count(t.key) })).filter((t) => t.n > 0), level: core("Volltext-Screening"), complete: false,
  };
}

let duplicatePairs = [
  {
    a: dupRecord("d1", { abstract: "Background: Paper diaries are often incomplete.", doi: "10.5555/mig.2023.1004" }),
    b: dupRecord("d2", { title: "Smartphone-based headache diary for adults with episodic migraine - a randomised trial.", authors: "Keller, J.", journal: "" }),
    a_source: "MEDLINE", b_source: "Handsuche", similarity: 100, keep: "d1",
  },
  {
    a: dupRecord("d3", { title: "Telemedicine follow-up for chronic migraine in rural areas", authors: "Okafor, Chidi", year: 2022, journal: "Neurology Practice", doi: "10.5555/mig.2022.1011", abstract: "Cohort study." }),
    b: dupRecord("d4", { title: "Telemedicine follow up for chronic migraine in rural regions", authors: "Okafor, C.", year: 2022, journal: "Neurol Pract" }),
    a_source: "MEDLINE", b_source: "Scopus", similarity: 93, keep: "d3",
  },
];
let duplicateCounts = { n_resolved: 0, n_distinct: 0 };

// Kleine zweiseitige PDF für die Vorschau, damit Blättern, Zoom und Suche etwas zu zeigen haben.
function samplePdf(title: string): ArrayBuffer {
  const page = (lines: string[]) => `BT /F1 12 Tf 60 780 Td 16 TL ${lines.map((l) => `(${l.replace(/[()\\]/g, "")}) Tj T*`).join(" ")} ET`;
  const streams = [
    page([title.slice(0, 70), "", "Background: Migraine is a common neurological disorder.", "Methods: Randomised trial of a smartphone app in adults with migraine.", "Results: Monthly migraine days fell in the intervention group."]),
    page(["Discussion", "", "The app was acceptable and adherence was high.", "Limitations include the short follow-up of twelve weeks.", "Keywords: migraine, smartphone, telemedicine"]),
  ];
  const objects = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 5 0 R /Resources << /Font << /F1 7 0 R >> >> >>",
    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 6 0 R /Resources << /Font << /F1 7 0 R >> >> >>",
    ...streams.map((body) => `<< /Length ${body.length} >>\nstream\n${body}\nendstream`),
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
  ];
  let out = "%PDF-1.4\n";
  const offsets = objects.map((body, i) => {
    const at = out.length;
    out += `${i + 1} 0 obj\n${body}\nendobj\n`;
    return at;
  });
  const xref = out.length;
  out += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n${offsets.map((o) => `${String(o).padStart(10, "0")} 00000 n \n`).join("")}`;
  out += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF`;
  return new TextEncoder().encode(out).buffer as ArrayBuffer;
}

const fulltextOf = (path: string) => ({
  status: "retrieved", origin: "folder", path, zotero_key: "", sha256: "ab".repeat(32), size: 1_480_000, reason: "", set_by: me.actor_id, set_at: "2026-11-18T10:00:00Z",
});
const ftRow = (i: number, title: string, authors: string, year: number, extra: Record<string, unknown> = {}) => ({
  record_id: `rec-${i}`, title, authors, year, doi: i % 2 ? `10.5555/mig.2023.10${i}` : "", needed: true, fulltext: null as Record<string, unknown> | null, file: "", zotero_item: null as string | null, ...extra,
});
const fulltexts = {
  folder: "/Users/dorian/Reviews/Migräne/Volltexte" as string | null, folder_found: true,
  zotero: null as Record<string, unknown> | null,
  rows: [
    ftRow(0, "Smartphone-based behavioural therapy for episodic migraine: a randomised trial", "Okafor C.; Rossi M.", 2023, { fulltext: fulltextOf("Okafor - 2023 - Smartphone-based behavioural therapy.pdf"), file: "ok" }),
    ftRow(1, "Telemedicine follow-up in a tertiary headache centre", "Lindqvist P.; Meyer A.", 2022, { fulltext: fulltextOf("unter/lindqvist2022.pdf"), file: "ok" }),
    ftRow(2, "A wearable neuromodulation device with app feedback for migraine prevention", "Tanaka H.", 2021, {
      fulltext: { ...fulltextOf(""), status: "unavailable", origin: "", size: 0, sha256: "", reason: "Fernleihe erfolglos, Autorin antwortet nicht" },
    }),
    ftRow(3, "Digital headache diaries and adherence in adolescents", "Novak J.; Keller J.", 2020),
    ftRow(4, "App-guided relaxation training for chronic migraine: cohort study", "Suzuki K.; Moreau L.", 2024),
    ftRow(5, "Patient experiences with a migraine self-management app: qualitative study", "Haddad S.", 2022, { zotero_item: "ABCD2345" }),
  ],
};
const fulltextView = () => {
  const has = (status: string) => fulltexts.rows.filter((r) => r.fulltext?.status === status).length;
  return { ...fulltexts, needed: fulltexts.rows.length, retrieved: has("retrieved"), unavailable: has("unavailable") };
};
const ftRowOf = (id: unknown) => fulltexts.rows.find((r) => r.record_id === id);

const stageOf = (id: unknown) => [...stages, fulltextStage].find((x) => x.id === id) ?? stages[1];
const nextOpen = () => records.find((r) => !listState[r.id].mine) ?? null;
const doneCount = () => records.filter((r) => listState[r.id].mine).length;

export function installMock() {
  mockWindows("main");
  let identity: typeof me | null = new URLSearchParams(location.search).has("neu") ? null : me;
  mockIPC((cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    const form = mockForms(cmd, a, { members, sourceTypes, terms });
    if (form !== undefined) return form;
    const guidance = mockGuidance(cmd, a, () => mockProtocol.registration);
    if (guidance !== undefined) return guidance;
    switch (cmd) {
      case "identity_get": return identity;
      case "templates_list": return templates;
      case "review_create": return { id: "r2", title: String(a.title), role: "owner" };
      case "stage_gate": return a.stageId === "s2" ? { passed: false, checks: [
        { check: "all_decided", passed: false, numerator: 24, denominator: 38 },
        { check: "no_open_conflicts", passed: false, numerator: 9, denominator: 38 },
      ] } : null;
      case "identity_create": identity = { ...me, name: String(a.name) }; return identity;
      case "reviews_list": return [
        { id: "r1", title: "Digitale Interventionen bei Migräne", role: "owner" },
        { id: "r2", title: "KI-gestützte Triage in der Notaufnahme", role: "admin" },
      ];
      case "review_detail": return {
        id: a.reviewId, title: a.reviewId === "r2" ? "KI-gestützte Triage in der Notaufnahme" : "Digitale Interventionen bei Migräne",
        role: "owner", me: me.actor_id, members, n_records: 40, n_duplicates: 2, criteria,
        stages: a.reviewId === "r2" ? [] : [...stages, formStage],
        workflow: a.reviewId === "r2" ? freeWorkflow : jbiWorkflow,
        imports: [
          { id: "i1", filename: "pubmed_2026-11-02.nbib", format: "nbib", source: "MEDLINE", status: "done", n_records: 35, n_duplicates: 0, error: "", created_at: "", origin: "database", search_id: "q1", search_version: 2 },
          { id: "i2", filename: "scopus_2026-11-02.bib", format: "bibtex", source: "Scopus", status: "done", n_records: 5, n_duplicates: 2, error: "", created_at: "", origin: "database", search_id: "q2", search_version: 1 },
          { id: "i3", filename: "handsuche.ris", format: "ris", source: "Handsuche", status: "done", n_records: 1, n_duplicates: 0, error: "", created_at: "", origin: "other", search_id: null, search_version: null },
        ],
      };
      case "searches": return a.reviewId === "r2" ? [] : searches;
      case "search_publish": {
        const d = a.data as Record<string, unknown>;
        const existing = searches.find((x) => x.id === d.search_id);
        const entry = version(existing ? existing.versions.length + 1 : 1, { ...d, published_at: "" });
        if (existing) existing.versions.push(entry);
        else searches = [...searches, { id: `q${searches.length + 1}`, versions: [entry] }];
        return existing ?? searches[searches.length - 1];
      }
      case "search_review": {
        const target = searches.find((x) => x.id === a.searchId)?.versions.find((v) => v.version === a.version);
        target?.peer_reviews.push({ reviewed_on: String(a.reviewedOn), note: String(a.note), reviewer: (a.reviewer as string | null) ?? null, recorded_by: "p0" } as never);
        return null;
      }
      case "duplicates_list": return a.reviewId === "r2"
        ? { pairs: [], total: 0, n_exact: 0, n_resolved: 0, n_distinct: 0 }
        : { pairs: duplicatePairs, total: duplicatePairs.length, n_exact: 2, ...duplicateCounts };
      case "duplicates_resolve": {
        const decided = a.pairs as { a: string; b: string; decision: string }[];
        for (const d of decided) {
          duplicatePairs = duplicatePairs.filter((p) => !(p.a.id === d.a && p.b.id === d.b));
          if (d.decision === "duplicate") duplicateCounts = { ...duplicateCounts, n_resolved: duplicateCounts.n_resolved + 1 };
          else duplicateCounts = { ...duplicateCounts, n_distinct: duplicateCounts.n_distinct + 1 };
        }
        return decided.length;
      }
      case "stage_detail": {
        const s = stageOf(a.stageId);
        const old = s.criteria_version === "1";
        return {
          stage: s, role: "owner", progress: { done: doneCount(), total: records.length },
          criteria: old ? { ...criteria, version: 1, previous_version: null } : criteria, newer_criteria: old,
          criteria_schema: criteriaSchema, members, terms,
        };
      }
      case "stage_next": return { record: nextOpen(), progress: { done: doneCount(), total: records.length } };
      case "stage_records": {
        const blind = stageOf(a.stageId).blind_active;
        const f = (a.filter ?? {}) as Record<string, string | number | null>;
        const text = (r: (typeof records)[number]) => `${r.title}\n${r.abstract}`.toLowerCase();
        const hits = records.filter((r) => {
          const st = listState[r.id];
          if (f.decision === "open" && st.mine) return false;
          if (f.decision === "conflict" && !["disagreement", "needs_clarification"].includes(st.status)) return false;
          if (["include", "exclude", "maybe"].includes(String(f.decision)) && st.mine !== f.decision) return false;
          if (typeof f.votes_min === "number" && st.n < f.votes_min) return false;
          if (typeof f.votes_max === "number" && st.n > f.votes_max) return false;
          if (f.keyword && !text(r).includes(String(f.keyword).toLowerCase())) return false;
          return !f.query || text(r).includes(String(f.query).toLowerCase());
        });
        const count = (kind: string) => (keyword: string) => ({ keyword, kind, n: records.filter((r) => text(r).includes(keyword)).length });
        return {
          items: hits.map((r) => {
            const st = listState[r.id];
            return { id: r.id, title: r.title, year: r.year, my_decision: st.mine, wants_my_vote: !st.mine, n_votes: blind ? null : st.n, status: blind ? null : st.status, decision: null };
          }),
          total: hits.length,
          keywords: [...criteria.include_keywords.map(count("include")), ...criteria.exclude_keywords.map(count("exclude"))],
          locked: blind ? ["conflict", "person", "votes"] : [],
        };
      }
      case "stage_record": {
        const st = listState[String(a.recordId)];
        return {
          record: records.find((r) => r.id === a.recordId),
          my_vote: st?.mine ? { decision: st.mine, primary_reason: st.primary, reasons: [], note: "" } : null,
          fulltext: a.stageId === "s3" ? fulltextOf("Okafor - 2023 - Smartphone-based behavioural therapy.pdf") : null,
        };
      }
      case "vote_cast": {
        const st = listState[String(a.recordId)];
        if (!st.mine) st.n += 1;
        st.mine = String(a.decision);
        st.primary = (a.primaryReason as string | null) ?? null;
        return null;
      }
      case "record_flag": listState[String(a.recordId)].mine = "maybe"; return null;
      case "stage_summary": {
        const s = stageOf(a.stageId);
        const reviewers = [{ name: "Anna Berger", n_votes: 38 }, { name: "Ben Krüger", n_votes: 32 }, { name: "Clara Weiß", n_votes: 15 }, { name: me.name, n_votes: 31 }];
        if (s.blind_active) return { stage: s, progress: { done: 4, total: s.n_records }, reviewers, decisions: null, agreement: null };
        const est = (v: number, lo: number, hi: number) => ({ value: v, ci_low: lo, ci_high: hi, level: 0.95, n_items: 32, n_boot: 1000, seed: 20261117 });
        return {
          stage: s, progress: { done: 31, total: 38 }, reviewers,
          decisions: { include: 9, exclude: 14, maybe: 1, disagreement: 7, needs_clarification: 2, pending: 5 },
          agreement: {
            percent: { n_items: 32, percent_agreement: 0.719 },
            percent_ci: est(0.719, 0.563, 0.875),
            krippendorff: { n_items: 32, alpha: 0.452 },
            alpha_ci: est(0.452, 0.193, 0.689),
            cohen_pairs: [
              { rater_a: "Anna Berger", rater_b: "Ben Krüger", n_items: 32, p_o: 0.75, p_e: 0.48, kappa: 0.52, pabak: 0.5 },
              { rater_a: "Anna Berger", rater_b: "Clara Weiß", n_items: 15, p_o: 0.8, p_e: 0.5, kappa: 0.6, pabak: 0.6 },
            ],
          },
        };
      }
      case "stage_conflicts": return [
        { record: records[1], status: "disagreement", votes: [
          { name: "Anna Berger", decision: "include", primary_reason: null, reasons: [], note: "" },
          { name: "Ben Krüger", decision: "exclude", primary_reason: "E1", reasons: [], note: "Nur Jugendliche?" },
        ] },
        { record: { ...records[2], abstract: "" }, status: "needs_clarification", votes: [
          { name: "Clara Weiß", decision: "maybe", primary_reason: null, reasons: [], note: "Kein Abstract vorhanden, Volltext nötig." },
        ] },
      ];
      case "sources": return a.reviewId === "r2"
        ? { sources: [], source_types: [], n_records: 0, n_sources: 0, n_without_type: 0, by_type: [], level: null, complete: false }
        : mockSources();
      case "study_link": {
        const chosen = a.records as string[];
        const study = (a.studyId as string | null) ?? `b${Object.keys(studyLabels).length + 1}`;
        studyLabels = { ...studyLabels, [study]: String(a.label) };
        sourceRecords = sourceRecords.map((r) => (chosen.includes(r.id) ? { ...r, study, primary: r.id === a.primary } : r));
        return study;
      }
      case "study_dissolve":
        sourceRecords = sourceRecords.map((r) => (r.study === a.studyId ? { ...r, study: "", primary: true } : r));
        return null;
      case "source_type_set": {
        const chosen = a.records as string[];
        sourceRecords = sourceRecords.map((r) => (chosen.includes(r.id) ? { ...r, type: String(a.sourceType) } : r));
        return chosen.length;
      }
      case "protocol_register":
        mockProtocol.registration = {
          registered_on: a.registeredOn as string, registry: a.registry as string, identifier: a.identifier as string,
          note: a.note as string, recorded_at: mockProtocol.registration?.recorded_at ?? new Date().toISOString(),
          corrected: mockProtocol.registration !== null,
        };
        return mockProtocol;
      case "report": return a.reviewId === "r2"
        ? { ...reportExtras(false), ...mockReport, flow: { ...mockReport.flow, sources: [], identified: 0, exact_duplicates: 0, resolved_duplicates: 0, unique: 0, levels: [], included: null }, svg: "", methods: "# Methoden: Auswahl der Quellen\n\nDas Review lief ohne feste Vorlage.", search_appendix: "# Suchanhang\n\nEs ist keine Suche festgehalten.\n", n_excluded_rows: 0 }
        : { ...mockReport, ...reportExtras(true) };
      case "report_export": return null;
      case "fulltext_overview": return a.reviewId === "r2" ? { folder: null, folder_found: false, zotero: null, needed: 0, retrieved: 0, unavailable: 0, rows: [] } : fulltextView();
      case "fulltext_folder_set": fulltexts.folder = (a.path as string | null) ?? null; return null;
      case "fulltext_suggestions": return fulltexts.rows.filter((r) => !r.fulltext).slice(0, 2).map((r, i) => ({
        record_id: r.record_id, title: r.title, authors: r.authors, year: r.year, size: 900_000,
        path: i === 0 ? "Novak - 2020 - Digital headache diaries and adherence.pdf" : "suzuki2024.pdf",
        basis: i === 0 ? "title" : "author_year", similarity: i === 0 ? 97 : 0,
      }));
      case "fulltext_assign": {
        const list = a.list as { record_id: string; path: string }[];
        list.forEach((item) => Object.assign(ftRowOf(item.record_id) ?? {}, { fulltext: fulltextOf(item.path), file: "ok" }));
        return list.length;
      }
      case "fulltext_unavailable": Object.assign(ftRowOf(a.recordId) ?? {}, { fulltext: { ...fulltextOf(""), status: "unavailable", origin: "", reason: String(a.reason) } }); return null;
      case "fulltext_reset": Object.assign(ftRowOf(a.recordId) ?? {}, { fulltext: null, file: "" }); return null;
      case "fulltext_verify": return [{ record_id: "rec-1", title: fulltexts.rows[1].title, path: "unter/lindqvist2022.pdf", problem: "changed" }];
      case "fulltext_read": return samplePdf(ftRowOf(a.recordId)?.title ?? records.find((r) => r.id === a.recordId)?.title ?? "Volltext");
      case "fulltext_open_external": return null;
      case "zotero_libraries": return [
        { library_type: "user", library_id: 0, name: "Meine Bibliothek", collections: [{ key: "COLL0001", name: "Migräne Review" }] },
        { library_type: "group", library_id: 4711, name: "AG Kopfschmerz", collections: [] },
      ];
      case "zotero_link": fulltexts.zotero = a.link as Record<string, unknown>; return null;
      case "zotero_suggestions": return fulltexts.rows.filter((r) => !r.fulltext).slice(0, 2).map((r, i) => ({
        record_id: r.record_id, title: r.title, authors: r.authors, year: r.year, item_key: `ITEM000${i}`, item_title: r.title,
        attachment_key: i === 0 ? "PDF00001" : null, basis: i === 0 ? "doi" : "title", similarity: 100,
      }));
      case "zotero_confirm": {
        const list = a.list as { record_id: string; item_key: string; attachment_key: string | null }[];
        list.forEach((item) => Object.assign(ftRowOf(item.record_id) ?? {}, item.attachment_key
          ? { fulltext: { ...fulltextOf(""), origin: "zotero", zotero_key: item.attachment_key }, zotero_item: item.item_key }
          : { zotero_item: item.item_key }));
        return list.filter((item) => item.attachment_key).length;
      }
      case "chain_verify": return { n_events: 214, n_bad: 0, problems: [], ok: true };
      case "plugin:dialog|open": case "plugin:dialog|save": return null;
      case "plugin:dialog|ask": return true;
      // `ask` und `confirm` schicken eine Meldung mit Knöpfen und vergleichen den gedrückten Knopf.
      case "plugin:dialog|message": {
        const buttons = a.buttons as string | Record<string, string | string[]> | undefined;
        if (buttons === "YesNo" || buttons === "YesNoCancel") return "Yes";
        if (buttons && typeof buttons === "object") {
          const custom = Object.values(buttons)[0];
          return Array.isArray(custom) ? custom[0] : custom;
        }
        return "Ok";
      }
      default: return null;
    }
  });
}
