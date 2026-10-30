// Watches the display for a TV or monitor on the USB-C port and applies the saved UI size.
// gamescope switches the output and the launcher window follows it; this only says so.

import { useEffect } from "react";
import { native } from "../lib/bridge";
import { dockChange, scaleFactor, scaleOf, type UiScale } from "../lib/display";
import { useApp } from "../lib/store";
import type { DisplayState } from "../lib/types";

/** Writes the Large and Larger multiplier the stylesheet reads. */
export function applyUiScale(s: UiScale) {
  document.documentElement.style.setProperty("--ui-scale", String(scaleFactor(s)));
}

export function DockWatch() {
  useEffect(() => {
    void native.config().then((c) => applyUiScale(scaleOf(c))).catch(() => {});
    let prev: DisplayState | null = null;
    let stop = false;
    const tick = () => {
      void native
        .displayState()
        .then((d) => {
          if (stop) return;
          const msg = dockChange(prev, d);
          prev = d;
          if (msg) useApp.getState().toast(msg, "info", 5000);
        })
        .catch(() => {});
    };
    tick();
    const id = setInterval(tick, 3000);
    return () => {
      stop = true;
      clearInterval(id);
    };
  }, []);
  return null;
}
