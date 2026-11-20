// Beispieldaten für Formulare und eine entblindete Charting-Stufe in der Browser-Vorschau.
// Die Regeln (Abgleich, Pflichtfelder, Verblindung) liegen im Kern; hier steht nur genug, um die Seiten zu zeigen.

import { core } from "./core";
import { readMockSettings } from "./mockSettings";

type Value = string | number | string[] | null;

const field = (key: string, label: string, type: string, group: string, extra: Record<string, unknown> = {}) => ({
  key, label, description: "", type, required: false, group, analysis: "", ...extra,
});
const fields = [
  field("country", "Land", "text", "Quelle", { required: true, description: "Land, in dem die Arbeit durchgeführt wurde.", analysis: "Häufigkeit je Land" }),
  field("design", "Design", "choice", "Methodik", { required: true, options: ["RCT", "Kohortenstudie", "Qualitative Studie", "Leitlinie"], analysis: "Häufigkeit je Design" }),
  field("sample_size", "Stichprobengröße", "integer", "Methodik"),
  field("intervention", "Art der Intervention", "multi_choice", "PCC", { options: ["App", "Telemedizin", "Tagebuch", "Schulung"] }),
  field("key_findings", "Zentrale Ergebnisse", "text", "Ergebnisse", { required: true }),
  field("quote", "Belegzitat", "quote", "Ergebnisse"),
];
const version = (n: number, extra: Record<string, unknown> = {}) => ({
  version: n, name: "Charting-Formular", fields: n === 1 ? fields.slice(0, 5) : fields, change_note: "",
  published_at: `2026-11-1${n}T09:00:00Z`, confirmed: false, after_confirmation: false, ...extra,
});
let forms = [{ id: "f1", purpose: "charting", confirmed: false, versions: [version(1), version(2, { change_note: "Belegzitat ergänzt" })] }];

export const formStage = {
  id: "s4", review_id: "r1", name: "Charting-Pilot", kind: "charting_pilot", blind: true, min_reviewers: 2, maybe_as: null,
  scope: "sample", sample_size: 3, seed: 4711, n_records: 3, opened_at: "2026-11-18T09:00:00Z", unblinded: true, closed: false,
  blind_active: false, station: "charting_pilot", round: 1, decision_rule: "unanimous", conflict_rule: "discussion", verify: null,
  reason_required: false, criteria_version: "2", form_id: "f1", form_version: 2,
};

const sources = [
  { record_id: "e1", title: "App-based relaxation training for adults with migraine: a randomised trial", authors: "Hartmann, Eva; Lindqvist, Per", year: 2022, label: "Hartmann-Studie", source_type: "primary_study", n_records: 2 },
  { record_id: "e3", title: "Smartphone-based headache diary for adults with episodic migraine: a randomised trial", authors: "Keller, Jana; Moreau, Luc", year: 2023, label: "", source_type: "primary_study", n_records: 1 },
  { record_id: "e5", title: "Digital self-management of migraine: recommendations of a working group", authors: "Sauer, Timo", year: 2024, label: "", source_type: "guideline", n_records: 1 },
];
// Eigene Werte und die der zweiten Person; e3 weicht im Design ab, e5 ist noch offen.
const mine: Record<string, Record<string, Value>> = {
  e1: { country: "Schweden", design: "RCT", sample_size: 184, intervention: ["App", "Schulung"], key_findings: "Weniger Kopfschmerztage nach zwölf Wochen." },
  e3: { country: "Deutschland", design: "RCT", sample_size: 96, intervention: ["App", "Tagebuch"], key_findings: "Vollständigere Tagebücher als auf Papier." },
  e5: { country: "Deutschland" },
};
const other: Record<string, Record<string, Value>> = {
  e1: { ...mine.e1 },
  e3: { ...mine.e3, design: "Kohortenstudie" },
};
const completed = new Set(["e1", "e3"]);
// Letzter KI-Vorschlag je Quelle. Im Modell „fehler“ (Einstellungen) schlägt die Anfrage fehl.
const extractions: Record<string, { id: string; model: string; fulltext_sha256: string; form_version: number; truncated: boolean; fields: Record<string, { value: Value; quote: string; page: number | null }>; accepted: string[] | null; at: string }> = {};
const wait = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const consensus: Record<string, Record<string, Value>> = {};

