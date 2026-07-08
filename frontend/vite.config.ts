import { Agent } from "node:http";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const backendPort = Number.parseInt(process.env.TTTB_PORT ?? "8080", 10);
const backendTarget = `http://127.0.0.1:${backendPort}`;

// Reuse TCP connections to the backend instead of opening a fresh localhost
// socket per request. The per-request connect intermittently stalled under
// load and surfaced in the browser as "connect ETIMEDOUT 127.0.0.1" proxy
// errors ("Could not load asset data"); pooling removes that hop from the hot
// path.
const keepAliveAgent = new Agent({
  keepAlive: true,
  keepAliveMsecs: 15_000,
  maxSockets: 64,
});

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": {
        target: backendTarget,
        agent: keepAliveAgent,
        // Leave a richer, timestamped trace when a proxy hop fails, so an
        // intermittent connect stall can be diagnosed after the fact instead
        // of only "http proxy error".
        configure: (proxy) => {
          proxy.on("error", (error, request) => {
            const err = error as NodeJS.ErrnoException;
            const code = err.code ?? "UNKNOWN";
            const syscall = err.syscall ?? "-";
            console.error(
              `[api-proxy] ${new Date().toISOString()} ${code} ${syscall} ` +
                `${request.method ?? "?"} ${request.url ?? "?"} -> ${backendTarget}: ${err.message}`,
            );
          });
        },
      },
    },
  },
});
