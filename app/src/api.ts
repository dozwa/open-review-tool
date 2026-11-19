// Aufrufe in den Rust-Kern. Rechte, Verblindung und Prüfungen liegen dort, nicht hier.

import { invoke } from "@tauri-apps/api/core";

function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  return invoke<T>(command, args);
}

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  return error instanceof Error ? error.message : "Unbekannter Fehler";
}

export interface Person {
  actor_id: string;
  name: string;
  public_key: string;
}

export interface ReviewListItem {
  id: string;
  title: string;
  role: string | null;
}

/** Ein Kriterium. Das Kürzel eines Ausschlusskriteriums ist zugleich der Ausschlussgrund. */
export interface Criterion {
  code: string;
  element: string;
  kind: "include" | "exclude";
  text: string;
  note: string;
}

export interface Criteria {
  version: number;
  previous_version: number | null;
  items: Criterion[];
  guidance: string;
  include_keywords: string[];
  exclude_keywords: string[];
  change_note: string;
}

/** Element des Kriterienschemas der Vorlage, z. B. Population. */
export interface SchemaElement {
  key: string;
  label: string;
}

/** Ausschlussgründe einer Kriterienversion: Ausschlusskriterien mit Kürzel. */
export function reasonsOf(criteria: Criteria): Criterion[] {
  return criteria.items.filter((c) => c.kind === "exclude" && c.code);
}

export interface Stage {
  id: string;
  review_id: string;
  name: string;
  kind: string;
  blind: boolean;
  min_reviewers: number;
  maybe_as: string | null;
  scope: string | null;
  sample_size: number | null;
  seed: number | null;
  n_records: number;
  opened_at: string;
  unblinded: boolean;
  closed: boolean;
  blind_active: boolean;
  station: string | null;
  round: number;
  decision_rule: string;
  conflict_rule: string;
  verify: Verify | null;
  reason_required: boolean;
  criteria_version: string | null;
  /** Formular einer Formular-Stufe; Screening-Stufen haben keines. */
  form_id: string | null;
  form_version: number | null;
}

/** Zweite Stimme für Ausschlüsse bei der Regel „eine Stimme mit Prüfung“. */
export interface Verify {
  scope: "all" | "sample";
  percent?: number | null;
  seed?: number | null;
}

/** Eine Regel der Vorlage und ob sie beim Öffnen einer Stufe geändert werden darf. */
export interface Rule<T> {
  value: T;
  editable: boolean;
}

export interface Rules {
  decision_rule: Rule<string>;
  min_reviewers: Rule<number>;
  maybe_as: Rule<string | null>;
  conflict_rule: Rule<string>;
  blind: Rule<boolean>;
}

export interface GateCheckResult {
  check: string;
  passed: boolean;
  numerator: number;
  denominator: number;
  threshold_percent?: number | null;
  percent_x100?: number | null;
}

export interface GateResult {
  passed: boolean;
  checks: GateCheckResult[];
}

export interface StationCheck {
  check: string;
  label: string;
  passed: boolean | null;
  detail: string;
}

export interface Round {
  round: number;
  stage_id: string;
  name: string;
  closed: boolean;
  gate: GateResult | null;
}

export interface StageDefaults {
  name: string;
  round: number;
  scope: string;
  sample_size: number | null;
  source_stage: string | null;
  rules: Rules;
  reason_required: boolean;
}

export interface Station {
  key: string;
  kind: "setup" | "screening" | "form" | "report";
  label: string;
  description: string;
  tool: string | null;
  state: "locked" | "open" | "running" | "done";
  available: boolean;
  blocking: boolean;
  blocked_by: { key: string; label: string }[];
  checks: StationCheck[];
  rounds: Round[];
  overridden: string | null;
  free_rounds: boolean;
  defaults: StageDefaults | null;
}

export interface Workflow {
  published: boolean;
  version: number;
  template_key: string;
  template_name: string;
  review_type: string;
  terms: Record<string, string>;
  criteria_schema: SchemaElement[];
  stations: Station[];
}

export interface TemplateInfo {
  key: string;
  name: string;
  review_type: string;
  description: string;
  stations: string[];
}

