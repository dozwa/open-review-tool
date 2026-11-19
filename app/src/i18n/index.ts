// Sprachen der Oberfläche.
//
// Der deutsche Text im Quelltext ist zugleich der Schlüssel: `t("Speichern")`. Jede weitere Sprache
// ist ein Katalog in diesem Ordner, der den deutschen Text auf seine Übersetzung abbildet, plus ein
// Eintrag in `LANGUAGES`. Der Typ von `t` lässt nur Texte zu, die im englischen Katalog stehen; ein
// neuer Text ohne Übersetzung fällt damit bei `npm run typecheck` auf.
//
// Die Sprache ist eine Einstellung der Person (im Browser-Speicher der App), nie Teil eines Reviews.
// Ein Wechsel lädt die Oberfläche neu; deshalb darf `t` auch in Konstanten auf Modulebene stehen.

import React from "react";

import { en } from "./en";

/** Eine Übersetzung: ein Text oder je Mehrzahlform einer (`one`, `other`, je nach Sprache auch `few`, `many`). */
export type Entry = string | Partial<Record<Intl.LDMLPluralRule, string>>;
export type Key = keyof typeof en;
/** Katalog einer weiteren Sprache. Was fehlt, erscheint auf Englisch. */
export type Catalog = Partial<Record<Key, Entry>>;

export const LANGUAGES = [
  { code: "de", name: "Deutsch", locale: "de-DE", catalog: null },
  { code: "en", name: "English", locale: "en-GB", catalog: en as Catalog },
] as const satisfies readonly { code: string; name: string; locale: string; catalog: Catalog | null }[];
export type LanguageCode = (typeof LANGUAGES)[number]["code"];

const STORAGE_KEY = "ort.language";
const FALLBACK: Catalog = en;

function known(code: string | null | undefined): LanguageCode | null {
  const base = (code ?? "").split(/[-_]/)[0].toLowerCase();
  return LANGUAGES.find((l) => l.code === base)?.code ?? null;
}

function initial(): LanguageCode {
  let stored: string | null = null;
  try {
    stored = localStorage.getItem(STORAGE_KEY);
  } catch {
    stored = null;
  }
  // Ohne eigene Wahl gilt die Sprache des Systems, sonst Englisch.
  return known(stored) ?? known(typeof navigator === "undefined" ? null : navigator.language) ?? "en";
}

/** Die Sprache der Oberfläche. Sie steht beim Laden fest. */
export const language: LanguageCode = initial();
const catalog: Catalog | null = LANGUAGES.find((l) => l.code === language)?.catalog ?? null;
/** Schreibweise von Zahlen und Daten, z. B. `en-GB` für britisches Englisch. */
const locale: string = LANGUAGES.find((l) => l.code === language)?.locale ?? language;
const plural = new Intl.PluralRules(locale);

if (typeof document !== "undefined") document.documentElement.lang = language;

/** Wählt eine Sprache und lädt die Oberfläche neu. */
export function setLanguage(code: LanguageCode) {
  try {
    localStorage.setItem(STORAGE_KEY, code);
  } catch {
    // Ohne Speicher gilt die Wahl nur bis zum nächsten Start.
  }
  window.location.reload();
}

const OUTPUT_KEY = "ort.outputLanguage";

/**
 * Sprache für Dateien, die das Review verlassen (Bericht, Vorbereitung der Registrierung). Sie darf von
 * der Sprache der Oberfläche abweichen: Ein Team arbeitet deutsch und berichtet englisch.
 */
export function outputLanguage(): LanguageCode {
  try {
    return known(localStorage.getItem(OUTPUT_KEY)) ?? language;
  } catch {
    return language;
  }
}

export function setOutputLanguage(code: LanguageCode) {
  try {
    localStorage.setItem(OUTPUT_KEY, code);
  } catch {
    // Ohne Speicher gilt die Wahl nur für diesen Aufruf.
  }
}

type Params = Record<string, string | number>;

function pattern(key: string, count?: number): string {
  const entry: Entry | undefined = catalog ? (catalog as Record<string, Entry>)[key] ?? (FALLBACK as Record<string, Entry>)[key] : undefined;
  if (entry === undefined) return key;
  if (typeof entry === "string") return entry;
  return entry[plural.select(count ?? 0)] ?? entry.other ?? key;
}

const fill = (text: string, params?: Params) =>
  params ? text.replace(/\{(\w+)\}/g, (whole, name: string) => (name in params ? String(params[name]) : whole)) : text;

/** Text in der Sprache der Oberfläche. Werte stehen als `{name}` im Text. */
export function t(key: Key, params?: Params): string {
  return fill(pattern(key), params);
}

/**
 * Text mit Einzahl und Mehrzahl: `tn("{n} Eintrag", "{n} Einträge", n)`. Schlüssel im Katalog ist die
 * Einzahl; die Übersetzung nennt dort ihre Formen (`{ one: "{n} record", other: "{n} records" }`).
 * `{n}` ist immer die Zahl.
 */
export function tn(one: Key, other: string, count: number, params?: Params): string {
  const all = { n: count, ...params };
  if (!catalog) return fill(count === 1 ? one : other, all);
  return fill(pattern(one, count), all);
}

/** Wie `t`, aber mit Elementen im Satz: `tx("Mehr dazu im {link}.", { link: <a>Handbuch</a> })`. */
export function tx(key: Key, parts: Record<string, React.ReactNode>): React.ReactNode {
  const pieces = pattern(key).split(/(\{\w+\})/g);
  return React.createElement(
    React.Fragment,
    null,
    ...pieces.map((piece) => {
      const name = /^\{(\w+)\}$/.exec(piece)?.[1];
      return name !== undefined && name in parts ? parts[name] : piece;
    }),
  );
}

/**
 * Großer Anfangsbuchstabe für Begriffe der Vorlage, die allein als Beschriftung stehen. Der Kern liefert
 * sie so, wie sie mitten im Satz stehen (englisch klein).
 */
export const capital = (text: string): string => text.charAt(0).toLocaleUpperCase(locale) + text.slice(1);

/** Übersetzt einen Text, der nicht im Quelltext steht (z. B. aus der Vorschau). Ohne Eintrag bleibt er. */
export const tAny = (text: string, params?: Params): string => fill(pattern(text), params);

// ---------------------------------------------------------------- Zahlen und Daten

/** Tag als Datum der Sprache, aus `JJJJ-MM-TT` oder einem ISO-Zeitstempel. Leeres bleibt leer. */
export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "";
  const date = new Date(iso.length === 10 ? `${iso}T12:00:00` : iso);
  return Number.isNaN(date.getTime()) ? iso : new Intl.DateTimeFormat(locale, { day: "2-digit", month: "2-digit", year: "numeric" }).format(date);
}

/** Datum mit Uhrzeit. */
export function formatDateTime(iso: string | null | undefined): string {
  if (!iso) return "";
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

/** Zahl mit dem Dezimalzeichen der Sprache. */
export function formatNumber(value: number, digits = 0): string {
  return new Intl.NumberFormat(locale, { minimumFractionDigits: digits, maximumFractionDigits: digits }).format(value);
}

/** Anteil in Prozent, z. B. `71,9 %` oder `71.9%`. */
export function formatPercent(value: number, digits = 0): string {
  return new Intl.NumberFormat(locale, { style: "percent", minimumFractionDigits: digits, maximumFractionDigits: digits }).format(value / 100);
}
