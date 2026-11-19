import { createContext, useContext } from "react";

export const ROLE_LABEL: Record<string, string> = { owner: "Owner", admin: "Admin", reviewer: "Reviewer", viewer: "Viewer" };

/** Seitenleiste neu laden, z. B. nach Anlegen oder Einlesen eines Reviews. */
export const RefreshContext = createContext<() => void>(() => {});
export const useRefreshSidebar = () => useContext(RefreshContext);

/** Die Bereiche eines Reviews. Sie stehen in der Seitenleiste; welcher offen ist, steht in der Adresse (`?tab=`). */
export const REVIEW_TABS = [
  { value: "overview", label: "Ablauf", icon: "stack" },
  { value: "criteria", label: "Kriterien", icon: "list" },
  { value: "search", label: "Suche", icon: "search" },
  { value: "imports", label: "Import", icon: "upload" },
  { value: "fulltext", label: "Volltexte", icon: "file" },
  { value: "sources", label: "Quellen", icon: "stack" },
  { value: "report", label: "Bericht", icon: "chart" },
  { value: "members", label: "Mitglieder", icon: "people" },
  { value: "exchange", label: "Austausch", icon: "exchange" },
  { value: "proof", label: "Nachweis", icon: "shield" },
] as const;
export type ReviewTab = (typeof REVIEW_TABS)[number]["value"];
export const isReviewTab = (value: string | null): value is ReviewTab => REVIEW_TABS.some((t) => t.value === value);

/** Begriffe aus der Vorlage des offenen Reviews, damit die Seitenleiste sie zeigen kann. */
export const TermsContext = createContext<(reviewId: string, terms: Record<string, string>) => void>(() => {});
export const useShareTerms = () => useContext(TermsContext);