export interface ImportBatch {
  id: string;
  filename: string;
  format: string;
  source: string;
  status: string;
  n_records: number;
  n_duplicates: number;
  error: string;
  created_at: string;
  /** Leer bei Importen aus der Zeit vor der Angabe. */
  origin: string;
  search_id: string | null;
  search_version: number | null;
}

export const ORIGIN_LABEL: Record<string, string> = {
  database: "Datenbank",
  register: "Register",
  citation: "Zitationssuche",
  other: "Andere Quelle",
};

/** Woher die Treffer eines Imports stammen: eine Suche oder Quelle mit Herkunft. */
export interface ImportSource {
  source?: string;
  origin?: string;
  search_id?: string | null;
  search_version?: number | null;
}

export interface PeerReview {
  reviewed_on: string;
  note: string;
  reviewer: string | null;
  recorded_by: string;
}

export interface SearchVersion {
  version: number;
  previous_version: number | null;
  source: string;
  platform: string;
  origin: string;
  query: string;
  searched_on: string;
  limits: string;
  hit_limit: number | null;
  hits: number | null;
  change_note: string;
  published_at: string;
  peer_reviews: PeerReview[];
  n_imports: number;
  n_records: number;
  n_duplicates: number;
}

export interface Search {
  id: string;
  versions: SearchVersion[];
}

export interface DuplicatePair {
  a: RecordItem;
  b: RecordItem;
  a_source: string;
  b_source: string;
  similarity: number;
  /** Vorschlag, welcher Eintrag bleibt. */
  keep: string;
}

export interface DuplicateList {
  pairs: DuplicatePair[];
  total: number;
  n_exact: number;
  n_resolved: number;
  n_distinct: number;
}

export interface PairDecision {
  a: string;
  b: string;
  decision: "duplicate" | "distinct";
  keep?: string | null;
}

export interface Member {
  actor_id: string;
  name: string;
  role: string;
  is_me: boolean;
}

export interface ReviewDetail {
  id: string;
  title: string;
  role: string;
  me: string;
  members: Member[];
  n_records: number;
  n_duplicates: number;
  criteria: Criteria;
  stages: Stage[];
  imports: ImportBatch[];
  workflow: Workflow;
}

export interface RecordItem {
  id: string;
  title: string;
  abstract: string;
  authors: string;
  year: number | null;
  journal: string;
  doi: string;
  url: string;
  keywords: string;
  duplicate_of: string | null;
}

export interface Progress {
  done: number;
  total: number;
}

export interface StageDetail {
  stage: Stage;
  role: string;
  progress: Progress;
  /** Kriterien, mit denen die Stufe geöffnet wurde. */
  criteria: Criteria;
  newer_criteria: boolean;
  criteria_schema: SchemaElement[];
  members: Member[];
  terms: Record<string, string>;
}

export interface Vote {
  name: string;
  decision: string;
  primary_reason: string | null;
  reasons: string[];
  note: string;
}

/** Filter der Liste auf der Screening-Seite. Gesperrte Filter weist der Kern ab. */
export interface RecordFilter {
  decision?: string | null;
  person?: string | null;
  votes_min?: number | null;
  votes_max?: number | null;
  keyword?: string | null;
  query?: string | null;
  offset?: number;
  limit?: number | null;
}

export interface ListRow {
  id: string;
  title: string;
  year: number | null;
  my_decision: string | null;
  n_votes: number | null;
  status: string | null;
  decision: string | null;
}

export interface RecordList {
  items: ListRow[];
  total: number;
  keywords: { keyword: string; kind: "include" | "exclude"; n: number }[];
  locked: string[];
}

export interface MyVote {
  decision: string;
  primary_reason: string | null;
  reasons: string[];
  note: string;
}

export interface RecordDetail {
  record: RecordItem;
  my_vote: MyVote | null;
  fulltext: FullText | null;
}

/** Stand eines Volltexts. Die Datei liegt im verknüpften Ordner oder in Zotero, nie in der App. */
export interface FullText {
  status: "retrieved" | "unavailable";
  origin: "folder" | "zotero" | "";
  path: string;
  zotero_key: string;
  sha256: string;
  size: number;
  reason: string;
  set_by: string;
  set_at: string;
}

export interface ZoteroLink {
  library_type: "user" | "group";
  library_id: number;
  collection_key: string | null;
  name: string;
}

