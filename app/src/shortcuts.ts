// Tastenkürzel der Arbeit an der Literatur: aus einem Tastendruck das Kürzel lesen, vergleichen und anzeigen.
// Welche Kürzel gelten, steht in den Einstellungen des Programms; der Kern prüft sie (`settings.rs`).
//
// Ein Kürzel ist eine Taste mit beliebig vielen Zusatztasten davor, etwa `M`, `Alt+M` oder `Ctrl+Shift+ArrowDown`.

import type { Shortcuts } from "./settings";

export type ShortcutAction = keyof Shortcuts;
export const SHORTCUT_ACTIONS: ShortcutAction[] = ["include", "maybe", "exclude", "next", "previous", "focus"];

const MODIFIER_KEYS = ["Control", "Alt", "Shift", "Meta", "AltGraph", "CapsLock", "Fn", "Unidentified"];
/** Fest belegt oder zum Verlassen eines Feldes nötig; mit Strg, Wahl- oder Befehlstaste sind sie frei. */
const RESERVED = ["Escape", "Tab", "Enter", "Space", "Backspace", "Delete"];

// Mit Wahl- oder Umschalttaste meldet der Browser ein anderes Zeichen („µ“ statt „M“). Die Taste selbst
// verrät dann nur ihre Lage auf der Tastatur. Welches Zeichen dort ohne Zusatztaste liegt, merkt sich die
// App bei jedem gewöhnlichen Tastendruck; bis dahin gilt die amerikanische Belegung.
const LEARNED = "ort.keys";
let learned: Record<string, string> = {};
try {
  learned = JSON.parse(localStorage.getItem(LEARNED) ?? "{}");
} catch {
  // Ohne Browser-Speicher lernt die App die Belegung in jeder Sitzung neu.
}

function learn(code: string, key: string) {
  if (!code || learned[code] === key) return;
  learned[code] = key;
  try {
    localStorage.setItem(LEARNED, JSON.stringify(learned));
  } catch {
    // gilt dann nur für diese Sitzung
  }
}

const named = (key: string) => (key === " " ? "Space" : key === "+" ? "Plus" : key.toUpperCase());

/** Die Taste eines Tastendrucks ohne Zusatztasten; `null`, wenn nur eine Zusatztaste gedrückt ist. */
function keyOf(event: KeyboardEvent): string | null {
  const key = event.key;
  if (MODIFIER_KEYS.includes(key)) return null;
  if (key.length > 1 && key !== "Dead") return key;
  if (!event.altKey && !event.shiftKey && key !== "Dead") {
    learn(event.code, named(key));
    return named(key);
  }
  const usual = /^Key([A-Z])$/.exec(event.code)?.[1] ?? /^Digit(\d)$/.exec(event.code)?.[1];
  return learned[event.code] ?? usual ?? (key === "Dead" ? null : named(key));
}

/** Das Kürzel eines Tastendrucks in der gespeicherten Schreibweise. */
export function shortcutOf(event: KeyboardEvent): string | null {
  const key = keyOf(event);
  if (!key) return null;
  const parts = [event.ctrlKey && "Ctrl", event.altKey && "Alt", event.shiftKey && "Shift", event.metaKey && "Meta", key];
  return parts.filter(Boolean).join("+");
}

/** Ob gerade in ein Feld getippt wird. */
export function typing(event: KeyboardEvent): boolean {
  const target = event.target as HTMLElement | null;
  return target !== null && (["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) || target.isContentEditable);
}

/**
 * Ob der Tastendruck das Kürzel ist. Beim Tippen in einem Feld zählen nur Kürzel mit Strg oder Befehlstaste:
 * Buchstaben gehören dann zum Text, und die Wahltaste schreibt Sonderzeichen.
 */
export function hits(event: KeyboardEvent, shortcut: string): boolean {
  if (!shortcut || shortcutOf(event) !== shortcut) return false;
  return !typing(event) || event.ctrlKey || event.metaKey;
}

/** Ob das Kürzel vergeben werden darf: Die Ziffern wählen den Grund, einige Tasten sind fest belegt. */
export function allowed(shortcut: string): boolean {
  const parts = shortcut.split("+");
  const key = parts.pop() ?? "";
  if (parts.some((part) => part !== "Shift")) return true;
  return !/^\d$/.test(key) && !RESERVED.includes(key);
}

const SYMBOLS: Record<string, string> = {
  Ctrl: "⌃", Alt: "⌥", Shift: "⇧", Meta: "⌘", ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→",
  Enter: "↩", Space: "␣", Backspace: "⌫", Delete: "⌦", Tab: "⇥", Escape: "esc", Plus: "+",
};

/** Das Kürzel, wie es auf den Tasten des Mac steht (`Alt+M` wird „⌥M“). */
export const shortcutLabel = (shortcut: string): string => shortcut.split("+").map((part) => SYMBOLS[part] ?? part).join("");
