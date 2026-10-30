// Quick Settings: the device and nothing else. Opens from the Quick
// button over the launcher or over a game (the overlay's Quick mode). Order is Switch 2's:
// header with battery time, Wi-Fi / Bluetooth / Airplane / Do not disturb, brightness, volume,
// performance (More holds the power limit, boost, fan, frame limit, upscaling), capture, power,
// All settings. Notices, the SUI strip, Portfolio, Steam and Desktop mode live elsewhere.

import { useEffect, useRef, useState } from "react";
import { native } from "../lib/bridge";
import { useApp } from "../lib/store";
import type { FanMode, GamePowerView, HudLevel, Levels, Perf, Profile, Radios } from "../lib/types";
import { appByRunId } from "../apps/catalog";
import { streamTitle } from "../stream/state";
import { focusFirst } from "../nav/focus";
import { usePadHandler } from "../nav/input";
import { Hint, Toggle } from "../ui/controls";
import { useFocusedHints } from "../nav/hints";
import { useNotify } from "../notify/store";
import { Clock } from "./Chrome";
import { feedback } from "../nav/feedback";
import { UpscaleRows } from "./UpscaleRows";
import { BellOff, Bluetooth, Camera, ChevronDown, Images, Plane, Settings as SettingsIcon, Video, Wifi } from "lucide-react";
import { clipsNote, openShare, takeScreenshot, saveClip, useShare } from "../share";
import { IconBattery, IconBolt, IconMoon, IconPower, IconRestart, IconSun, IconVolume } from "./icons";

// The watts under each profile come from the launcher (system::profile_watts: the profile's
// limit within this device's range); these are the SuiPlay's until it answers.
const PROFILES: Array<{ id: Exclude<Profile, "custom">; label: string; watts: number }> = [
  { id: "quiet", label: "Quiet", watts: 10 },
  { id: "balanced", label: "Balanced", watts: 15 },
  { id: "performance", label: "Performance", watts: 25 },
];
// Off, 30, 45 and 60. 15 and 20 went: nobody plays at 15 fps.
// Clean divisors of the 60 Hz panel only (no VRR): 45 judders. Off, 15, 20, 30, 60.
const FPS = [0, 15, 20, 30, 60];
// Fan curves live in the launcher (system::fan_curve): Cool spins up early, Quiet stays low to about 65 C.
// The performance overlay levels (perfhud.rs draws them with mangoapp).
const HUDS: Array<{ id: HudLevel; label: string }> = [
  { id: "off", label: "Off" },
  { id: "fps", label: "FPS" },
  { id: "basic", label: "Basic" },
  { id: "full", label: "Full" },
];
const FANS: Array<{ id: FanMode; label: string }> = [
  { id: "default", label: "Default" },
  { id: "cool", label: "Cool" },
  { id: "quiet", label: "Quiet" },
];

function Slider({ value, min, max, step, onChange, icon, label, suffix = "%" }: { value: number; min: number; max: number; step: number; onChange: (v: number) => void; icon: React.ReactNode; label: string; suffix?: string }) {
  const pct = ((value - min) / (max - min)) * 100;
  const ref = useRef<HTMLDivElement>(null);
  const live = useRef({ value, onChange });
  live.current = { value, onChange };
  useEffect(() => {
    const el = ref.current;
    const h = (e: Event) => {
      const v = Math.min(max, Math.max(min, live.current.value + (e as CustomEvent<number>).detail * step));
      live.current.onChange(v);
    };
    el?.addEventListener("nudge", h);
    return () => el?.removeEventListener("nudge", h);
  }, [min, max, step]);
  return (
    <div ref={ref} data-nav data-slider data-hint-a="" tabIndex={0} className="rounded-card bg-l2 px-[0.95rem] py-[0.8rem]">
      <div className="mb-[0.6rem] flex items-center gap-[0.55rem] text-small text-fg/85">
        <span className="text-icon-2">{icon}</span>
        {label}
        <span className="ml-auto tabular-nums font-medium text-fg">{Math.round(value)}{suffix}</span>
      </div>
      <div
        className="relative h-[0.35rem] rounded-full bg-l4"
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          onChange(Math.round((min + ((e.clientX - r.left) / r.width) * (max - min)) / step) * step);
        }}
      >
        <div className="absolute inset-y-0 left-0 rounded-full bg-fg" style={{ width: `${pct}%` }} />
        <div className="absolute top-1/2 h-[1.05rem] w-[1.05rem] -translate-x-1/2 -translate-y-1/2 rounded-full bg-fg" style={{ left: `${pct}%` }} />
      </div>
    </div>
  );
}

