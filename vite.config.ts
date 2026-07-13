import { defineConfig } from "vite"
import solid from "vite-plugin-solid"

// Tauri verwacht een vaste dev-poort; clearScreen uit zodat Rust-logs zichtbaar blijven.
export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_"],
  build: { target: "esnext", minify: true },
})
