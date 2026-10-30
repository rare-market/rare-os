// A mocked gaming PC for the browser build (dev:ui, screenshots): one paired PC running
// Sunshine with an RTX 40 (AV1), and one unpaired PC on the network. URL switches:
//   ?stream=none        no PCs yet
//   ?stream=nomoonlight Moonlight not installed
//   ?stream=asleep      the paired PC is asleep (Wake-on-LAN)
//   ?streambusy         the PC is running another game
// Box art is the games' own Steam library covers, standing in for what Sunshine serves.

import type { Host, PairView, Status, StreamApp, StreamSettings } from "./types";
import { DEFAULT_SETTINGS } from "./logic";

const params = typeof location !== "undefined" ? new URLSearchParams(location.search) : new URLSearchParams();
const mode = params.get("stream") ?? "";
const cover = (id: number) => `https://cdn.cloudflare.steamstatic.com/steam/apps/${id}/library_600x900.jpg`;

export const PC = "5F6A2B10-71C3-4E7D-9A55-0B1C2D3E4F50";
const DEN = "B7E01C44-2D9A-4F10-8C3B-6A5D4E3F2A10";

const APPS: StreamApp[] = [
  { id: 1091500, name: "Cyberpunk 2077", art: cover(1091500), hdr: true },
  { id: 1245620, name: "ELDEN RING", art: cover(1245620), hdr: false },
  { id: 1086940, name: "Baldur's Gate 3", art: cover(1086940), hdr: false },
  { id: 1551360, name: "Forza Horizon 5", art: cover(1551360), hdr: true },
  { id: 1174180, name: "Red Dead Redemption 2", art: cover(1174180), hdr: false },
  { id: 2358720, name: "Black Myth: Wukong", art: cover(2358720), hdr: true },
  { id: 881448767, name: "Desktop", art: null, hdr: false },
  { id: 1093255277, name: "Steam Big Picture", art: null, hdr: false },
];

let settings: StreamSettings = { ...DEFAULT_SETTINGS };
let installed = mode !== "nomoonlight";
const hosts: Host[] =
  mode === "none"
    ? []
    : [
        {
          uuid: PC,
          name: "Gaming-PC",
          address: "192.168.0.20",
          paired: true,
          online: mode !== "asleep",
          currentGame: params.has("streambusy") ? 1245620 : null,
          canWake: true,
          software: "Sunshine",
          codecs: 0x1 | 0x100 | 0x200 | 0x10000 | 0x20000,
        },
      ];
export const FOUND: Host = { uuid: DEN, name: "Living-room", address: "192.168.0.31", paired: false, online: true, currentGame: null, canWake: true, software: "Sunshine", codecs: 0x1 | 0x100 };

const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function status(): Promise<Status> {
  return {
    moonlight: installed ? "flatpak" : null,
    offer: installed ? null : { flatpak: "com.moonlight_stream.Moonlight", name: "Moonlight", command: "flatpak install --user -y flathub com.moonlight_stream.Moonlight" },
    decode: { av1: true, hevc: true, h264: true },
    settings,
    hosts: hosts.map((h) => ({ ...h })),
    apps: Object.fromEntries(hosts.filter((h) => h.paired).map((h) => [h.uuid, APPS])),
  };
}

export const refresh = async () => (await pause(300), hosts.map((h) => ({ ...h })));

export async function discover(): Promise<Host[]> {
  await pause(1600);
  if (!hosts.some((h) => h.uuid === DEN)) hosts.push({ ...FOUND });
  return hosts.map((h) => ({ ...h }));
}

export async function add(address: string): Promise<Host> {
  await pause(700);
  if (address !== FOUND.address && address !== "192.168.0.20") throw new Error(`No PC answered at ${address}. Check Sunshine is running on it.`);
  const h = hosts.find((x) => x.address === address) ?? (hosts.push({ ...FOUND }), FOUND);
  return { ...h };
}

let pairing: PairView | null = null;
let pairTimer: ReturnType<typeof setTimeout> | null = null;

export async function pairStart(address: string): Promise<PairView> {
  const h = hosts.find((x) => x.address === address) ?? FOUND;
  pairing = { uuid: h.uuid, name: h.name, address, pin: "4821", phase: "waiting", error: null, software: h.software ?? "Sunshine" };
  // Screenshots hold the PIN; otherwise the PC "gets" it after a while.
  if (!params.has("streampair")) {
    pairTimer = setTimeout(() => {
      if (!pairing) return;
      pairing = { ...pairing, phase: "paired" };
      const host = hosts.find((x) => x.uuid === pairing!.uuid);
      if (host) host.paired = true;
    }, 8000);
  }
  return { ...pairing };
}

export const pairPoll = async () => (pairing ? { ...pairing } : null);
export async function pairCancel() {
  if (pairTimer) clearTimeout(pairTimer);
  pairing = null;
}

export const apps = async (uuid: string) => (await pause(500), uuid === PC || hosts.find((h) => h.uuid === uuid)?.paired ? APPS : []);

export async function wake(uuid: string) {
  await pause(400);
  setTimeout(() => {
    const h = hosts.find((x) => x.uuid === uuid);
    if (h) h.online = true;
  }, 3000);
}

export async function quitHost(uuid: string) {
  const h = hosts.find((x) => x.uuid === uuid);
  if (h) h.currentGame = null;
}

export async function setSettings(s: StreamSettings) {
  settings = s;
}

export async function markInstalled() {
  installed = true;
}
