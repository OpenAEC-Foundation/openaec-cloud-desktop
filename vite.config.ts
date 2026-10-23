import { defineConfig } from "vite"
import solid from "vite-plugin-solid"

// Tauri verwacht een vaste dev-poort; clearScreen uit zodat Rust-logs zichtbaar blijven.
export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_"],
  // Drie vensters: het hoofdvenster (index.html), het statusvenster bij het systeemvak (status.html) en het
  // deelvenster (delen.html).
  build: {
    target: "esnext",
    minify: true,
    rollupOptions: { input: { main: "index.html", status: "status.html", delen: "delen.html" } },
  },
})
