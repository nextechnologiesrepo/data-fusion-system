import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The dashboard is a thin client: it proxies all API calls to the fusion-api
// service so the browser only ever talks to the dev server origin.
const apiTarget = process.env.VITE_API_TARGET || "http://localhost:8088";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": { target: apiTarget, changeOrigin: true },
      "/health": { target: apiTarget, changeOrigin: true },
      "/openapi.yaml": { target: apiTarget, changeOrigin: true },
    },
  },
});