export interface ZoteroLibrary {
  library_type: "user" | "group";
  library_id: number;
  name: string;
  collections: { key: string; name: string }[];
}

export interface FullTextRow {
  record_id: string;
  title: string;
  authors: string;
  year: number | null;
  doi: string;
  needed: boolean;
  fulltext: FullText | null;
  file: "ok" | "missing" | "";
  zotero_item: string | null;
}

export interface FullTextOverview {
  folder: string | null;
  folder_found: boolean;
  zotero: ZoteroLink | null;
  needed: number;
  retrieved: number;
  unavailable: number;
  rows: FullTextRow[];
}

/** Vorschlag, eine Datei aus dem Ordner einem Eintrag zuzuordnen. */
export interface FileSuggestion {
  record_id: string;
  title: string;
  authors: string;
  year: number | null;
  path: string;
  size: number;
  basis: "doi_metadata" | "doi_filename" | "title" | "author_year";
  similarity: number;
}

export interface ZoteroSuggestion {
  record_id: string;
  title: string;
  authors: string;
  year: number | null;
  item_key: string;
  item_title: string;
  attachment_key: string | null;
  basis: "doi" | "title";
  similarity: number;
}

export interface FileProblem {
  record_id: string;
  title: string;
  path: string;
  problem: "missing" | "changed";
}

export interface Conflict {
  record: RecordItem;
  status: string;
  votes: Vote[];
}

export interface CohenPair {
  rater_a: string;
  rater_b: string;
  n_items: number;
  p_o: number | null;
  p_e: number | null;
  kappa: number | null;
  pabak: number | null;
}

export interface Estimate {
  value: number | null;
  ci_low: number | null;
  ci_high: number | null;
  level: number;
  n_items: number;
  n_boot: number;
  seed: number;
}

export interface Summary {
  stage: Stage;
  progress: Progress;
  reviewers: { name: string; n_votes: number }[] | null;
  decisions: {
    include: number;
    exclude: number;
    maybe: number;
    disagreement: number;
    needs_clarification: number;
    pending: number;
  } | null;
  agreement: {
    percent: { n_items: number; percent_agreement: number | null };
    percent_ci: Estimate;
    krippendorff: { n_items: number; alpha: number | null };
    alpha_ci: Estimate;
    cohen_pairs: CohenPair[];
  } | null;
}

export interface Snapshot {
  label: string;
  hash: string;
  n_heads: number;
  gate: GateResult | null;
}

export interface ChainReport {
  n_events: number;
  n_bad: number;
  problems: string[];
  ok: boolean;
}

export interface MergeReport {
  accepted: number;
  already_known: number;
  rejected: string[];
  records_added: number;
}

export interface BundleImported {
  review_id: string;
  title: string;
  created: boolean;
  report: MergeReport;
}

export interface StageParams {
  name: string;
  kind: string;
  blind: boolean;
  min_reviewers: number;
  maybe_as: string | null;
  scope: string;
  sample_size: number | null;
  seed: number | null;
  source_stage: string | null;
  station: string | null;
  decision_rule: string | null;
  conflict_rule: string | null;
  verify: Verify | null;
  reason_required: boolean | null;
  form_id?: string | null;
}

export const RULE_LABEL: Record<string, string> = {
  unanimous: "Einstimmig",
  liberal: "Eine Einschluss-Stimme genügt",
  single_verified: "Eine Stimme mit Prüfung der Ausschlüsse",
};
export const CONFLICT_LABEL: Record<string, string> = {
  discussion: "Diskussion",
  third_person: "Entscheid durch eine dritte Person",
};

/** Begriff aus der Vorlage, z. B. „Einträge“ oder „Quellen“. */
export function term(terms: Record<string, string> | undefined, key: string, fallback: string): string {
  return terms?.[key] || fallback;
}

/** Ergebnis eines Gates in einem Satz je Prüfung. */
export function gateText(gate: GateResult): string {
  return gate.checks
    .map((c) => {
      if (c.check === "agreement_min") {
        const percent = c.percent_x100 == null ? "keine doppelt bewerteten Einträge" : `${(c.percent_x100 / 100).toLocaleString("de-DE", { maximumFractionDigits: 1 })} %`;
        return `Übereinstimmung ${percent}, verlangt sind ${c.threshold_percent} %`;
      }
      if (c.check === "all_decided") return `${c.numerator} von ${c.denominator} entschieden`;
      return `${c.numerator} offene Konflikte`;
    })
    .join("; ");
}

