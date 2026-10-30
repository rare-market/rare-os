// Mock and demo code (the browser stand-in for the native side, ?inv=mock, ?notify=mock, ?friends=mock,
// ?storedemo, devsign, "Demo wallet" ...) exists for dev (`npm run dev:ui`), tests (vitest) and screenshots only.
//
// A release build (`npm run build`, tauri build) never has it: MOCK_OK is false, `isTauri` is the constant true,
// every `isTauri ? native : mock` branch folds to native, and the mock modules are dropped from the bundle
// (vite.config.ts marks them side-effect free; scripts/check-no-demo.mjs fails the build if any demo string is left).
// A failed native call therefore shows the screen's own error or empty state, never mock data.
//
// To build a bundle WITH mocks for `vite preview` screenshots: VITE_MOCK=1 npm run build:ui
// Set by vite.config.ts `define` as a literal (true for dev, vitest and VITE_MOCK=1; false for a release build).
export const MOCK_OK: boolean = __MOCK_OK__;

// isTauri (lib/bridge.ts, wallet/native.ts) is `const isTauri = __RARE_NATIVE__`, defined in vite.config.ts.
