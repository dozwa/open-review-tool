import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Läuft in der WebView von Tauri (WKWebView). Im Entwicklungsmodus lädt Tauri die Seite von diesem Server.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
    // Die Vorschau im Browser liest Leitlinien und Registrierungsformular aus dem Kern (`../crates`).
    fs: { allow: [".."] },
  },
  build: {
    target: "safari15",
    outDir: "dist",
  },
});
