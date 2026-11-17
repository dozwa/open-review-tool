import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Die gebaute Oberfläche wird von Django unter /static/ ausgeliefert.
export default defineConfig({
  base: "/static/",
  plugins: [react()],
  server: {
    proxy: { "/api": "http://localhost:8000" },
  },
});