let lastBrightnessSet = 0;

/** Dim after idle: Off or On (30% after 60 s with no input in the launcher). */
function DimIdleRow() {
  const [on, setOn] = useState<boolean | null>(null);
  useEffect(() => {
    void native.brightnessInfo().then((i) => setOn(i.dimIdle)).catch(() => {});
  }, []);
  if (on == null) return null;
  const set = (v: boolean) => {
    setOn(v);
    void native.setDimIdle(v).catch(() => {});
  };
  const chip = (a: boolean) => `btn h-[2.25rem] min-w-[3.3rem] rounded-chip px-[0.8rem] text-small tabular-nums font-medium ${a ? "bg-fg text-bg" : "bg-l2 text-fg"}`;
  return (
    <div className="flex items-center gap-[0.5rem]" data-nav-group="qam-dim-idle">
      <span className="mr-auto text-small text-fg/85">Dim after idle</span>
      <button data-nav onClick={() => set(false)} className={chip(!on)}>Off</button>
      <button data-nav onClick={() => set(true)} className={chip(on)}>60 s</button>
    </div>
  );
}

/** "3 h 10 min left", "Full in 40 min"; null when the battery doesn't say. */
export function batteryTimeLine(b: { percent: number; charging: boolean; minutes?: number | null }): string | null {
  if (b.charging && b.percent >= 100) return "Full";
  if (b.minutes == null) return null;
  const h = Math.floor(b.minutes / 60);
  const m = b.minutes % 60;
  const t = h > 0 ? `${h} h${m ? ` ${m} min` : ""}` : `${Math.max(1, m)} min`;
  return b.charging ? `Full in ${t}` : `${t} left`;
}

/** One of the four tiles: A toggles, X opens the details. */
function Tile({ icon, label, line, on, disabled, onClick, details }: { icon: React.ReactNode; label: string; line: string; on: boolean; disabled?: boolean; onClick: () => void; details?: boolean }) {
  return (
    <button
      data-nav
      data-hint-a={on ? "Turn off" : "Turn on"}
      data-hint-x={details ? "Details" : undefined}
      data-tile={details ? label : undefined}
      disabled={disabled}
      onClick={() => {
        feedback("toggle");
        onClick();
      }}
      className={`btn flex h-[5.9rem] min-w-0 flex-col items-center justify-center gap-[0.25rem] rounded-card px-[0.3rem] disabled:opacity-40 ${on ? "bg-fg text-bg" : "bg-l2 text-fg"}`}
    >
      {icon}
      <span className="max-w-full text-center text-small font-semibold leading-tight">{label}</span>
      <span className={`max-w-full truncate text-label ${on ? "text-bg/70" : "text-muted"}`}>{line}</span>
    </button>
  );
}

