import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const backendPort = Number.parseInt(process.env.TTTB_PORT ?? "8080", 10);

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": `http://127.0.0.1:${backendPort}`,
    },
  },
});
