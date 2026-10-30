// The launcher's half of the Rare Local API and `rare://` links (docs/LOCAL-API.md).
//
// Rust (src-tauri/src/localapi) checks every call and sends it here as a `rare-api` event:
// { id, gameId, transport, origin, method, params, closeAfter }. Reads answer at once; sell, buy and
// claim show a sheet over the game (GameSheets) and answer when the player is done. Answers go back
// with `localapi_reply`. Links arrive as `deep-link` events with a parsed route.
//
// Browser development and tests: window.__rareLocalApiDev.call(gameId, method, params) runs the same
// path (scripts/localapi-e2e.mjs relays a sample web game's rare:platform calls through it).

import { normalizeSuiAddress } from "@mysten/sui/utils";
import { isTauri, native } from "../lib/bridge";
import { loadGames } from "../lib/rare";
import { useApp } from "../lib/store";
import { profile } from "../games/catalog";
import { FRIENDS_BASE } from "../friends/types";
import type { ItemCard } from "../marketplace/api";
import { openInMarket } from "../marketplace/nav";
import { openOverlay, useOverlay } from "../overlay/session";
import { SHOW_STORE } from "../store/show";
import { canonType } from "../sui/chain";
import { keystore } from "../wallet";
import { grpcClient } from "../wallet/grpc";
import { canSignWithWallet } from "../wallet/market";
import { withDevGame } from "./games";
import { ApiFail, gameFor, inventory, me, prepareBuy, prepareClaim, prepareSell, type Deps } from "./methods";
import { dropSheet, setReplier, showSheet, useGameSheet, type OverlayTabId } from "./sheets";

export interface ApiRequest {
  id: number;
  gameId: string | null;
  transport: "native" | "web" | "web-http" | "dev";
  origin: string | null;
  method: "me" | "inventory" | "sell" | "buy" | "claim" | "open";
  params: Record<string, unknown>;
  closeAfter: boolean;
}

/** A parsed link from Rust (localapi/deeplink.rs Route). */
export type Route =
  | { kind: "item"; id: string; sheet: "sell" | "buy" | null }
  | { kind: "collection"; id: string }
  | { kind: "game"; slug: string }
  | { kind: "store"; slug: string }
  | { kind: "trophies"; slug: string }
  | { kind: "profile"; who: string }
  | { kind: "overlay"; tab: "items" | "market" | "trophies" | "notices" | null }
  | { kind: "claim"; voucher: string };

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export function defaultDeps(): Deps {
  return {
    get: native.rareGet,
    account: () => keystore.current(),
    canSign: (a) => canSignWithWallet(a as Parameters<typeof canSignWithWallet>[0]),
    games: async () => withDevGame((await loadGames()).games),
    library: () => useApp.getState().library,
    username: async (address) => {
      const r = await fetch(`${FRIENDS_BASE}/players/${address.toLowerCase()}`);
      if (!r.ok) return null;
      return ((await r.json()) as { username?: string | null }).username ?? null;
    },
    object: async (id) => {
      try {
        const r = await (grpcClient as unknown as { core: { getObject(o: { objectId: string; include: { json: true } }): Promise<{ object: { type: string; json?: unknown } }> } }).core.getObject({ objectId: id, include: { json: true } });
        return { type: r.object.type, json: (r.object.json as Record<string, unknown> | null) ?? null };
      } catch {
        return null;
      }
    },
  };
}

// ---------------------------------------------------------------- answering

type Reply = (id: number, ok: boolean, value: unknown) => void;
const nativeReply: Reply = (id, ok, value) => void call("localapi_reply", { id, ok, value }).catch(() => {});
let reply: Reply = nativeReply;
/** Dev calls wait here for their answer. */
const devWaiting = new Map<number, (r: { ok: boolean; value: unknown }) => void>();

function failValue(e: unknown): { code: string; message: string } {
  if (e instanceof ApiFail) return { code: e.code, message: e.message };
  const m = (e as Error)?.message ?? String(e);
  if (/^rare\.market \d+|failed to fetch|networkerror|load failed/i.test(m)) return { code: "offline", message: "Couldn't reach rare.market" };
  return { code: "failed", message: m || "Something went wrong" };
}

