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

export interface Criteria {
  version: number;
  previous_version: number | null;
  text: string;
  include_keywords: string[];
  exclude_keywords: string[];
  exclusion_reasons: string[];
  change_note: string;
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
  criteria: Criteria;
}

export interface Vote {
  name: string;
  decision: string;
  reasons: string[];
  note: string;
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
}

export const canManage = (role: string) => role === "owner" || role === "admin";
export const canVote = (role: string) => role === "owner" || role === "admin" || role === "reviewer";

export const api = {
  identity: () => call<Person | null>("identity_get"),
  createIdentity: (name: string) => call<Person>("identity_create", { name }),
  exportIdentity: (path: string) => call<void>("identity_export", { path }),

  reviews: () => call<ReviewListItem[]>("reviews_list"),
  createReview: (title: string) => call<ReviewListItem>("review_create", { title }),
  review: (reviewId: string) => call<ReviewDetail>("review_detail", { reviewId }),
  addMember: (reviewId: string, path: string, role: string) => call<string>("member_add", { reviewId, path, role }),
  setRole: (reviewId: string, actorId: string, role: string) =>
    call<void>("member_set_role", { reviewId, actorId, role }),
  publishCriteria: (reviewId: string, data: Record<string, unknown>) =>
    call<Criteria>("criteria_publish", { reviewId, data }),
  importRecords: (reviewId: string, path: string, source: string) =>
    call<ImportBatch>("records_import", { reviewId, path, source }),
  verifyChain: (reviewId: string) => call<ChainReport>("chain_verify", { reviewId }),

  openStage: (reviewId: string, params: StageParams) => call<Stage>("stage_open", { reviewId, params }),
  stage: (reviewId: string, stageId: string) => call<StageDetail>("stage_detail", { reviewId, stageId }),
  next: (reviewId: string, stageId: string) =>
    call<{ record: RecordItem | null; progress: Progress }>("stage_next", { reviewId, stageId }),
  vote: (reviewId: string, stageId: string, recordId: string, decision: string, reasons: string[], note: string) =>
    call<void>("vote_cast", { reviewId, stageId, recordId, decision, reasons, note }),
  flag: (reviewId: string, stageId: string, recordId: string, note: string) =>
    call<void>("record_flag", { reviewId, stageId, recordId, note }),
  consensus: (reviewId: string, stageId: string, recordId: string, decision: string) =>
    call<void>("consensus_record", { reviewId, stageId, recordId, decision }),
  unblind: (reviewId: string, stageId: string) => call<void>("stage_unblind", { reviewId, stageId }),
  close: (reviewId: string, stageId: string) => call<Snapshot>("stage_close", { reviewId, stageId }),
  conflicts: (reviewId: string, stageId: string) => call<Conflict[]>("stage_conflicts", { reviewId, stageId }),
  summary: (reviewId: string, stageId: string) => call<Summary>("stage_summary", { reviewId, stageId }),
  exportCsv: (reviewId: string, stageId: string, path: string) =>
    call<void>("stage_export_csv", { reviewId, stageId, path }),

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
