// PC streaming: the pure parts of the UI state (tested in logic.test.ts). Which PC shows on
// Home, what a tile says, what Play does, and the quality presets for the SuiPlay.

import type { Codec, Decode, Host, StreamApp, StreamSettings } from "./types";

/** Streams run as "stream:<host uuid>:<app id>" in the launcher's running list. */
export const runId = (uuid: string, app: number) => `stream:${uuid}:${app}`;

export function parseRunId(rid: string): { uuid: string; app: number } | null {
  if (!rid.startsWith("stream:")) return null;
  const rest = rid.slice(7);
  const i = rest.lastIndexOf(":");
  const app = Number(rest.slice(i + 1));
  if (i <= 0 || !Number.isInteger(app)) return null;
  return { uuid: rest.slice(0, i), app };
}

/** The PC Home's "On your PC" row shows: the first paired one that answers. */
export const homeHost = (hosts: Host[]) => hosts.find((h) => h.paired && h.online) ?? null;

/** Every game on every paired PC, for Library > Streaming. */
export function pcGames(hosts: Host[], apps: Record<string, StreamApp[]>): Array<{ host: Host; app: StreamApp }> {
  return hosts.filter((h) => h.paired).flatMap((host) => (apps[host.uuid] ?? []).map((app) => ({ host, app })));
}

/** Games with box art first (Sunshine lists Desktop and Steam Big Picture first), order kept otherwise. */
export const ordered = (apps: StreamApp[]) => [...apps.filter((a) => a.art), ...apps.filter((a) => !a.art)];

export type PlayState =
  /** Streaming to this SuiPlay now. */
  | "playing"
  /** Running on the PC with no stream: Play picks it back up. */
  | "on-pc"
  /** The PC runs a different game: Play quits it first (asked). */
  | "busy"
  | "ready"
  /** Not answering, but Wake-on-LAN can start it. */
  | "asleep"
  | "offline";

export function playState(host: Host, app: StreamApp, running: string[]): PlayState {
  if (running.includes(runId(host.uuid, app.id))) return "playing";
  if (!host.online) return host.canWake ? "asleep" : "offline";
  if (host.currentGame === app.id) return "on-pc";
  if (host.currentGame != null) return "busy";
  return "ready";
}

/** The second line under a cover. */
export function tileLine(host: Host, state: PlayState): string {
  switch (state) {
    case "playing":
      return "Playing";
    case "on-pc":
      return "Running on your PC";
    case "asleep":
      return `${host.name} is asleep`;
    case "offline":
      return `${host.name} is off`;
    default:
      return host.name;
  }
}

// ServerCodecModeSupport bits (moonlight-common-c).
const SCM_H264 = 0x1;
const SCM_HEVC = 0x100;
const SCM_AV1 = 0x10000 | 0x20000;

/** The codec Auto lands on (same rule as the native `pick_codec`); null: Moonlight decides. */
export function autoCodec(decode: Decode | null, hostCodecs: number | null): Exclude<Codec, "auto"> | null {
  if (!decode || hostCodecs == null) return null;
  if (decode.av1 && hostCodecs & SCM_AV1) return "av1";
  if (decode.hevc && hostCodecs & SCM_HEVC) return "hevc";
  if (hostCodecs & SCM_H264 || hostCodecs === 0) return "h264";
  return null;
}

export const CODEC_LABEL: Record<Codec, string> = { auto: "Auto", av1: "AV1", hevc: "HEVC", h264: "H.264" };

export const RESOLUTIONS = [
  { value: "1920x1200", label: "1920 x 1200" },
  { value: "1280x800", label: "1280 x 800" },
];
export const BITRATES = [10, 20, 30, 50].map((m) => ({ value: String(m), label: `${m} Mbps` }));
export const CODECS: Array<{ value: Codec; label: string }> = (["auto", "av1", "hevc", "h264"] as Codec[]).map((c) => ({ value: c, label: CODEC_LABEL[c] }));

export const DEFAULT_SETTINGS: StreamSettings = { resolution: "1920x1200", fps: 60, bitrateMbps: 20, codec: "auto" };

/** A on a setting row: the next value, wrapping. */
export function cycle<T>(values: T[], cur: T): T {
  const i = values.indexOf(cur);
  return values[(i + 1) % values.length];
}

/** One line for the quality the next stream uses: "1920 x 1200, 60 fps, 20 Mbps, AV1". */
export function qualityLine(s: StreamSettings, decode: Decode | null, hostCodecs: number | null): string {
  const res = RESOLUTIONS.find((r) => r.value === s.resolution)?.label ?? s.resolution;
  const codec = s.codec === "auto" ? autoCodec(decode, hostCodecs) : s.codec;
  return [res, `${s.fps} fps`, `${s.bitrateMbps} Mbps`, codec ? CODEC_LABEL[codec] : null].filter(Boolean).join(", ");
}

/** "192.168.0.20" style check for Add by address (an IPv4, IPv6 or a .local name, maybe a port). */
export function validAddress(a: string): boolean {
  const s = a.trim();
  return s.length > 0 && s.length <= 253 && /^[A-Za-z0-9.:\-[\]%]+$/.test(s);
}