const same = (x: Value, y: Value) => JSON.stringify(x ?? null) === JSON.stringify(y ?? null);
function compare(id: string) {
  const both = completed.has(id) && other[id] !== undefined;
  const cells = fields.map((f) => {
    const [a, b] = [mine[id]?.[f.key] ?? null, other[id]?.[f.key] ?? null];
    const values = both ? [{ actor_id: "a1", name: "Dorian Zwanzig", value: a }, { actor_id: "a2", name: "Mira Okafor", value: b }] : [];
    if (consensus[id]?.[f.key] !== undefined) return { field: f.key, status: "consensus", value: consensus[id][f.key], values };
    if (both && !same(a, b)) return { field: f.key, status: "differs", value: null, values };
    return { field: f.key, status: both && a !== null ? "agreed" : "empty", value: both ? a : null, values };
  });
  const status = !both ? "open" : cells.some((c) => c.status === "differs") ? "conflict" : "done";
  return { status, cells, n_completed: both ? 2 : 0, extractors: both ? ["a1", "a2"] : [] };
}
const mineOf = (id: string) => (completed.has(id) ? "completed" : Object.keys(mine[id] ?? {}).length ? "draft" : "open");
const record = (s: (typeof sources)[number]) => ({
  id: s.record_id, title: s.title, abstract: "Background: Digital tools may support self-management. Methods: Adults with migraine were followed for twelve weeks. Results: Headache days decreased.",
  authors: s.authors, year: s.year, journal: "Headache Care", doi: "", url: "", keywords: "", duplicate_of: null,
});

