// Einstellungen der Oberfläche: Programm (gilt überall) und Projekt (überschreibt für ein Review).
// Beide liegen nur auf diesem Rechner; der Kern prüft, speichert und führt sie zusammen (`settings.rs`).
//
// Dieses Modul lädt vor allem anderen und kennt deshalb weder `i18n` noch `api`: Die Sprache der
// Oberfläche steht in den Einstellungen und muss feststehen, bevor ein Text übersetzt wird.

import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface ScreeningLayout {
  show_list: boolean;
  show_criteria: boolean;
  show_source: boolean;
  highlight: boolean;
}

export interface Shortcuts {
  include: string;
  maybe: string;
  exclude: string;
  next: string;
  previous: string;
  focus: string;
}

export interface Settings {
  version: number;
  /** Leer: Sprache des Systems. */
  language: string;
  /** Leer: wie die Oberfläche. */
  output_language: string;
  appearance: "system" | "light" | "dark";
  accent: string;
  palette: "standard" | "colorblind";
  font_size: "small" | "normal" | "large";
  density: "compact" | "normal";
  screening: ScreeningLayout;
  /** Tastenkürzel wie `M` oder `Alt+M`; leer heißt ohne. */
  shortcuts: Shortcuts;
  /** Leer: Der Dialog beginnt im zuletzt benutzten Ordner. */
  export_dir: string;
  package_dir: string;
}

/** `null` heißt „wie im Programm“. */
export interface ProjectSettings {
  output_language: string | null;
  export_dir: string | null;
  package_dir: string | null;
  screening: ScreeningLayout | null;
  color: string;
}

export interface Effective {
  output_language: string;
  export_dir: string;
  package_dir: string;
  screening: ScreeningLayout;
}

export interface ProjectView {
  project: ProjectSettings;
  effective: Effective;
  file: string;
  fulltext_folder: string | null;
}

export const ACCENTS = ["system", "blue", "purple", "pink", "red", "orange", "green", "graphite"] as const;
export const PROJECT_COLORS = ["blue", "purple", "pink", "red", "orange", "yellow", "green", "gray"] as const;

export const DEFAULTS: Settings = {
  version: 1,
  language: "",
  output_language: "",
  appearance: "system",
  accent: "system",
  palette: "standard",
  font_size: "normal",
  density: "normal",
  screening: { show_list: true, show_criteria: true, show_source: false, highlight: true },
  shortcuts: { include: "I", maybe: "M", exclude: "E", next: "J", previous: "K", focus: "F" },
  export_dir: "",
  package_dir: "",
};

let program: Settings = DEFAULTS;
let storage = { data_dir: "", reviews_dir: "" };
let native = false;
const projects = new Map<string, ProjectView>();
// Zählt jede Änderung, damit `useSyncExternalStore` neu zeichnet.
let revision = 0;
const listeners = new Set<() => void>();

function changed() {
  revision += 1;
  listeners.forEach((listener) => listener());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}

/** Die Einstellungen des Programms, wie sie beim Aufruf gelten. */
export const settings = (): Settings => program;
/** Wo die Daten liegen (nur zur Anzeige). */
export const storageInfo = () => storage;

const dark = typeof matchMedia === "undefined" ? null : matchMedia("(prefers-color-scheme: dark)");

/** Überträgt Erscheinungsbild, Farben, Schriftgröße und Dichte auf das Dokument. */
function apply() {
  const root = document.documentElement;
  const theme = program.appearance === "system" ? (dark?.matches ? "dark" : "light") : program.appearance;
  root.dataset.theme = theme;
  root.dataset.accent = program.accent;
  root.dataset.palette = program.palette;
  root.dataset.size = program.font_size;
  root.dataset.density = program.density;
}

/** In der App folgt auch das Fenster (Titelleiste, Glaseffekt) dem gewählten Erscheinungsbild. */
async function applyWindowTheme() {
  if (!native) return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().setTheme(program.appearance === "system" ? null : program.appearance);
  } catch {
    // Ohne das Recht bleibt das Fenster beim Erscheinungsbild des Systems.
  }
}

dark?.addEventListener("change", apply);

/** Werte aus dem Browser-Speicher früherer Versionen; sie ziehen einmal in die Datei um. */
function legacy(): Partial<Settings> {
  const out: Partial<Settings> = {};
  try {
    const language = localStorage.getItem("ort.language");
    if (language) out.language = language;
    const output = localStorage.getItem("ort.outputLanguage");
    if (output) out.output_language = output;
    const source = localStorage.getItem("ort.screening.source");
    const criteria = localStorage.getItem("ort.screening.criteria");
    if (source !== null || criteria !== null) {
      out.screening = {
        ...DEFAULTS.screening,
        show_source: source === null ? DEFAULTS.screening.show_source : source === "1",
        show_criteria: criteria === null ? DEFAULTS.screening.show_criteria : criteria === "1",
      };
    }
    ["ort.language", "ort.outputLanguage", "ort.screening.source", "ort.screening.criteria"].forEach((key) => localStorage.removeItem(key));
  } catch {
    // Ohne Browser-Speicher gibt es nichts umzuziehen.
  }
  return out;
}

