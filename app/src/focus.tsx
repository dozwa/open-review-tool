// Fokusmodus für die Arbeit an der Literatur (Screening, Eingabe im Formular): Die Seitenleiste und alles,
// was nicht zum Lesen und Entscheiden gehört, verschwindet, und das Fenster geht in den Vollbildmodus.
// Der Modus gilt, solange die Seite offen ist; das Kürzel aus den Einstellungen (anfangs `F`) schaltet um,
// `Esc` beendet ihn.

import { useCallback, useEffect, useRef, useState } from "react";

import { t } from "./i18n";
import { useSettings } from "./settings";
import { hits, shortcutLabel } from "./shortcuts";
import { Icon } from "./ui";

const native = () => document.documentElement.classList.contains("native");

/** Vollbild des Fensters: in der App über Tauri, in der Vorschau über den Browser. */
async function isFullscreen(): Promise<boolean> {
  if (!native()) return document.fullscreenElement !== null;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow().isFullscreen();
}

async function setFullscreen(on: boolean) {
  if (native()) {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().setFullscreen(on);
  } else if (on) await document.documentElement.requestFullscreen();
  else if (document.fullscreenElement) await document.exitFullscreen();
}

export function useFocusMode(): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState(false);
  // War das Fenster schon vorher im Vollbild, bleibt es das auch nach dem Fokusmodus.
  const wasFullscreen = useRef(false);
  const shortcut = useSettings().shortcuts.focus;

  const set = useCallback((next: boolean) => {
    setOn(next);
    if (next) document.documentElement.dataset.focus = "on";
    else delete document.documentElement.dataset.focus;
    (async () => {
      if (next) {
        wasFullscreen.current = await isFullscreen();
        if (!wasFullscreen.current) await setFullscreen(true);
      } else if (!wasFullscreen.current) await setFullscreen(false);
    })().catch(() => undefined); // Ohne Vollbild bleibt der Rest des Fokusmodus.
  }, []);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape" && on) set(false);
      else if (hits(event, shortcut)) {
        event.preventDefault();
        set(!on);
      }
    }
    // Im Browser beendet Esc das Vollbild selbst; der Fokusmodus endet dann mit.
    const onLeave = () => {
      if (on && !native() && !document.fullscreenElement) {
        wasFullscreen.current = true;
        set(false);
      }
    };
    window.addEventListener("keydown", onKey);
    document.addEventListener("fullscreenchange", onLeave);
    return () => {
      window.removeEventListener("keydown", onKey);
      document.removeEventListener("fullscreenchange", onLeave);
    };
  }, [on, set, shortcut]);

  // Beim Verlassen der Seite endet der Modus.
  const active = useRef(false);
  active.current = on;
  useEffect(() => () => {
    if (active.current) set(false);
  }, [set]);

  return [on, set];
}

export function FocusButton({ on, onChange }: { on: boolean; onChange: (on: boolean) => void }) {
  const shortcut = useSettings().shortcuts.focus;
  const start = t("Fokusmodus: nur der Text, im Vollbild");
  return (
    <button className="ghost focus-button" aria-pressed={on} onClick={() => onChange(!on)} title={on ? t("Fokusmodus beenden (Esc)") : shortcut ? `${start} (${shortcutLabel(shortcut)})` : start}>
      <Icon name={on ? "x" : "eye"} size={14} /> {on ? t("Fokus beenden") : t("Fokus")}
    </button>
  );
}
