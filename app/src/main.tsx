// Start der Oberfläche. Erst die Einstellungen, dann alles andere: Die Sprache steht in den Einstellungen
// und muss feststehen, bevor ein Modul mit Texten lädt. Deshalb kommt die App erst danach dazu.

import { loadSettings, presetSettings } from "./settings";
import "./styles.css";

async function start() {
  const native = "__TAURI_INTERNALS__" in window;
  document.documentElement.classList.toggle("native", native);
  // Vorschau im Browser (nur Entwicklung): Rust-Befehle durch Beispieldaten ersetzen, mit `?bridge=<port>`
  // durch die Entwicklungsbrücke zum echten Kern.
  if (import.meta.env.DEV && !native) {
    const bridge = new URLSearchParams(location.search).get("bridge");
    if (bridge) await import("./dev/bridge").then((m) => m.installBridge(bridge));
    else {
      // Die Beispieldaten sind übersetzt; ihre Sprache muss deshalb schon vor ihrem Laden feststehen.
      presetSettings((await import("./dev/mockSettings")).readMockSettings().settings);
      await import("./dev/mock").then((m) => m.installMock());
    }
  }
  await loadSettings(native);
  await import("./App").then((app) => app.render());
}

start();