export function QuickSettings({ onClose }: { onClose?: () => void } = {}) {
  const appClose = useApp((s) => s.close);
  const close = onClose ?? appClose;
  const setTab = useApp((s) => s.setTab);
  const dnd = useNotify((s) => s.prefs.dnd);
  const setPrefs = useNotify((s) => s.setPrefs);
  const [radios, setRadios] = useState<Radios | null>(null);
  const [more, setMore] = useState(false);
  const hints = useFocusedHints();
  const net = useApp((s) => s.device?.wifi ?? null);
  const device = useApp((s) => s.device);
  const toast = useApp((s) => s.toast);
  const [perf, setPerf] = useState<Perf | null>(null);
  const [levels, setLevels] = useState<Levels | null>(null);
  const [tdp, setTdp] = useState(15);
  const [hud, setHudLevel] = useState<HudLevel>("off");
  // The running game: settings changed here are its own, and can be kept for next time.
  const [game, setGame] = useState<GamePowerView | null>(null);
  const gameName = useApp((s) => (game ? runTitle(game.id, s.library?.games) : null));
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void native.perf().then((p) => {
      setPerf(p);
      setTdp(p.tdp ?? 15);
    });
    void native.gamePower().then(setGame).catch(() => setGame(null));
    void native.levels().then(setLevels);
    void native.hud().then(setHudLevel).catch(() => {});
    void native.radios().then(setRadios).catch(() => {});
    setTimeout(() => root.current && focusFirst(root.current), 30);
    // The brightness keys move the backlight too; follow them while the panel is open.
    const id = setInterval(() => {
      if (Date.now() - lastBrightnessSet < 1500) return;
      void native.levels().then((n) => setLevels((l) => (l && n.brightness != null && n.brightness !== l.brightness ? { ...l, brightness: n.brightness } : l)));
    }, 1500);
    return () => clearInterval(id);
  }, []);

  const apply = async (profile: Profile, t?: number, fps?: number) => {
    try {
      await native.setPerf(profile, t, fps);
    } catch (e) {
      toast(String(e), "error");
    }
    const p = await native.perf();
    setPerf({ ...p, profile, fpsLimit: fps === undefined ? p.fpsLimit : fps || null });
    if (p.tdp) setTdp(p.tdp);
    refreshGame();
  };

  // A change in a game is remembered for it, so "Use for {game}" turns on by itself.
  const refreshGame = () => void native.gamePower().then(setGame).catch(() => {});

  const setHud = async (level: HudLevel) => {
    setHudLevel(level);
    try {
      await native.setHud(level);
    } catch (e) {
      toast(String(e), "error");
    }
    refreshGame();
  };

  const setBoost = async (on: boolean) => {
    setPerf((p) => (p ? { ...p, cpuBoost: on } : p));
    try {
      await native.setCpuBoost(on);
    } catch (e) {
      toast(String(e), "error");
    }
    setPerf(await native.perf());
    refreshGame();
  };

  const saveForGame = async (save: boolean) => {
    setGame((g) => (g ? { ...g, saved: save } : g));
    try {
      await native.saveGamePower(save);
    } catch (e) {
      toast(String(e), "error");
    }
    setGame(await native.gamePower().catch(() => null));
  };

  const setFan = async (mode: FanMode) => {
    setPerf((p) => (p ? { ...p, fan: mode } : p));
    try {
      await native.setFan(mode);
    } catch (e) {
      toast(String(e), "error");
    }
    setPerf(await native.perf());
  };

  const setRadio = async (kind: "wifi" | "bluetooth" | "airplane", on: boolean) => {
    setRadios((r) => (r ? { ...r, [kind]: on } : r));
    try {
      setRadios(await native.setRadio(kind, on));
    } catch (e) {
      console.warn("radio", e);
      toast(String(e).replace(/^Error: /, ""), "error");
      setRadios(await native.radios().catch(() => null));
    }
  };

  // Sliders: left/right while focused. X on a tile opens its details in Settings.
  usePadHandler((b, _repeat) => {
    if (b === "b" || b === "qam" || b === "start") {
      close();
      return true;
    }
    if (b === "x") {
      const tile = (document.activeElement as HTMLElement | null)?.dataset.tile;
      if (tile) {
        close();
        setTab("settings");
      }
      return true;
    }
    const el = document.activeElement as HTMLElement | null;
    if (el?.dataset.slider !== undefined && (b === "left" || b === "right")) {
      el.dispatchEvent(new CustomEvent("nudge", { detail: b === "left" ? -1 : 1 }));
      return true;
    }
    return false;
  });

  const setBrightness = (v: number) => {
    lastBrightnessSet = Date.now();
    setLevels((l) => (l ? { ...l, brightness: v } : l));
    void native.setLevels(v, undefined).catch((e) => toast(String(e), "error"));
  };
  const setVolume = (v: number) => {
    setLevels((l) => (l ? { ...l, volume: v } : l));
    void native.setLevels(undefined, v).catch((e) => toast(String(e), "error"));
  };

  const bat = device?.battery ?? null;
  const batLine = bat ? batteryTimeLine(bat) : null;
  const wifiLine = radios?.wifi === false ? "Off" : net ?? "On";
  const btLine = radios?.bluetooth === false ? "Off" : "On";
  const profile = perf?.profile;

  return (
    <div className="fixed inset-0 z-40 flex justify-end bg-black/40" onClick={close}>
      <div
        ref={root}
        data-nav-scope
        onClick={(e) => e.stopPropagation()}
        className="slide-in-right flex h-full w-[27rem] flex-col gap-[1.1rem] overflow-y-auto border-l border-border-light bg-l1 px-[1.4rem] py-[1.4rem] shadow-[-2rem_0_4rem_rgb(0_0_0/0.45)]"
      >
        <div className="flex items-center gap-[0.75rem]">
          <span className="text-row font-semibold">Quick settings</span>
          <span className="ml-auto flex items-center gap-[0.9rem] text-small text-fg/80">
            {bat && (
              <span className="flex items-center gap-[0.35rem]">
                <IconBattery size="1.3rem" percent={bat.percent} charging={bat.charging} />
                <span className="tabular-nums">{bat.percent}%</span>
                {batLine && <span className="text-muted">{batLine}</span>}
              </span>
            )}
            <Clock />
          </span>
        </div>

        <section className="grid grid-cols-4 gap-[0.5rem]" data-nav-group="qam-tiles">
          <Tile icon={<Wifi size="1.4rem" />} label="Wi-Fi" line={wifiLine} on={radios?.wifi === true} disabled={!radios || radios.wifi == null} details onClick={() => void setRadio("wifi", !radios?.wifi)} />
          <Tile icon={<Bluetooth size="1.4rem" />} label="Bluetooth" line={btLine} on={radios?.bluetooth === true} disabled={!radios || radios.bluetooth == null} details onClick={() => void setRadio("bluetooth", !radios?.bluetooth)} />
          <Tile icon={<Plane size="1.4rem" />} label="Airplane" line={radios?.airplane ? "On" : "Off"} on={!!radios?.airplane} disabled={!radios} onClick={() => void setRadio("airplane", !radios?.airplane)} />
          <Tile icon={<BellOff size="1.4rem" />} label="Do not disturb" line={dnd ? "On" : "Off"} on={dnd} onClick={() => setPrefs({ dnd: !dnd })} />
        </section>

        <section className="flex flex-col gap-[0.6rem]">
          {levels?.brightness != null && <Slider value={levels.brightness} min={0} max={100} step={5} label="Brightness" icon={<IconSun size="1rem" />} onChange={setBrightness} />}
          {levels?.brightness != null && levels.brightness < 10 && (
            <div className="text-meta text-muted">Night dim. Darker to look at, but it does not save battery.</div>
          )}
          {levels?.brightness != null && <DimIdleRow />}
          {levels?.volume != null && <Slider value={levels.volume} min={0} max={100} step={5} label="Volume" icon={<IconVolume size="1rem" />} onChange={setVolume} />}
        </section>

        <section className="flex flex-col gap-[0.6rem]">
          <div className="text-label font-medium text-secondary">Performance</div>
          <div className="grid grid-cols-3 gap-[0.5rem]" data-nav-group="qam-profile">
            {PROFILES.map((p) => (
              <button
                key={p.id}
                data-nav
                data-nav-default={profile === p.id ? "" : undefined}
                onClick={() => void apply(p.id)}
                className={`btn flex flex-col items-start rounded-card px-[0.85rem] py-[0.7rem] text-left ${profile === p.id ? "bg-fg text-bg" : "bg-l2 text-fg"}`}
              >
                <span className="text-small font-semibold">{p.label}</span>
                <span className={`tabular-nums text-label ${profile === p.id ? "text-bg/70" : "text-muted"}`}>{Math.round(perf?.profileWatts?.[p.id] ?? p.watts)} W</span>
              </button>
            ))}
          </div>
          <div className="flex items-center gap-[0.5rem]" data-nav-group="qam-hud">
            <span className="mr-auto text-small text-fg/85">Performance overlay</span>
            {HUDS.map((h) => (
              <button
                key={h.id}
                data-nav
                onClick={() => void setHud(h.id)}
                className={`btn h-[2.25rem] min-w-[3.3rem] rounded-chip px-[0.8rem] text-small font-medium ${hud === h.id ? "bg-fg text-bg" : "bg-l2 text-fg"}`}
              >
                {h.label}
              </button>
            ))}
          </div>
          {game && (
            <button
              data-nav
              data-hint-a={game.saved ? "Turn off" : "Turn on"}
              onClick={() => {
                feedback("toggle");
                void saveForGame(!game.saved);
              }}
              className="flex items-center gap-[0.75rem] rounded-card bg-l2 px-[0.95rem] py-[0.8rem] text-left"
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate text-small text-fg/85">Use for {gameName ?? "this game"}</span>
                <span className="block truncate text-label text-muted">{game.saved ? "Starts with these settings, overlay included" : "Starts on Balanced"}</span>
              </span>
              <Toggle on={game.saved} />
            </button>
          )}
          <button
            data-nav
            data-hint-a={more ? "Show less" : "More"}
            aria-expanded={more}
            onClick={() => setMore((m) => !m)}
            className="btn flex h-[2.4rem] items-center justify-between rounded-card bg-l2 px-[0.95rem] text-small font-medium"
          >
            More
            <ChevronDown size="1rem" className={`transition-transform duration-150 ${more ? "rotate-180" : ""}`} />
          </button>
          {more && (
            <div className="flex flex-col gap-[0.6rem]">
              {perf && (
                <Slider value={tdp} min={perf.tdpMin} max={perf.tdpMax} step={1} suffix=" W" label="Power limit (TDP)" icon={<IconBolt size="1rem" />} onChange={(v) => { setTdp(v); void apply("custom", v); }} />
              )}
              {perf?.cpuBoost != null && (
                <button
                  data-nav
                  data-hint-a={perf.cpuBoost ? "Turn off" : "Turn on"}
                  onClick={() => {
                    feedback("toggle");
                    void setBoost(!perf.cpuBoost);
                  }}
                  className="flex items-center gap-[0.75rem] rounded-card bg-l2 px-[0.95rem] py-[0.8rem] text-left"
                >
                  <span className="min-w-0 flex-1">
                    <span className="block text-small text-fg/85">CPU boost</span>
                    <span className="block text-label text-muted">Faster CPU, more heat and battery</span>
                  </span>
                  <Toggle on={perf.cpuBoost} />
                </button>
              )}
              {perf?.fan != null && (
                <div className="flex items-center gap-[0.5rem]" data-nav-group="qam-fan">
                  <span className="mr-auto text-small text-fg/85">Fan</span>
                  {FANS.map((f) => (
                    <button
                      key={f.id}
                      data-nav
                      onClick={() => void setFan(f.id)}
                      className={`btn h-[2.25rem] min-w-[3.3rem] rounded-chip px-[0.8rem] text-small font-medium ${perf.fan === f.id ? "bg-fg text-bg" : "bg-l2 text-fg"}`}
                    >
                      {f.label}
                    </button>
                  ))}
                </div>
              )}
              <div className="flex items-center gap-[0.5rem]" data-nav-group="qam-fps">
                <span className="mr-auto text-small text-fg/85">Frame limit</span>
                {FPS.map((f) => (
                  <button
                    key={f}
                    data-nav
                    onClick={() => void apply(perf?.profile ?? "balanced", undefined, f)}
                    className={`btn h-[2.25rem] min-w-[3.3rem] rounded-chip px-[0.8rem] text-small tabular-nums font-medium ${(perf?.fpsLimit ?? 0) === f ? "bg-fg text-bg" : "bg-l2 text-fg"}`}
                  >
                    {f === 0 ? "Off" : f}
                  </button>
                ))}
              </div>
              <UpscaleRows />
            </div>
          )}
          {perf?.note && <div className="text-meta text-muted">{perf.note}</div>}
        </section>

        <CaptureSection />

        <section className="flex flex-col gap-[0.6rem]">
          <div className="text-label font-medium text-secondary">Power</div>
          <div className="grid grid-cols-3 gap-[0.5rem]" data-nav-group="qam-power">
            <QBtn icon={<IconMoon size="1.15rem" />} label="Sleep" onClick={() => void native.power("suspend")} />
            <QBtn icon={<IconRestart size="1.15rem" />} label="Restart" onClick={() => void native.power("reboot")} />
            <QBtn icon={<IconPower size="1.15rem" />} label="Turn off" onClick={() => void native.power("poweroff")} />
          </div>
        </section>

        <section className="mt-auto grid grid-cols-1">
          <QBtn
            icon={<SettingsIcon size="1.15rem" />}
            label="All settings"
            onClick={() => {
              close();
              setTab("settings");
            }}
            wide
          />
        </section>
        <div className="flex items-center justify-end gap-[1.1rem] text-small text-fg/70">
          <Hint b="a">Select</Hint>
          {hints.x && <Hint b="x">{hints.x}</Hint>}
          <Hint b="b">Close</Hint>
        </div>
      </div>
    </div>
  );
}

