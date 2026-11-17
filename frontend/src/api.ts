// Dünner Client für die API. Sitzung über Cookie; schreibende Aufrufe tragen das CSRF-Token.

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

function csrfToken(): string {
  const match = document.cookie.match(/(?:^|; )csrftoken=([^;]+)/);
  return match ? decodeURIComponent(match[1]) : "";
}

interface Options {
  method?: string;
  body?: unknown;
  form?: FormData;
}

export async function api<T>(path: string, options: Options = {}): Promise<T> {
  const method = options.method ?? "GET";
  const headers: Record<string, string> = {};
  let body: BodyInit | undefined;
  if (options.form) {
    body = options.form;
  } else if (options.body !== undefined) {
    headers["Content-Type"] = "application/json";
    body = JSON.stringify(options.body);
  }
  if (method !== "GET") headers["X-CSRFToken"] = csrfToken();
  const response = await fetch("/api" + path, { method, headers, body, credentials: "same-origin" });
  const text = await response.text();
  let data: unknown = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = null;
  }
  if (!response.ok) {
    let message = `Fehler ${response.status}`;
    if (data && typeof data === "object") {
      const detail = (data as Record<string, unknown>).detail;
      if (typeof detail === "string" && detail) message = detail;
    }
    throw new ApiError(response.status, message);
  }
  return data as T;
}

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : "Unbekannter Fehler";
}

export interface Me {
  authenticated: boolean;
  username?: string;
  is_staff?: boolean;
}

export interface ReviewListItem {
  id: string;
  title: string;
  role: string;
}

export interface Criteria {
  version: number;
  text: string;
  include_keywords: string[];
  exclude_keywords: string[];
  exclusion_reasons: string[];
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

export interface ReviewDetail {
  id: string;
  title: string;
  role: string;
  members: { username: string; role: string }[];
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
  username: string;
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

export interface Summary {
  stage: Stage;
  progress: Progress;
  reviewers?: { username: string; n_votes: number }[];
  decisions?: {
    include: number;
    exclude: number;
    maybe: number;
    disagreement: number;
    needs_clarification: number;
    pending: number;
  };
  agreement?: {
    percent: { n_items?: number; percent_agreement?: number | null };
    krippendorff: { n_items?: number; alpha?: number | null };
    cohen_pairs: CohenPair[];
  };
}

export const canManage = (role: string) => role === "owner" || role === "admin";
export const canVote = (role: string) => role === "owner" || role === "admin" || role === "reviewer";
