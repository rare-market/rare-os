import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { native } from "./lib/bridge";
import { applyNetworkConfig } from "./lib/network";
import { setRenderMode } from "./lib/img";
import { bootState } from "./lib/state";
import "./styles/app.css";

// The saved network (Settings > Developer) is applied before anything reads the chain, the
// renderer is known before any art decodes (lib/img.ts), and the data the launcher showed last
// time is loaded from Rust's cache (lib/state.ts) so the first frame has it. One IPC call each,
// in parallel.
void Promise.all([
  native
    .config()
    .then(applyNetworkConfig)
    .catch(() => {}),
  native
    .renderMode()
    .then(setRenderMode)
    .catch(() => {}),
  bootState(),
]).then(() => {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
});
