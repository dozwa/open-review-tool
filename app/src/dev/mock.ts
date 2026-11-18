// Nur für die Vorschau im Browser während der Entwicklung (`npm run dev`, dann http://127.0.0.1:5173).
// Ersetzt die Rust-Befehle durch Beispieldaten, damit sich die Gestaltung ohne App ansehen lässt.
// Wird in der App und im Release-Build nie geladen. Mit `?neu` startet die Vorschau beim Erststart.

import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

const me = { actor_id: "a1b2c3d4-0000-4000-8000-000000000001", name: "Dorian Zwanzig", public_key: "x" };
const criteria = {
  version: 2,
  previous_version: 1,
  text: "Eingeschlossen werden Originalstudien zu digitalen Interventionen bei Menschen mit Migräne (alle Altersgruppen).\nAusgeschlossen: andere Kopfschmerzarten, Editorials, Protokolle ohne Daten.",
  include_keywords: ["migraine", "smartphone", "app", "telemedicine"],
  exclude_keywords: ["editorial", "protocol", "tension-type"],
  exclusion_reasons: ["Falsche Population", "Keine digitale Intervention", "Kein Originalbeitrag", "Protokoll ohne Daten"],
  change_note: "Qualitative Studien ausdrücklich eingeschlossen",
};
const stage = (id: string, name: string, kind: string, extra: Record<string, unknown>) => ({
  id, review_id: "r1", name, kind, blind: true, min_reviewers: 2, maybe_as: null, scope: "all",
  sample_size: null, seed: null, n_records: 38, opened_at: "2026-11-12T09:00:00Z",
  unblinded: false, closed: false, blind_active: true, ...extra,
});
const stages = [
  stage("s1", "Pilot Titel/Abstract", "pilot", { n_records: 10, seed: 42, unblinded: true, closed: true, blind_active: false, maybe_as: "include" }),
  stage("s2", "Titel/Abstract", "title_abstract", { unblinded: true, blind_active: false }),
  stage("s3", "Volltext", "fulltext", { n_records: 12 }),
];
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
];
let screened = 0;

export function installMock() {
  mockWindows("main");
  let identity: typeof me | null = new URLSearchParams(location.search).has("neu") ? null : me;
  mockIPC((cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "identity_get": return identity;
      case "identity_create": identity = { ...me, name: String(a.name) }; return identity;
      case "reviews_list": return [
        { id: "r1", title: "Digitale Interventionen bei Migräne", role: "owner" },
        { id: "r2", title: "KI-gestützte Triage in der Notaufnahme", role: "admin" },
      ];
      case "review_detail": return {
        id: a.reviewId, title: a.reviewId === "r2" ? "KI-gestützte Triage in der Notaufnahme" : "Digitale Interventionen bei Migräne",
        role: "owner", me: me.actor_id, members, n_records: 40, n_duplicates: 2, criteria,
        stages: a.reviewId === "r2" ? [] : stages,
        imports: [
          { id: "i1", filename: "pubmed_2026-11-02.ris", format: "ris", source: "PubMed", status: "done", n_records: 35, n_duplicates: 0, error: "", created_at: "" },
          { id: "i2", filename: "scopus_2026-11-02.bib", format: "bibtex", source: "Scopus", status: "done", n_records: 5, n_duplicates: 2, error: "", created_at: "" },
        ],
      };
      case "stage_detail": {
        const s = stages.find((x) => x.id === a.stageId) ?? stages[1];
        return { stage: s, role: "owner", progress: { done: 31 + screened, total: s.n_records }, criteria };
      }
      case "stage_next": return { record: records[screened % records.length], progress: { done: 31 + screened, total: 38 } };
      case "vote_cast": case "record_flag": screened += 1; return null;
      case "stage_summary": {
        const s = stages.find((x) => x.id === a.stageId) ?? stages[1];
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
          { name: "Anna Berger", decision: "include", reasons: [], note: "" },
          { name: "Ben Krüger", decision: "exclude", reasons: ["Falsche Population"], note: "Nur Jugendliche?" },
        ] },
        { record: { ...records[2], abstract: "" }, status: "needs_clarification", votes: [
          { name: "Clara Weiß", decision: "maybe", reasons: [], note: "Kein Abstract vorhanden, Volltext nötig." },
        ] },
      ];
      case "chain_verify": return { n_events: 214, n_bad: 0, problems: [], ok: true };
      case "plugin:dialog|open": case "plugin:dialog|save": return null;
      case "plugin:dialog|ask": case "plugin:dialog|message": return true;
      default: return null;
    }
  });
}