export const canManage = (role: string) => role === "owner" || role === "admin";
export const canVote = (role: string) => role === "owner" || role === "admin" || role === "reviewer";

/** Grund eines Ausschlusses mit Zahl; ohne Kürzel heißt: kein Hauptgrund genannt. */
export interface ReasonCount {
  code: string;
  text: string;
  n: number;
}

/** Eine Ebene des Flussdiagramms: eine Screening-Station mit allen Runden. */
export interface FlowLevel {
  key: string;
  label: string;
  rounds: number;
  input_from: string | null;
  input: number;
  retrieval: { sought: number; unavailable: number; open: number; waiting: number } | null;
  not_assigned: number;
  assessed: number;
  included: number;
  excluded: number;
  open: number;
  blinded: number;
  reasons: ReasonCount[];
  fulltext: boolean;
  /** Von den Geprüften und Eingeschlossenen: Einträge aus Zitationssuche und anderen Wegen. */
  other_assessed: number;
  other_included: number;
}

export interface TypeCount {
  /** Schlüssel der Quellenart; leer für „ohne Quellenart“. */
  key: string;
  label: string;
  n: number;
}

export interface SourceRecord extends RecordItem {
  primary: boolean;
  /** Ob der Eintrag am Ende der Auswahl eingeschlossen ist. */
  included: boolean;
}

/** Eine Quelle: ein einzelner Eintrag oder ein Bündel mit Haupteintrag. */
export interface SourceView {
  /** ID des Haupteintrags. */
  id: string;
  study_id: string | null;
  label: string;
  source_type: string;
  records: SourceRecord[];
}

export interface SourceList {
  sources: SourceView[];
  /** Quellenarten der Vorlage; ohne Liste ist die Quellenart freier Text. */
  source_types: SchemaElement[];
  n_records: number;
  n_sources: number;
  n_without_type: number;
  by_type: TypeCount[];
  level: string | null;
  complete: boolean;
}

export interface Flow {
  sources: { source: string; origin: string; n_records: number; n_duplicates: number }[];
  identified: number;
  exact_duplicates: number;
  resolved_duplicates: number;
  unique: number;
  /** Von `identified`: Treffer aus Zitationssuche und anderen Wegen. */
  other_identified: number;
  levels: FlowLevel[];
  included: number | null;
  /** Dasselbe nach dem Bündeln. */
  included_sources: number | null;
  source_types: TypeCount[];
  complete: boolean;
  record_term: string;
  records_term: string;
  source_term: string;
  sources_term: string;
}

export interface Report {
  flow: Flow;
  /** Stellen, an denen das Diagramm nicht aufgeht; im Normalfall leer. */
  problems: string[];
  svg: string;
  methods: string;
  search_appendix: string;
  n_excluded_rows: number;
  protocol: Protocol;
  /** Fehlt, wenn die Vorlage keine Checkliste nennt. */
  checklist: Checklist | null;
  table: { n_sources: number; n_charted: number; n_fields: number };
}

export interface Registration {
  /** JJJJ-MM-TT */
  registered_on: string;
  registry: string;
  identifier: string;
  note: string;
  /** Erster Eintrag im Review; ab hier zählen neue Versionen als Abweichung. */
  recorded_at: string;
  corrected: boolean;
}

export interface Deviation {
  kind: "criteria" | "search" | "form" | "workflow" | "gate";
  at: string;
  title: string;
  note: string;
}

export interface Protocol {
  registration: Registration | null;
  deviations: Deviation[];
}

export interface ChecklistItem {
  number: number;
  section: string;
  topic: string;
  status: "ready" | "open" | "manual" | "optional";
  material: string;
}

export interface Checklist {
  name: string;
  items: ChecklistItem[];
}

// ---------------------------------------------------------------- Formulare

export type FieldType = "text" | "integer" | "decimal" | "choice" | "multi_choice" | "quote";
export type FormValue = string | number | string[] | null;

