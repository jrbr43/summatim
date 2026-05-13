import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

/** Sert `../chaines.json` en dev pour l’aperçu navigateur (sans IPC Tauri). */
function pluginChainesJsonRacine() {
  const repertoireUi = dirname(fileURLToPath(import.meta.url));
  const cheminChaines = resolve(repertoireUi, "..", "chaines.json");
  return {
    name: "chaines-json-racine",
    configureServer(server: { middlewares: { use: (fn: unknown) => void } }) {
      server.middlewares.use(
        (
          req: { url?: string },
          res: { setHeader: (k: string, v: string) => void; end: (b: string) => void },
          next: () => void,
        ) => {
          const cheminUrl = req.url?.split("?")[0];
          if (cheminUrl === "/chaines.json") {
            try {
              const contenu = readFileSync(cheminChaines, "utf-8");
              res.setHeader("Content-Type", "application/json; charset=utf-8");
              res.end(contenu);
              return;
            } catch {
              /* laisser Vite gérer ou 404 */
            }
          }
          next();
        },
      );
    },
  };
}

export default defineConfig({
  plugins: [
    pluginChainesJsonRacine(),
    svelte({
      compilerOptions: {
        compatibility: {
          componentApi: 4,
        },
      },
    }),
  ],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    // « localhost » peut résoudre en ::1 (IPv6) sous Windows ; 127.0.0.1 seul ne reçoit pas ces requêtes → -102.
    // Écoute sur toutes les interfaces : IPv4 + IPv6, donc http://localhost:5173 et http://127.0.0.1:5173 fonctionnent.
    host: true,
    hmr: {
      protocol: "ws",
      host: "localhost",
      port: 5173,
    },
  },
});