/** Antwort auf einen Formular-Befehl; `undefined`, wenn der Befehl nicht hierher gehört. */
export function mockForms(cmd: string, a: Record<string, unknown>, context: { members: unknown; sourceTypes: unknown; terms: unknown }): unknown {
  const id = String(a.recordId ?? "");
  const form = { id: "f1", purpose: "charting", name: "Charting-Formular", version: 2, fields, confirmed: forms[0]?.confirmed ?? false };
  switch (cmd) {
    case "forms":
      return { forms: a.reviewId === "r2" ? [] : forms, starters: a.reviewId === "r2" ? [{ purpose: "charting", name: core("Charting-Formular"), fields }] : [], term: core("Charting") };
    case "form_publish": {
      const data = a.data as { name: string; fields: typeof fields; change_note: string };
      const last = forms[0].versions[forms[0].versions.length - 1];
      forms = [{ ...forms[0], versions: [...forms[0].versions, version(last.version + 1, { name: data.name, fields: data.fields, change_note: data.change_note, after_confirmation: forms[0].confirmed })] }];
      return forms[0];
    }
    case "form_confirm": {
      const versions = forms[0].versions.map((v, i, all) => (i === all.length - 1 ? { ...v, confirmed: true } : v));
      forms = [{ ...forms[0], confirmed: true, versions }];
      return null;
    }
    case "form_stage": {
      const rows = sources.map((s) => {
        const compared = compare(s.record_id);
        return { ...s, mine: mineOf(s.record_id), status: compared.status, n_completed: compared.n_completed, cells: compared.cells };
      });
      const count = (status: string) => rows.filter((r) => r.status === status).length;
      return {
        stage: formStage, role: "owner", form, rows, mine_done: completed.size, todo: 0,
        counts: { open: count("open"), extracted: 0, conflict: count("conflict"), incomplete: 0, done: count("done") },
        times: [{ source_type: "primary_study", n: 4, seconds: 754 }], members: context.members, source_types: context.sourceTypes, terms: context.terms,
      };
    }
    case "form_entry": {
      const index = Math.max(0, sources.findIndex((s) => s.record_id === id));
      const s = sources[index];
      const open = sources.find((x, i) => i !== index && mineOf(x.record_id) !== "completed");
      return {
        stage: formStage, role: "owner", form, record: record(s),
        companions: s.n_records > 1 ? [{ ...record(s), id: "e2", title: "App-based relaxation training for adults with migraine: twelve-month follow-up", year: 2023 }] : [],
        fulltexts: s.record_id === "e5" ? [] : [s.record_id], label: s.label, source_type: s.source_type, mode: "extract",
        values: mine[s.record_id] ?? {}, completed: completed.has(s.record_id) ? { form_version: 2, seconds: 700, verify: false } : null,
        missing: [], compared: compare(s.record_id), ai: { allowed: true, suggestion: extractions[s.record_id] ?? null }, position: index + 1, total: sources.length,
        previous: sources[index - 1]?.record_id ?? null, next: sources[index + 1]?.record_id ?? null, next_open: open?.record_id ?? null,
        source_types: context.sourceTypes, terms: context.terms,
      };
    }
    case "form_value":
      mine[id] = { ...mine[id], [String(a.field)]: a.value as Value };
      return true;
    case "ai_extract": {
      const model = readMockSettings().settings.ai.model || "gpt-5-mini";
      const pages = (a.pages as string[]) ?? [];
      return wait(1500).then(() => {
        if (model === "fehler") return Promise.reject("Die KI weist den Zugang ab (401). Prüfe den API-Schlüssel in den Einstellungen.");
        extractions[id] = {
          id: `x-${id}-${Date.now()}`, model, fulltext_sha256: "ab12", form_version: 2, truncated: pages.join("").length > 400000, accepted: null, at: new Date().toISOString(),
          fields: {
            country: { value: "Deutschland", quote: "Participants were recruited at three headache centres in Germany.", page: 2 },
            design: { value: "RCT", quote: "We conducted a two-arm randomised controlled trial.", page: 2 },
            sample_size: { value: 98, quote: "A total of 98 adults were randomised.", page: 3 },
            intervention: { value: ["App", "Tagebuch"], quote: "The intervention was a smartphone diary app.", page: 2 },
            quote: { value: "Diary completeness was higher in the app group.", quote: "Diary completeness was higher in the app group.", page: 5 },
          },
        };
        return extractions[id];
      });
    }
    case "ai_extract_apply": {
      const found = extractions[id];
      const keys = (a.accepted as string[]) ?? [];
      if (!found) return Promise.reject("KI-Vorschlag nicht gefunden.");
      for (const key of keys) mine[id] = { ...mine[id], [key]: found.fields[key].value };
      extractions[id] = { ...found, accepted: keys };
      return keys.length;
    }
    case "form_complete":
      completed.add(id);
      return null;
    case "form_consensus":
      consensus[id] = { ...consensus[id], [String(a.field)]: a.value as Value };
      return null;
    case "form_frequencies":
      return fields.filter((f) => f.type === "choice" || f.type === "multi_choice").map((f) => {
        const cells = sources.map((s) => compare(s.record_id).cells.find((c) => c.field === f.key)!);
        const chosen = cells.flatMap((c) => (Array.isArray(c.value) ? c.value : c.value === null ? [] : [String(c.value)]));
        const options = (f as { options?: string[] }).options ?? [];
        return {
          field: f.key, label: f.label, counts: options.map((option) => ({ option, n: chosen.filter((c) => c === option).length })),
          n_empty: cells.filter((c) => c.status !== "differs" && c.value === null).length, n_open: cells.filter((c) => c.status === "differs").length,
        };
      });
    case "form_export":
      return null;
    default:
      return undefined;
  }
}
