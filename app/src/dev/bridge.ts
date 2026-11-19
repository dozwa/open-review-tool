// Nur Entwicklung: Die Vorschau im Browser spricht über die Entwicklungsbrücke (`bridge.rs`) mit dem
// echten Kern statt mit Beispieldaten. Aufruf: http://127.0.0.1:5173/?bridge=<port>.
//
// Dateidialoge gibt es im Browser nicht. „Speichern“ legt die Datei im Ausgabeverzeichnis der Brücke
// ab; „Öffnen“ und „Speichern unter“ nehmen den nächsten Pfad aus `window.__ortPicks`, wenn dort einer
// liegt (für Tests per Skript). Rückfragen werden mit Ja beantwortet.

import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";

declare global {
  interface Window {
    __ortPicks: (string | string[] | null)[];
    __ortSaved: string[];
  }
}

export function installBridge(port: string) {
  window.__ortPicks = [];
  window.__ortSaved = [];
  mockWindows("main");
  let out: string | null = null;

  async function send(cmd: string, args: unknown) {
    const res = await fetch(`http://127.0.0.1:${port}/`, { method: "POST", body: JSON.stringify({ cmd, args }) });
    const answer = await res.json();
    if (answer && "err" in answer) throw answer.err;
    if (answer && "raw" in answer) return Uint8Array.from(atob(answer.raw), (c) => c.charCodeAt(0)).buffer;
    return answer?.ok ?? null;
  }

  mockIPC(async (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "plugin:dialog|open":
        return window.__ortPicks.length ? window.__ortPicks.shift() : null;
      case "plugin:dialog|save": {
        if (window.__ortPicks.length) return window.__ortPicks.shift();
        out ??= (await send("__bridge_out", {})) as string;
        const name = String((a.options as { defaultPath?: string } | undefined)?.defaultPath ?? "datei").split("/").pop();
        const path = `${out}/${name}`;
        window.__ortSaved.push(path);
        return path;
      }
      case "plugin:dialog|ask":
      case "plugin:dialog|confirm":
        return true;
      case "plugin:dialog|message": {
        // `ask` und `confirm` schicken eine Meldung mit Knöpfen und vergleichen den gedrückten Knopf.
        console.info("[ort-dialog]", a.title, "–", a.message);
        const buttons = a.buttons as string | Record<string, string | string[]> | undefined;
        if (buttons === "YesNo" || buttons === "YesNoCancel") return "Yes";
        if (buttons && typeof buttons === "object") {
          const custom = Object.values(buttons)[0];
          return Array.isArray(custom) ? custom[0] : custom;
        }
        return "Ok";
      }
      default:
        return send(cmd, a);
    }
  });
}
