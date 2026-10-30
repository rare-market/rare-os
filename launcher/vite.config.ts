/// <reference types="vitest/config" />
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const src = (p: string) => fileURLToPath(new URL(`./src/${p}`, import.meta.url));

// Release builds drop the mock and demo modules. `isTauri` is the constant true there (src/lib/mockflag.ts), so
// nothing runs them; this swaps each mock file for a stub with the same export names (all undefined) so even a
// dead import cannot pull its fixtures into the bundle. VITE_MOCK=1 keeps the real files (screenshots against
// `vite preview`); dev and vitest never use this plugin.
const MOCK_FILES = /\/src\/(?:lib\/mock|(?:friends|inventory|notify|share|store|stream|trophies)\/mock|wallet\/(?:ledger|keystone)\/mock|wallet\/ledger\/_testkit|onboarding\/move\/demo|dev\/[^/]+|wallet\/ui\/home\/devSeed)\.tsx?$/;
function dropMocks(): Plugin {
  return {
    name: "rare-drop-mocks",
    apply: "build",
    enforce: "pre",
    async load(id) {
      if (!MOCK_FILES.test(id)) return null;
      const src = await readFile(id, "utf8");
      const names = new Set<string>();
      for (const m of src.matchAll(/^export\s+(?:async\s+)?(?:function\*?|const|let|var|class)\s+([A-Za-z_$][\w$]*)/gm)) names.add(m[1]);
      for (const m of src.matchAll(/^export\s*\{([^}]*)\}/gm)) for (const part of m[1].split(",")) names.add(part.trim().split(/\s+as\s+/).pop()!);
      names.delete("");
      const lines = [...names].map((n) => `export const ${n} = undefined;`);
      if (/^export\s+default\s/m.test(src)) lines.push("export default undefined;");
      return { code: lines.join("\n") + "\nexport {};\n", moduleSideEffects: false };
    },
  };
}

// In the Tauri app the Rust side fetches rare.market (the API sends no CORS headers);
// in browser dev the same paths go through this proxy.
export default defineConfig(({ command, mode }) => {
  // Mocks stay for dev (serve), vitest and an explicit VITE_MOCK=1 build; a release build drops them.
  const mockOk = command === "serve" || mode === "test" || process.env.VITE_MOCK === "1";
  return {
  define: {
    __MOCK_OK__: JSON.stringify(mockOk),
    __RARE_NATIVE__: mockOk ? `(typeof window !== "undefined" && "__TAURI_INTERNALS__" in window)` : "true",
  },
  plugins: [react(), tailwindcss(), ...(mockOk ? [] : [dropMocks()])],
  clearScreen: false,
  resolve: {
    alias: [
      // Ledger: the Sui app client imports Ledger's token-metadata service only for "resolution",
      // which Rare Wallet never uses; the real one needs Node (require('https')) at load
      { find: /^@ledgerhq\/ledger-cal-service$/, replacement: src("wallet/ledger/shims/cal-service.ts") },
      // Keystone: @ngraveio/bc-ur requires Node's `assert`; the npm polyfill reads `process` at load
      { find: /^assert$/, replacement: src("wallet/keystone/shims/assert.ts") },
      // Keystone: bc-ur-registry's bs58check pulls in readable-stream, which needs Node's `process`
      { find: /^bs58check$/, replacement: src("wallet/keystone/shims/bs58check.ts") },
    ],
  },
  server: {
    port: 5180,
    strictPort: true,
    proxy: {
      "/rare": { target: "https://rare.market", changeOrigin: true, rewrite: (p) => p.replace(/^\/rare/, "") },
      // The Trade tab's API (the Rust trade_api command in the app).
      "/trade-api": { target: "https://rare.market", changeOrigin: true, rewrite: (p) => p.replace(/^\/trade-api/, "/v1/trade") },
    },
  },
  build: {
    target: "es2022",
    sourcemap: false,
    chunkSizeWarningLimit: 1500,
    // `isTauri` is a constant in a release build (src/lib/mockflag.ts): inline it across modules so the
    // mock branches fold away and the mock modules fall out of the bundle.
    rolldownOptions: { optimization: { inlineConst: { mode: "all", pass: 3 } } },
  },
  test: {
    // @ledgerhq ships ESM with extensionless imports: let vite resolve them (and apply the aliases)
    server: { deps: { inline: [/@ledgerhq\//, /@mysten\/ledgerjs-hw-app-sui/, /@ngraveio\/bc-ur/, /@keystonehq\//] } },
  },
};
});
