// Settings > Display: what the screen is (built in, or docked to a TV) and the UI size.
// Rows only; DisplaySection puts them in its panel.

import { useEffect, useState } from "react";
import { applyUiScale } from "../../components/DockWatch";
import { native } from "../../lib/bridge";
import { dockedLine, scaleOf, sizeLabel, UI_SCALES, scaleLabel, type UiScale } from "../../lib/display";
import { useApp } from "../../lib/store";
import type { DisplayState } from "../../lib/types";
import { Dot } from "../../ui";
import { patchConfig } from "./config";
import { ChoiceRow, SettingRow } from "./rows";
import { failed } from "../../lib/fail";

export function DisplayRows() {
  const toast = useApp((s) => s.toast);
  const [state, setState] = useState<DisplayState | null>(null);
  const [scale, setScale] = useState<UiScale>("auto");
  const [size, setSize] = useState(sizeLabel(window.innerWidth, window.innerHeight));

  useEffect(() => {
    let live = true;
    const read = () => {
      void native.displayState().then((d) => live && setState(d)).catch(() => {});
      setSize(sizeLabel(window.innerWidth, window.innerHeight));
    };
    read();
    void native.config().then((c) => live && setScale(scaleOf(c)));
    const id = setInterval(read, 3000);
    return () => {
      live = false;
      clearInterval(id);
    };
  }, []);

  const docked = state?.docked ?? false;
  const external = (state?.outputs ?? []).filter((o) => !o.internal && o.connected);
  return (
    <>
      <SettingRow
        id="docked"
        title={
          <span className="inline-flex items-center gap-[0.6rem]">
            <Dot tone={docked ? "primary" : "muted"} />
            {dockedLine(state) || "Built-in screen"}
          </span>
        }
        body={
          docked
            ? `The picture and the sound are on the display. Showing at ${size}. Unplug the cable to come back to the built-in screen.`
            : external.length
              ? "A display is plugged in and will take over in a moment."
              : "Plug a TV or monitor into the USB-C port and the picture and the sound move to it."
        }
      />
      <ChoiceRow
        id="ui-size"
        title="UI size"
        body="Makes text and buttons bigger on top of the automatic size. Kept after a restart."
        value={scale}
        choices={UI_SCALES.map((v) => ({ value: v, label: scaleLabel(v) }))}
        onPick={(v) => {
          setScale(v);
          applyUiScale(v);
          void patchConfig({ uiScale: v === "auto" ? null : v }).catch((e) => toast(failed("Couldn't change the display size.", e), "error"));
        }}
      />
    </>
  );
}