export const FIELD_TYPE_LABEL: Record<FieldType, string> = {
  text: "Text",
  integer: "Ganze Zahl",
  decimal: "Dezimalzahl",
  choice: "Auswahl",
  multi_choice: "Mehrfachauswahl",
  quote: "Zitat",
};

export interface FormField {
  key: string;
  label: string;
  description: string;
  type: FieldType;
  required: boolean;
  group: string;
  analysis: string;
  options?: string[];
}

export interface FormVersion {
  version: number;
  name: string;
  fields: FormField[];
  change_note: string;
  published_at: string;
  confirmed: boolean;
  after_confirmation: boolean;
}

export interface Form {
  id: string;
  purpose: string;
  versions: FormVersion[];
  confirmed: boolean;
}

export interface FormStarter {
  purpose: string;
  name: string;
  fields: FormField[];
}

export interface FormsView {
  forms: Form[];
  starters: FormStarter[];
  term: string;
}

export interface FormRef {
  id: string;
  purpose: string;
  name: string;
  version: number;
  fields: FormField[];
  confirmed: boolean;
}

export interface FormCell {
  field: string;
  status: "agreed" | "consensus" | "differs" | "empty" | "mine";
  value: FormValue;
  values: { actor_id: string; name: string; value: FormValue }[];
}

export type EntryStatus = "open" | "extracted" | "conflict" | "incomplete" | "done";
export type MineStatus = "open" | "draft" | "completed" | "todo" | "to_verify" | "locked";

export interface FormRow {
  record_id: string;
  title: string;
  authors: string;
  year: number | null;
  label: string;
  source_type: string;
  n_records: number;
  mine: MineStatus;
  status: EntryStatus | null;
  n_completed: number | null;
  cells: FormCell[];
}

export interface FormStageView {
  stage: Stage;
  role: string;
  form: FormRef;
  rows: FormRow[];
  mine_done: number;
  todo: number;
  counts: Record<EntryStatus, number> | null;
  times: { source_type: string; n: number; seconds: number }[];
  members: Member[];
  source_types: SchemaElement[];
  terms: Record<string, string>;
}

export interface FormCompared {
  status: EntryStatus;
  cells: FormCell[];
  n_completed: number;
  extractors: string[];
}

export interface FormEntryView {
  stage: Stage;
  role: string;
  form: FormRef;
  record: RecordItem;
  companions: RecordItem[];
  fulltexts: string[];
  label: string;
  source_type: string;
  mode: "extract" | "verify" | "read";
  values: Record<string, FormValue>;
  completed: { form_version: number; seconds: number | null; verify: boolean } | null;
  missing: string[];
  compared: FormCompared | null;
  position: number;
  total: number;
  previous: string | null;
  next: string | null;
  next_open: string | null;
  source_types: SchemaElement[];
  terms: Record<string, string>;
}

export interface Frequency {
  field: string;
  label: string;
  counts: { option: string; n: number }[];
  n_empty: number;
  n_open: number;
}

/** Wert eines Formularfeldes als Text. */
export function showValue(value: FormValue | undefined): string {
  if (value === null || value === undefined) return "";
  return Array.isArray(value) ? value.join("; ") : String(value);
}

export type ReportFileKind =
  | "diagram" | "excluded" | "included" | "sources" | "search" | "methods" | "table" | "deviations" | "checklist";

