import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

// dev: proxy /api และ /ws ไป Rust backend (พอร์ต 8080)
export default defineConfig({
  plugins: [solid()],
  server: {
    port: 5173,
    proxy: {
      "/api": "http://localhost:8080",
      "/ws": { target: "ws://localhost:8080", ws: true },
    },
  },
  build: { target: "esnext", outDir: "dist" },
});
