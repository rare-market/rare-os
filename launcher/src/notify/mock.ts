// Mock notify API for browser development and screenshots (`?notify=mock`). Sample notices in
// the contract's shape: a few older ones for `recent`, then a live one shortly after the stream
// opens (`?notifyAt=ms`, default 2500) and another every 20 s. Price alerts live in local storage.

import type { StreamConn, StreamHandlers } from "./api";
import type { Notice, PriceAlert } from "./types";

const POPKIN = "0x89d249c6a43ff4490902247a950e2e69ba61b74089151166169cfe3fbfea8611";
const art = (id: string) => `https://storage.claynosaurz.com/popkins/images/${id}`;
const ME = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const SUI = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
const DIGEST = "5QDVv7YPKSPiLA959P8T1nW4phnfHHLg9XuXMP6umQRP";
const SUI_PERP = "0xa97dacdd972a0414363290ec5752cb93ac47443c3e7710df145cc7f337575002";

let seq = 1000;
const params = typeof location !== "undefined" ? new URLSearchParams(location.search) : new URLSearchParams();

type Sample = Omit<Notice, "id" | "cpTs" | "createdAt" | "sentAt">;

const base = { addr: ME, body: null, imageUrl: null, amount: null, item: null, digest: DIGEST };
const popkin = (n: number) => ({ id: POPKIN, name: `Popkins #${n}`, imageUrl: art(POPKIN), collection: { slug: "popkins", name: "Popkins" } });

const LIVE: Sample[] = [
  {
    ...base,
    kind: "sold",
    title: "Popkins #412 sold for 70 SUI",
    body: "Bought by 0x12ab...9f3c on Tradeport",
    imageUrl: art(POPKIN),
    amount: { raw: "70000000000", decimals: 9, symbol: "SUI", coinType: SUI, usd: 63.42 },
    item: popkin(412),
    action: { kind: "item", itemId: POPKIN, slug: "popkins" },
  },
  {
    ...base,
    kind: "offer_received",
    title: "New offer of 64 SUI on Popkins #412",
    body: "From 0x7c1d...a210, ends in 3 days",
    imageUrl: art(POPKIN),
    item: popkin(412),
    action: { kind: "item", itemId: POPKIN, slug: "popkins" },
  },
  {
    ...base,
    kind: "price_alert",
    addr: null,
    digest: null,
    title: "SUI is above $1.00",
    body: "Your alert at $1.00 was hit. Now $1.01.",
    action: { kind: "coin", coinType: SUI },
  },
  {
    ...base,
    kind: "perp_liquidation",
    digest: null,
    title: "SUI 5x long is near liquidation",
    body: "Mark $0.87, liquidation at $0.81",
    action: { kind: "position", marketId: SUI_PERP, symbol: "SUI" },
  },
  {
    ...base,
    kind: "coin_received",
    title: "You received 25 SUI",
    body: "From 0x3f9e...11c0",
    amount: { raw: "25000000000", decimals: 9, symbol: "SUI", coinType: SUI, usd: 22.65 },
    action: { kind: "tx", digest: DIGEST },
  },
];

const OLD: Array<Sample & { ago: number }> = [
  { ...LIVE[4], ago: 25 * 60_000 },
  { ...base, kind: "bought", title: "You bought Popkins #1187 for 58 SUI", body: "From a listing on rare.market", imageUrl: art(POPKIN), item: popkin(1187), action: { kind: "item", itemId: POPKIN, slug: "popkins" }, ago: 3 * 3600_000 },
  { ...base, kind: "airdrop", title: "Airdrop: Popkins Pass", body: "Minted to you by 0x55aa...0e12", imageUrl: art(POPKIN), action: { kind: "collection", slug: "popkins" }, ago: 26 * 3600_000 },
  { ...base, kind: "price_alert", addr: null, digest: null, title: "SUI is below $0.90", body: "Your alert at $0.90 was hit. Now $0.89.", action: { kind: "coin", coinType: SUI }, ago: 2 * 86400_000 },
];

const stamp = (s: Sample, ago = 0): Notice => {
  const t = Date.now() - ago;
  return { ...s, id: String(++seq), cpTs: t - 380 - Math.round(Math.random() * 200), createdAt: t - 120, sentAt: t - 40 };
};

let oldCache: Notice[] | null = null;
export async function recent(): Promise<Notice[]> {
  await new Promise((r) => setTimeout(r, 150));
  // Oldest first so ids grow with time, then newest first as the API answers.
  if (!oldCache) oldCache = [...OLD].reverse().map(({ ago, ...s }) => stamp(s, ago)).reverse();
  return oldCache;
}

export function openStream(h: StreamHandlers): StreamConn {
  let i = 0;
  const timers: Array<ReturnType<typeof setTimeout>> = [];
  timers.push(
    setTimeout(() => {
      h.open();
      h.hello({ now: Date.now(), addrs: [ME] });
    }, 100),
  );
  const first = Number(params.get("notifyAt") ?? 2500);
  const every = Number(params.get("notifyEvery") ?? 20_000);
  const emit = () => h.notice(stamp(LIVE[i++ % LIVE.length]));
  timers.push(setTimeout(emit, first));
  const loop = setInterval(emit, every);
  return {
    close: () => {
      timers.forEach(clearTimeout);
      clearInterval(loop);
    },
  };
}

const KEY = "rare.notify.mockAlerts";
function load(): PriceAlert[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (Array.isArray(v)) return v;
  } catch {
    /* ignore */
  }
  return [{ id: "a1", coinType: SUI, symbol: "SUI", dir: "above", target: 1.1, createdAt: Date.now() - 3600_000, firedAt: null, active: true }];
}
function save(v: PriceAlert[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(v));
  } catch {
    /* ignore */
  }
}
export async function listAlerts(): Promise<PriceAlert[]> {
  return load();
}
export async function addAlert(a: { coinType: string; dir: "above" | "below"; target: number; symbol?: string }): Promise<string> {
  const id = `a${Date.now()}`;
  save([...load(), { id, coinType: a.coinType, symbol: a.symbol ?? a.coinType.split("::").pop() ?? "", dir: a.dir, target: a.target, createdAt: Date.now(), firedAt: null, active: true }]);
  return id;
}
export async function deleteAlert(id: string): Promise<void> {
  save(load().filter((x) => x.id !== id));
}