export const api = {
  identity: () => call<Person | null>("identity_get"),
  createIdentity: (name: string) => call<Person>("identity_create", { name }),
  exportIdentity: (path: string) => call<void>("identity_export", { path }),

  reviews: () => call<ReviewListItem[]>("reviews_list"),
  templates: () => call<TemplateInfo[]>("templates_list"),
  createReview: (title: string, template: string | null) => call<ReviewListItem>("review_create", { title, template }),
  review: (reviewId: string) => call<ReviewDetail>("review_detail", { reviewId }),
  addMember: (reviewId: string, path: string, role: string) => call<string>("member_add", { reviewId, path, role }),
  setRole: (reviewId: string, actorId: string, role: string) =>
    call<void>("member_set_role", { reviewId, actorId, role }),
  publishCriteria: (reviewId: string, data: Record<string, unknown>) =>
    call<Criteria>("criteria_publish", { reviewId, data }),
  importRecords: (reviewId: string, path: string, from: ImportSource) =>
    call<ImportBatch>("records_import", { reviewId, path, from }),
  searches: (reviewId: string) => call<Search[]>("searches", { reviewId }),
  publishSearch: (reviewId: string, data: Record<string, unknown>) =>
    call<Search>("search_publish", { reviewId, data }),
  reviewSearch: (reviewId: string, searchId: string, version: number, reviewedOn: string, note: string, reviewer: string | null) =>
    call<void>("search_review", { reviewId, searchId, version, reviewedOn, note, reviewer }),
  duplicates: (reviewId: string) => call<DuplicateList>("duplicates_list", { reviewId }),
  resolveDuplicates: (reviewId: string, pairs: PairDecision[]) =>
    call<number>("duplicates_resolve", { reviewId, pairs }),
  verifyChain: (reviewId: string) => call<ChainReport>("chain_verify", { reviewId }),
  sources: (reviewId: string) => call<SourceList>("sources", { reviewId }),
  linkStudy: (reviewId: string, studyId: string | null, label: string, records: string[], primary: string) =>
    call<string>("study_link", { reviewId, studyId, label, records, primary }),
  dissolveStudy: (reviewId: string, studyId: string) => call<void>("study_dissolve", { reviewId, studyId }),
  setSourceType: (reviewId: string, records: string[], sourceType: string) =>
    call<number>("source_type_set", { reviewId, records, sourceType }),
  report: (reviewId: string) => call<Report>("report", { reviewId }),
  registerProtocol: (reviewId: string, registeredOn: string, registry: string, identifier: string, note: string) =>
    call<Protocol>("protocol_register", { reviewId, registeredOn, registry, identifier, note }),
  exportReport: (reviewId: string, kind: ReportFileKind, path: string) =>
    call<void>("report_export", { reviewId, kind, path }),

  fulltexts: (reviewId: string) => call<FullTextOverview>("fulltext_overview", { reviewId }),
  setFulltextFolder: (reviewId: string, path: string | null) => call<void>("fulltext_folder_set", { reviewId, path }),
  fulltextSuggestions: (reviewId: string) => call<FileSuggestion[]>("fulltext_suggestions", { reviewId }),
  assignFulltexts: (reviewId: string, list: { record_id: string; path: string }[]) =>
    call<number>("fulltext_assign", { reviewId, list }),
  fulltextUnavailable: (reviewId: string, recordId: string, reason: string) =>
    call<void>("fulltext_unavailable", { reviewId, recordId, reason }),
  resetFulltext: (reviewId: string, recordId: string) => call<void>("fulltext_reset", { reviewId, recordId }),
  verifyFulltexts: (reviewId: string) => call<FileProblem[]>("fulltext_verify", { reviewId }),
  /** Inhalt der PDF für die Vorschau; der Kern hat Pfad und Prüfsumme geprüft. */
  readFulltext: (reviewId: string, recordId: string) => call<ArrayBuffer>("fulltext_read", { reviewId, recordId }),
  openFulltext: (reviewId: string, recordId: string) => call<void>("fulltext_open_external", { reviewId, recordId }),
  zoteroLibraries: () => call<ZoteroLibrary[]>("zotero_libraries"),
  linkZotero: (reviewId: string, link: ZoteroLink) => call<void>("zotero_link", { reviewId, link }),
  zoteroSuggestions: (reviewId: string) => call<ZoteroSuggestion[]>("zotero_suggestions", { reviewId }),
  confirmZotero: (reviewId: string, list: { record_id: string; item_key: string; attachment_key: string | null }[]) =>
    call<number>("zotero_confirm", { reviewId, list }),

  openStage: (reviewId: string, params: StageParams) => call<Stage>("stage_open", { reviewId, params }),
  stage: (reviewId: string, stageId: string) => call<StageDetail>("stage_detail", { reviewId, stageId }),
  next: (reviewId: string, stageId: string) =>
    call<{ record: RecordItem | null; progress: Progress }>("stage_next", { reviewId, stageId }),
  records: (reviewId: string, stageId: string, filter: RecordFilter) =>
    call<RecordList>("stage_records", { reviewId, stageId, filter }),
  record: (reviewId: string, stageId: string, recordId: string) =>
    call<RecordDetail>("stage_record", { reviewId, stageId, recordId }),
  vote: (
    reviewId: string,
    stageId: string,
    recordId: string,
    decision: string,
    primaryReason: string | null,
    reasons: string[],
    note: string,
  ) => call<void>("vote_cast", { reviewId, stageId, recordId, decision, primaryReason, reasons, note }),
  flag: (reviewId: string, stageId: string, recordId: string, note: string) =>
    call<void>("record_flag", { reviewId, stageId, recordId, note }),
  consensus: (reviewId: string, stageId: string, recordId: string, decision: string, primaryReason: string | null) =>
    call<void>("consensus_record", { reviewId, stageId, recordId, decision, primaryReason }),
  unblind: (reviewId: string, stageId: string) => call<void>("stage_unblind", { reviewId, stageId }),
  close: (reviewId: string, stageId: string) => call<Snapshot>("stage_close", { reviewId, stageId }),
  gate: (reviewId: string, stageId: string) => call<GateResult | null>("stage_gate", { reviewId, stageId }),
  overrideGate: (reviewId: string, station: string, reason: string) =>
    call<void>("gate_override", { reviewId, station, reason }),
  conflicts: (reviewId: string, stageId: string) => call<Conflict[]>("stage_conflicts", { reviewId, stageId }),
  summary: (reviewId: string, stageId: string) => call<Summary>("stage_summary", { reviewId, stageId }),
  exportCsv: (reviewId: string, stageId: string, path: string) =>
    call<void>("stage_export_csv", { reviewId, stageId, path }),

  forms: (reviewId: string) => call<FormsView>("forms", { reviewId }),
  publishForm: (reviewId: string, data: Record<string, unknown>) => call<Form>("form_publish", { reviewId, data }),
  confirmForm: (reviewId: string, formId: string) => call<void>("form_confirm", { reviewId, formId }),
  formStage: (reviewId: string, stageId: string) => call<FormStageView>("form_stage", { reviewId, stageId }),
  formEntry: (reviewId: string, stageId: string, recordId: string) =>
    call<FormEntryView>("form_entry", { reviewId, stageId, recordId }),
  formValue: (reviewId: string, stageId: string, recordId: string, field: string, value: FormValue) =>
    call<boolean>("form_value", { reviewId, stageId, recordId, field, value }),
  completeEntry: (reviewId: string, stageId: string, recordId: string, seconds: number | null) =>
    call<void>("form_complete", { reviewId, stageId, recordId, seconds }),
  formConsensus: (reviewId: string, stageId: string, recordId: string, field: string, value: FormValue, note: string) =>
    call<void>("form_consensus", { reviewId, stageId, recordId, field, value, note }),
  formFrequencies: (reviewId: string, stageId: string) => call<Frequency[]>("form_frequencies", { reviewId, stageId }),
  exportForm: (reviewId: string, stageId: string, kind: "table" | "frequencies", path: string) =>
    call<void>("form_export", { reviewId, stageId, kind, path }),

  exportPackage: (reviewId: string, actorId: string, path: string) =>
    call<void>("package_export", { reviewId, actorId, path }),
  exportReturn: (reviewId: string, path: string) => call<number>("return_export", { reviewId, path }),
  importBundle: (path: string) => call<BundleImported>("bundle_import", { path }),
  openUrl: (url: string) => call<void>("open_url", { url }),
};

export const BUNDLE_FILTER = [{ name: "Paket oder Rückmeldung", extensions: ["ortpkg", "ortvotes"] }];

/** Dateiname ohne Zeichen, die macOS oder andere Systeme stören. */
export function safeFileName(text: string): string {
  return text.replace(/[\\/:*?"<>|]+/g, "-").replace(/\s+/g, " ").trim().slice(0, 80) || "review";
}

export function importSummary(result: BundleImported): string {
  const r = result.report;
  const parts = [`${r.accepted} neue Einträge`];
  if (r.already_known) parts.push(`${r.already_known} schon vorhanden`);
  if (r.records_added) parts.push(`${r.records_added} Records`);
  if (r.rejected.length) parts.push(`${r.rejected.length} abgewiesen`);
  return `${result.created ? "Review angelegt" : "Eingelesen"}: ${result.title} (${parts.join(", ")}).`;
}
