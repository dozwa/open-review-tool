// Einstellungen in der Vorschau mit Beispieldaten. Sie liegen im Browser-Speicher, damit ein Wechsel der
// Sprache das Neuladen übersteht. Dieses Modul kennt `i18n` nicht, denn es lädt vor der Sprache.

import { DEFAULTS, ProjectSettings, ProjectView, Settings } from "../settings";

const KEY = "ort.mock.settings";
const projects = new Map<string, ProjectSettings>();
const folders = new Map<string, string | null>([["r1", "/Users/beispiel/Reviews/Migräne/Volltexte"]]);

export function readMockSettings(): { settings: Settings; stored: boolean } {
  try {
    const text = localStorage.getItem(KEY);
    if (text) return { settings: { ...DEFAULTS, ...JSON.parse(text) }, stored: true }; // Felder, die es beim Speichern noch nicht gab, nehmen den Standard
  } catch {
    // Ohne Speicher gelten die Standardwerte.
  }
  return { settings: DEFAULTS, stored: false };
}

function view(reviewId: string): ProjectView {
  const program = readMockSettings().settings;
  const project = projects.get(reviewId) ?? { output_language: null, export_dir: null, package_dir: null, screening: null, color: reviewId === "r1" ? "green" : "" };
  return {
    project,
    effective: {
      output_language: project.output_language ?? program.output_language,
      export_dir: project.export_dir ?? program.export_dir,
      package_dir: project.package_dir ?? program.package_dir,
      screening: project.screening ?? program.screening,
    },
    file: `/Users/beispiel/Library/Application Support/org.openreviewtool.app/reviews/${reviewId}.ortreview`,
    fulltext_folder: folders.get(reviewId) ?? null,
  };
}

/** Kennfarbe eines Reviews für die Liste. */
export const mockColor = (reviewId: string): string => view(reviewId).project.color;

/** Beantwortet die Befehle zu den Einstellungen; `undefined` bei jedem anderen Befehl. */
export function mockSettings(cmd: string, a: Record<string, unknown>): unknown {
  switch (cmd) {
    case "settings_get":
      return { ...readMockSettings(), data_dir: "/Users/beispiel/Library/Application Support/org.openreviewtool.app", reviews_dir: "/Users/beispiel/Library/Application Support/org.openreviewtool.app/reviews" };
    case "settings_set":
      try {
        localStorage.setItem(KEY, JSON.stringify(a.settings));
      } catch {
        // gilt dann nur bis zum Neuladen
      }
      return null;
    case "project_settings_get":
      return view(String(a.reviewId));
    case "project_settings_set":
      projects.set(String(a.reviewId), a.project as ProjectSettings);
      return view(String(a.reviewId));
    case "fulltext_folder_set":
      folders.set(String(a.reviewId), (a.path as string | null) ?? null);
      return undefined;
    case "storage_reveal":
      return null;
    default:
      return undefined;
  }
}