/** A running id's name: a library game, an app, a PC stream. */
function runTitle(id: string, games: Array<{ id: string; title: string }> | undefined): string | null {
  return games?.find((g) => g.id === id)?.title ?? appByRunId(id)?.name ?? streamTitle(id);
}

function QBtn({ icon, label, onClick, wide, disabled, hint }: { icon: React.ReactNode; label: string; onClick: () => void; wide?: boolean; disabled?: boolean; hint?: string }) {
  return (
    <button data-nav data-hint-a={hint} disabled={disabled} onClick={onClick} className={`flex h-[2.9rem] items-center gap-[0.6rem] rounded-card bg-l2 px-[0.9rem] text-small font-medium disabled:opacity-40 ${wide ? "col-span-2" : ""}`}>
      {icon}
      {label}
    </button>
  );
}

/** Screenshot, the last 30 seconds, and the Share screen. */
function CaptureSection() {
  const status = useShare((s) => s.status);
  const taking = useShare((s) => s.taking);
  const fresh = useShare((s) => (s.captures ?? []).filter((c) => c.createdMs > s.seenAt).length);
  useEffect(() => void useShare.getState().refresh(), []);
  const note = clipsNote(status);
  return (
    <section className="flex flex-col gap-[0.6rem]">
      <div className="flex items-center text-label font-medium text-secondary">
        Capture
        {fresh > 0 && <span className="ml-auto font-normal text-muted">{fresh} new</span>}
      </div>
      <div className="grid grid-cols-2 gap-[0.5rem]" data-nav-group="qam-share">
        <QBtn icon={<Camera size="1.15rem" />} label="Screenshot" hint="Take a screenshot" disabled={!!taking || status?.screenshot.available === false} onClick={() => void takeScreenshot({ closeFirst: true })} />
        <QBtn icon={<Video size="1.15rem" />} label="Save clip" hint="Save the last 30 seconds" disabled={!!taking || !status?.clips.recording} onClick={() => void saveClip()} />
        <QBtn icon={<Images size="1.15rem" />} label="Open Share" hint="Open Share" onClick={openShare} wide />
      </div>
      {note && <div className="text-meta text-muted">{note}</div>}
    </section>
  );
}