/** Runs one request from a game. */
export async function handleRequest(r: ApiRequest, d: Deps = defaultDeps()): Promise<void> {
  const answer = (value: unknown) => reply(r.id, true, value);
  // A sheet method that ends without a sheet gives the screen straight back.
  const noSheet = () => {
    if (r.closeAfter && (r.method === "sell" || r.method === "buy" || r.method === "claim")) void import("../overlay/session").then((m) => m.closeOverlay());
  };
  try {
    switch (r.method) {
      case "me":
        return answer(await me(r.gameId, d));
      case "inventory":
        return answer(await inventory(r.gameId, d));
      case "open":
        await openView(r.gameId, r.params as { view: string; id?: string | null }, d);
        return answer({ ok: true });
      case "sell": {
        const s = await prepareSell(r.gameId, r.params as { itemId: string; mode?: string; priceMist?: string | null }, d);
        if ("status" in s) return (answer(s), noSheet());
        if (!showSheet({ kind: "sell", reqId: r.id, closeAfter: r.closeAfter, data: s })) throw new ApiFail("busy", "Another Rare sheet is open");
        return;
      }
      case "buy": {
        const s = await prepareBuy(r.gameId, r.params as { itemId?: string | null; collection?: string | null; kind?: number | null; maxMist?: string | null }, d);
        if ("status" in s) return (answer(s), noSheet());
        if (!showSheet({ kind: "buy", reqId: r.id, closeAfter: r.closeAfter, data: s })) throw new ApiFail("busy", "Another Rare sheet is open");
        return;
      }
      case "claim": {
        const s = await prepareClaim(r.gameId, String(r.params.voucher ?? ""), d);
        if ("status" in s) return (answer(s), noSheet());
        if (!showSheet({ kind: "claim", reqId: r.id, closeAfter: r.closeAfter, data: s })) throw new ApiFail("busy", "Another Rare sheet is open");
        return;
      }
      default:
        throw new ApiFail("bad_request", "Unknown method");
    }
  } catch (e) {
    reply(r.id, false, failValue(e));
    noSheet();
  }
}

// ---------------------------------------------------------------- pages and links

/** The trophy case for a game slug: its Steam app's trophies when it has one, else the case. */
function openTrophies(slug: string | null) {
  const appid = slug ? profile(slug)?.native?.steamAppIds?.[0] : undefined;
  useApp.getState().open({ kind: "trophies", appid });
}

function openStore(slug: string) {
  useApp.getState().open(SHOW_STORE ? { kind: "storegame", slug } : { kind: "suigame", slug });
}

/** A collection by slug, or by Move type (rare.market resolves the slug). */
async function openCollection(id: string, d: Deps) {
  let slug = id;
  if (id.includes("::")) {
    const det = await d.get<{ slug?: string | null }>(`/v1/collections/${encodeURIComponent(canonType(id))}`).catch(() => null);
    if (!det?.slug) throw new ApiFail("not_found", "That collection isn't on Rare");
    slug = det.slug;
  }
  openInMarket({ kind: "collection", slug });
}

/** `open` from a game: the page in the launcher (Rust brings it to the front; "overlay" never gets here). */
export async function openView(gameId: string | null, p: { view: string; id?: string | null }, d: Deps = defaultDeps()) {
  const game = await gameFor(gameId, d);
  switch (p.view) {
    case "item":
      return openInMarket({ kind: "item", id: p.id! });
    case "collection":
      return openCollection(p.id!, d);
    case "trophies":
      return openTrophies(p.id ?? game.slug);
    case "store": {
      const slug = p.id ?? game.slug;
      if (!slug) throw new ApiFail("not_found", "This game has no Rare page");
      return openStore(slug);
    }
    default:
      throw new ApiFail("bad_request", "Unknown view");
  }
}

const TAB: Record<string, OverlayTabId> = { items: "items", market: "market", trophies: "trophies", notices: "news" };

