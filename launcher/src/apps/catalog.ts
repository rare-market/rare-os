// The Apps catalog (catalog.json, shared with src-tauri/src/apps/catalog.rs): Telegram, X,
// Netflix, YouTube, Discord and Chrome as console-style media tiles. `statusFor` mirrors the
// native side's planning so the browser build (dev:ui, screenshots) shows the same states.

import CATALOG from "./catalog.json";
import type { PadButton } from "../lib/types";

export type Controller = "focus" | "pointer" | "gamepad";
export type WayKind = "webview" | "chromeApp" | "chrome" | "flatpak" | "exec" | "steam";

export interface Way {
  webview?: string;
  chromeApp?: string;
  chrome?: boolean;
  flatpak?: string;
  exec?: string[];
  /** A steam:// URL: Steam's own UI, an app like the others (Home comes back to Rare). */
  steam?: string;
  label: string;
  controller?: Controller;
  back?: "escape" | "alt+left";
  userAgent?: string;
}

export interface AppEntry {
  id: string;
  name: string;
  icon: string;
  about: string;
  drm?: boolean;
  ways: Way[];
}

/** Something to install first, with the exact one-line command (native `Offer`). */
export interface Offer {
  flatpak: string;
  name: string;
  command: string;
}

/** An app on this device (native `Status`). */
export interface AppStatus {
  id: string;
  /** The way A opens, or null when something must be installed first. */
  way: number | null;
  opensIn: string | null;
  otherWays: number[];
  offer: Offer | null;
  controller: Controller | null;
}

export const APPS: AppEntry[] = (CATALOG as unknown as { apps: AppEntry[] }).apps;

export const appById = (id: string) => APPS.find((a) => a.id === id);
/** Apps run as "app:<id>" in the launcher's running list. */
export const runId = (id: string) => `app:${id}`;
export const appByRunId = (rid: string) => (rid.startsWith("app:") ? appById(rid.slice(4)) : undefined);

export function wayKind(w: Way): WayKind | null {
  const set: WayKind[] = [];
  if (w.webview) set.push("webview");
  if (w.chromeApp) set.push("chromeApp");
  if (w.chrome === true) set.push("chrome");
  if (w.flatpak) set.push("flatpak");
  if (w.exec?.length) set.push("exec");
  if (w.steam) set.push("steam");
  return set.length === 1 ? set[0] : null;
}

export const controllerOf = (w: Way): Controller => w.controller ?? (wayKind(w) === "webview" ? "focus" : wayKind(w) === "steam" ? "gamepad" : "pointer");

/** Browsers, best first (native BROWSERS). drm: has Widevine. */
const BROWSERS: Array<{ name: string; flatpak: string; drm: boolean }> = [
  { name: "Google Chrome", flatpak: "com.google.Chrome", drm: true },
  { name: "Microsoft Edge", flatpak: "com.microsoft.Edge", drm: true },
  { name: "Chromium", flatpak: "org.chromium.Chromium", drm: false },
  { name: "Brave", flatpak: "com.brave.Browser", drm: false },
];

export function installCommand(flatpak: string, userFlathub = true) {
  const install = `flatpak install --user -y flathub ${flatpak}`;
  return userFlathub ? install : `flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo && ${install}`;
}

/** The same answer as the native `status`, from a list of installed Flatpaks. */
export function statusFor(app: AppEntry, installed: string[], userFlathub = true): AppStatus {
  const browser = BROWSERS.find((b) => (!app.drm || b.drm) && installed.includes(b.flatpak));
  const need = (w: Way): string | null | true => {
    const k = wayKind(w);
    if (k === "webview" || k === "steam") return true;
    if (k === "chromeApp" || k === "chrome") return browser ? true : "com.google.Chrome";
    if (k === "flatpak") return installed.includes(w.flatpak!) ? true : w.flatpak!;
    return null;
  };
  const ready = app.ways.map((w, i) => [i, need(w)] as const).filter(([, n]) => n === true).map(([i]) => i);
  const way = ready.length ? ready[0] : null;
  const first = need(app.ways[0]);
  const offer =
    typeof first === "string"
      ? { flatpak: first, name: BROWSERS.find((b) => b.flatpak === first)?.name ?? app.ways.find((w) => w.flatpak === first)?.label ?? first, command: installCommand(first, userFlathub) }
      : null;
  const w = way == null ? null : app.ways[way];
  const k = w ? wayKind(w) : null;
  return {
    id: app.id,
    way,
    opensIn: w ? ((k === "chromeApp" || k === "chrome") && browser ? browser.name : w.label) : null,
    otherWays: ready.slice(1),
    offer,
    controller: w ? controllerOf(w) : null,
  };
}

/** The controller layout an app gets, for its page. `b` is a pad button, or a stick's name. */
export interface ControlRow {
  b: PadButton | "Right stick" | "Left stick" | "D-pad";
  does: string;
}

export function controlsFor(controller: Controller, way?: Way): ControlRow[] {
  const k = way ? wayKind(way) : null;
  const back = way?.back ?? (k === "chromeApp" || k === "chrome" ? "alt+left" : "escape");
  if (controller === "pointer")
    return [
      { b: "Right stick", does: "Moves the pointer" },
      { b: "a", does: "Click (RT too)" },
      { b: "lt", does: "Right click" },
      { b: "Left stick", does: "Scrolls" },
      { b: "D-pad", does: "Arrow keys" },
      { b: "b", does: back === "alt+left" ? "Back a page" : "Back (Escape)" },
      { b: "x", does: "Space: play or pause" },
      { b: "y", does: "On-screen keyboard" },
      { b: "start", does: "Enter" },
    ];
  if (controller === "focus")
    return [
      { b: "D-pad", does: "Moves between buttons" },
      { b: "a", does: "Select" },
      { b: "b", does: "Back (Escape)" },
      { b: "select", does: "On-screen keyboard" },
      { b: "Right stick", does: "Moves a pointer" },
    ];
  return [{ b: "D-pad", does: "The app reads your controller itself" }];
}
