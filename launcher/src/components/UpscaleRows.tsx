import { useEffect, useState } from "react";
import { native } from "../lib/bridge";
import type { Upscale } from "../lib/types";

const MODES: Array<{ id: Upscale["mode"]; label: string }> = [
  { id: "off", label: "Off" },
  { id: "fsr", label: "FSR" },
  { id: "nis", label: "NIS" },
];
const SIZES: Array<{ w: number; h: number; label: string }> = [
  { w: 0, h: 0, label: "1200p" },
  { w: 1280, h: 800, label: "800p" },
  { w: 960, h: 600, label: "600p" },
];
const SHARP = [4, 10, 16];

/** Upscaling rows for Quick Settings, same pattern as the Frame limit row. */
export function UpscaleRows() {
  const [u, setU] = useState<Upscale | null>(null);
  useEffect(() => {
    void native.upscale().then(setU).catch(() => {});
  }, []);
  if (!u) return null;
  const set = (patch: Partial<Upscale>) => {
    const next = { ...u, ...patch };
    setU(next);
    void native.setUpscale(next).catch(() => {});
  };
  const chip = (on: boolean) => `btn h-[2.25rem] min-w-[3.3rem] rounded-chip px-[0.8rem] text-small tabular-nums font-medium ${on ? "bg-fg text-bg" : "bg-l2 text-fg"}`;
  return (
    <>
      <div className="flex items-center gap-[0.5rem]" data-nav-group="qam-upscale">
        <span className="mr-auto text-small text-fg/85">Upscaling</span>
        {MODES.map((m) => (
          <button key={m.id} data-nav onClick={() => set({ mode: m.id })} className={chip(u.mode === m.id)}>
            {m.label}
          </button>
        ))}
      </div>
      {u.mode !== "off" && (
        <div className="flex items-center gap-[0.5rem]" data-nav-group="qam-upscale-sharp">
          <span className="mr-auto text-small text-fg/85">Sharpness</span>
          {SHARP.map((s, i) => (
            <button key={s} data-nav onClick={() => set({ sharpness: s })} className={chip(u.sharpness === s)}>
              {["Soft", "Medium", "Sharp"][i]}
            </button>
          ))}
        </div>
      )}
      <div className="flex items-center gap-[0.5rem]" data-nav-group="qam-render-size">
        <span className="mr-auto text-small text-fg/85">Render size</span>
        {SIZES.map((s) => (
          <button key={s.label} data-nav onClick={() => set({ width: s.w, height: s.h })} className={chip(u.width === s.w && u.height === s.h)}>
            {s.label}
          </button>
        ))}
      </div>
    </>
  );
}