/** A `rare://` (or https://rare.market) link, already checked by Rust. */
export async function openRoute(r: Route, d: Deps = defaultDeps()): Promise<void> {
  const toast = (t: string) => useApp.getState().toast(t);
  try {
    switch (r.kind) {
      case "item": {
        openInMarket({ kind: "item", id: r.id });
        if (r.sheet === "sell") {
          const s = await prepareSell(null, { itemId: r.id, mode: "best-offer" }, { ...d, games: async () => [] });
          if ("status" in s) return toast(s.reason);
          showSheet({ kind: "sell", reqId: null, closeAfter: false, data: s });
        } else if (r.sheet === "buy") {
          const card = await d.get<ItemCard>(`/v1/items/${encodeURIComponent(r.id)}`).catch(() => null);
          if (!card?.listing) return toast("That item isn't for sale");
          showSheet({ kind: "buy", reqId: null, closeAfter: false, data: { game: await gameFor(null, d), item: card, listing: card.listing } });
        }
        return;
      }
      case "collection":
        return await openCollection(r.id, d);
      case "game":
        return useApp.getState().open({ kind: "suigame", slug: r.slug });
      case "store":
        return openStore(r.slug);
      case "trophies":
        return openTrophies(r.slug);
      case "profile": {
        let address = r.who.startsWith("0x") ? r.who : null;
        if (!address) {
          const { lookup } = await import("../friends/username");
          address = (await lookup(r.who).catch(() => null))?.address ?? null;
          if (!address) return toast(`No Rare player called ${r.who}`);
        }
        const mine = await d.account();
        if (mine && normalizeSuiAddress(mine.address) === normalizeSuiAddress(address)) return useApp.getState().open({ kind: "profile" });
        return openInMarket({ kind: "account", address, label: r.who.startsWith("0x") ? `${address.slice(0, 6)}...${address.slice(-4)}` : r.who });
      }
      case "overlay":
        // Rust opens the overlay; it lands on this tab.
        useGameSheet.setState({ overlayTab: r.tab ? TAB[r.tab] : null });
        return;
      case "claim": {
        const s = await prepareClaim(null, r.voucher, d);
        if ("status" in s) return toast(s.reason);
        showSheet({ kind: "claim", reqId: null, closeAfter: false, data: s });
        return;
      }
    }
  } catch (e) {
    toast((e as Error)?.message || "That link couldn't be opened");
  }
}

// ---------------------------------------------------------------- start

let started = false;

export function startLocalApi() {
  if (started) return;
  started = true;
  setReplier((id, ok, value) => reply(id, ok, value));
  if (!isTauri) {
    installDevHook();
    return;
  }
  void (async () => {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<ApiRequest>("rare-api", (e) => void handleRequest(e.payload));
    await listen<{ id: number }>("rare-api-cancel", (e) => dropSheet(e.payload.id));
    await listen<Route>("deep-link", (e) => void openRoute(e.payload));
    const queued = await call<Route[]>("localapi_ready").catch(() => [] as Route[]);
    for (const r of queued) await openRoute(r);
    const { startNoticeForward } = await import("./notices");
    startNoticeForward((gameId, notice) => void call("localapi_notice", { gameId, notice }).catch(() => {}));
  })();
}

/** Browser stand-in for Rust: the same calls, answered in the page, with the overlay in dev mode. */
function installDevHook() {
  let seq = 0;
  reply = (id, ok, value) => {
    const w = devWaiting.get(id);
    devWaiting.delete(id);
    w?.({ ok, value });
  };
  const dev = {
    /** One Local API call for `gameId`, as Rust would send it after its checks. */
    call(gameId: string, method: ApiRequest["method"] | "notices", params: Record<string, unknown> = {}): Promise<{ ok: boolean; value: unknown }> {
      if (method === "notices") return Promise.resolve({ ok: true, value: { ok: true } });
      const id = ++seq;
      const sheet = method === "sell" || method === "buy" || method === "claim";
      const wasOpen = !!useOverlay.getState().session;
      return new Promise((resolve) => {
        devWaiting.set(id, resolve);
        // As in Rust: the overlay comes up over the game first, then the UI gets the request.
        void (sheet && !wasOpen ? openOverlay(gameId) : Promise.resolve()).then(() =>
          handleRequest({ id, gameId, transport: "dev", origin: null, method: method as ApiRequest["method"], params, closeAfter: sheet && !wasOpen }),
        );
      });
    },
    open: (r: Route) => openRoute(r),
    sheet: () => useGameSheet.getState().sheet?.kind ?? null,
  };
  (window as unknown as { __rareLocalApiDev: typeof dev }).__rareLocalApiDev = dev;
}