/** Nur für die Vorschau mit Beispieldaten: Einstellungen, die schon vor dem Laden bekannt sind. */
export function presetSettings(known: Settings) {
  program = known;
}

/** Lädt die Einstellungen vor dem ersten Zeichnen. Schlägt das fehl, gelten die Standardwerte. */
export async function loadSettings(isNative: boolean) {
  native = isNative;
  try {
    const view = await invoke<{ settings: Settings; stored: boolean; data_dir: string; reviews_dir: string }>("settings_get");
    program = view.settings;
    storage = { data_dir: view.data_dir, reviews_dir: view.reviews_dir };
    if (!view.stored) {
      const old = legacy();
      if (Object.keys(old).length > 0) {
        program = { ...program, ...old };
        await invoke("settings_set", { settings: program }).catch(() => {
          program = view.settings;
        });
      }
    }
  } catch {
    program = DEFAULTS;
  }
  apply();
  void applyWindowTheme();
}

/** Ändert Einstellungen des Programms. Sie gelten sofort; bei einem Fehler gilt wieder der alte Stand. */
export async function saveSettings(patch: Partial<Settings>) {
  const before = program;
  program = { ...program, ...patch };
  apply();
  changed();
  try {
    await invoke("settings_set", { settings: program });
    // Was für ein Review gilt, hängt an den Programmwerten.
    projects.clear();
    if ("appearance" in patch) void applyWindowTheme();
    changed();
  } catch (e) {
    program = before;
    apply();
    changed();
    throw e;
  }
}

/** Einstellungen eines Reviews; nach dem ersten Laden aus dem Zwischenspeicher. */
export async function projectSettings(reviewId: string): Promise<ProjectView> {
  const known = projects.get(reviewId);
  if (known) return known;
  const view = await invoke<ProjectView>("project_settings_get", { reviewId });
  projects.set(reviewId, view);
  changed();
  return view;
}

// Änderungen an einem Review laufen nacheinander, damit eine schnelle zweite die erste nicht überschreibt.
let saving: Promise<unknown> = Promise.resolve();

export function saveProjectSettings(reviewId: string, patch: Partial<ProjectSettings>): Promise<ProjectView> {
  const next = saving.catch(() => undefined).then(async () => {
    const current = await projectSettings(reviewId);
    const view = await invoke<ProjectView>("project_settings_set", { reviewId, project: { ...current.project, ...patch } });
    projects.set(reviewId, view);
    changed();
    return view;
  });
  saving = next;
  return next;
}

/** Verwirft den Zwischenstand eines Reviews, etwa nachdem der Volltextordner geändert wurde. */
export function forgetProjectSettings(reviewId: string) {
  projects.delete(reviewId);
  changed();
}

export function useSettings(): Settings {
  useSyncExternalStore(subscribe, () => revision);
  return program;
}

/** Einstellungen eines Reviews; `null`, solange sie laden. */
export function useProjectSettings(reviewId: string): ProjectView | null {
  useSyncExternalStore(subscribe, () => revision);
  const view = projects.get(reviewId) ?? null;
  useEffect(() => {
    if (!view) projectSettings(reviewId).catch(() => undefined);
  }, [reviewId, view]);
  return view;
}

/**
 * Aufbau der Screening-Seite für ein Review und eine Funktion, die ihn ändert. Die Änderung landet dort,
 * wo der Wert herkommt: im Projekt, wenn es einen eigenen Aufbau hat, sonst im Programm.
 */
export function useScreeningLayout(reviewId: string): [ScreeningLayout, (patch: Partial<ScreeningLayout>) => void] {
  const own = useProjectSettings(reviewId)?.project.screening ?? null;
  const layout = own ?? useSettings().screening;
  const set = (patch: Partial<ScreeningLayout>) => {
    const next = { ...layout, ...patch };
    const done = own ? saveProjectSettings(reviewId, { screening: next }) : saveSettings({ screening: next });
    done.catch(() => undefined);
  };
  return [layout, set];
}

/**
 * Vorschlag für einen Speicherdialog: der Dateiname im eingestellten Ordner. `export` steht für Berichte
 * und Tabellen, `package` für Pakete und Rückmeldungen. Ohne Ordner bleibt der Name allein, und der
 * Dialog beginnt, wo er zuletzt war.
 */
export async function inFolder(kind: "export" | "package", name: string, reviewId?: string): Promise<string> {
  const dir = await folder(kind, reviewId);
  return dir ? `${dir.replace(/\/+$/, "")}/${name}` : name;
}

/** Der eingestellte Ordner für einen Öffnen-Dialog; `undefined` ohne Wahl. */
export async function folder(kind: "export" | "package", reviewId?: string): Promise<string | undefined> {
  const key = kind === "export" ? "export_dir" : "package_dir";
  const view = reviewId ? await projectSettings(reviewId).catch(() => null) : null;
  return (view ? view.effective[key] : program[key]) || undefined;
}
