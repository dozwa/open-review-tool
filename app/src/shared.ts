import { createContext, useContext } from "react";

import type { Stage } from "./api";
import { capital, t } from "./i18n";

export const ROLE_LABEL: Record<string, string> = { owner: "Owner", admin: "Admin", reviewer: "Reviewer", viewer: "Viewer" };

/** Seitenleiste neu laden, z. B. nach Anlegen oder Einlesen eines Reviews. */
export const RefreshContext = createContext<() => void>(() => {});
export const useRefreshSidebar = () => useContext(RefreshContext);

/** Die Bereiche eines Reviews. Sie stehen in der Seitenleiste; welcher offen ist, steht in der Adresse (`?tab=`). */
export const REVIEW_TABS = [
  { value: "overview", label: t("Ablauf"), icon: "stack" },
  { value: "criteria", label: t("Kriterien"), icon: "list" },
  { value: "registration", label: t("Registrierung"), icon: "pen" },
  { value: "search", label: t("Suche"), icon: "search" },
  { value: "imports", label: t("Import"), icon: "upload" },
  { value: "fulltext", label: t("Volltexte"), icon: "file" },
  { value: "sources", label: capital(t("Quellen")), icon: "stack" },
  { value: "forms", label: t("Formular"), icon: "list" },
  { value: "report", label: t("Bericht"), icon: "chart" },
  { value: "guidance", label: t("Leitlinie"), icon: "book" },
  { value: "members", label: t("Mitglieder"), icon: "people" },
  { value: "exchange", label: t("Austausch"), icon: "exchange" },
  { value: "proof", label: t("Nachweis"), icon: "shield" },
  { value: "settings", label: t("Einstellungen"), icon: "gear" },
] as const;
export type ReviewTab = (typeof REVIEW_TABS)[number]["value"];
export const isReviewTab = (value: string | null): value is ReviewTab => REVIEW_TABS.some((tab) => tab.value === value);

/** Begriffe aus der Vorlage und Stufen des offenen Reviews, damit die Seitenleiste sie zeigen kann. */
export const TermsContext = createContext<(reviewId: string, terms: Record<string, string>, stages: Stage[]) => void>(() => {});
export const useShareTerms = () => useContext(TermsContext);
