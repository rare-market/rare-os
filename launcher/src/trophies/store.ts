// Trophy case state: local Steam achievements, the player's chain profile and minted trophies,
// each Steam game's chain definitions, claims in flight, and the unlock pop-up queue.

import { create } from "zustand";
import { isTauri } from "../lib/bridge";
import { useApp } from "../lib/store";
import { grpcReader, lookupGame, lookupProfile, ownedTrophies, readGame, readProfile, readRegistry, steamSlug, type RegistryInfo, type TrophyReader } from "./chain";
import { trophyConfig, type TrophyConfig } from "./config";
import { MOCK_CONFIG, mockReader, mockSeed, mockSteam, mockSubmit } from "./mock";
import { onSteamUnlocks, trophiesNative } from "./native";
import type { ChainGame, ChainProfile, ChainTrophy, NewUnlock, SteamGame, Tier, TrophySource } from "./types";
import { failed } from "../lib/fail";

interface ChainSetup {
  config: TrophyConfig;
  reader: TrophyReader;
  mock: boolean;
}

const param = (k: string) => {
  try {
    return new URLSearchParams(location.search).get(k);
  } catch {
    return null;
  }
};

/** The deployed package (deployment.json), or the fake one in browser dev with ?trophychain. */
function chainSetup(): ChainSetup | null {
  if (!isTauri && param("trophychain") != null) return { config: MOCK_CONFIG, reader: mockReader, mock: true };
  const config = trophyConfig();
  return config ? { config, reader: grpcReader(config.network), mock: false } : null;
}

export interface Popup {
  id: number;
  kind: "steam" | "minted";
  name: string;
  game: string;
  tier: Tier | null;
  points: number | null;
  iconUrl: string | null;
  source: TrophySource | null;
  /** "and 3 more" when a burst was folded into one */
  more: number;
}

interface TrophyState {
  setup: ChainSetup | null;
  steam: SteamGame[] | null;
  steamLoading: boolean;
  address: string | null;
  registry: RegistryInfo | null;
  profile: ChainProfile | null;
  profileId: string | null;
  minted: ChainTrophy[];
  chainLoading: boolean;
  chainError: string | null;
  chainAt: number;
  /** appid -> its Game on chain; null = not on Rare Trophies, missing = not read yet */
  games: Record<number, ChainGame | null>;
  /** appid -> API names being claimed */
  claiming: Record<number, string[]>;
  popups: Popup[];
  loadSteam: () => Promise<void>;
  loadChain: (address: string | null, force?: boolean) => Promise<void>;
  claim: (appid: number, keys: string[]) => Promise<void>;
  popup: (p: Omit<Popup, "id" | "more">) => void;
  dropPopup: (id: number) => void;
}

let popupSeq = 0;

export const useTrophies = create<TrophyState>((set, get) => ({
  setup: chainSetup(),
  steam: null,
  steamLoading: false,
  address: null,
  registry: null,
  profile: null,
  profileId: null,
  minted: [],
  chainLoading: false,
  chainError: null,
  chainAt: 0,
  games: {},
  claiming: {},
  popups: [],

  loadSteam: async () => {
    if (get().steamLoading) return;
    set({ steamLoading: true });
    try {
      const steam = await trophiesNative.steam();
      set({ steam });
    } catch {
      set({ steam: get().steam ?? [] });
    } finally {
      set({ steamLoading: false });
    }
  },

  loadChain: async (address, force) => {
    const { setup } = get();
    if (!setup) return;
    if (!address) return set({ address: null, profile: null, profileId: null, minted: [] });
    const s = get();
    if (s.chainLoading || (!force && s.address === address && Date.now() - s.chainAt < 60_000)) return;
    if (s.address !== address) set({ profile: null, profileId: null, minted: [] });
    set({ chainLoading: true, chainError: null, address });
    if (setup.mock) mockSeed(address);
    const read = setup.reader;
    try {
      const registry = s.registry ?? (await readRegistry(read, setup.config.registry));
      const [profileId, minted] = await Promise.all([lookupProfile(read, registry, address), ownedTrophies(read, setup.config.package, address)]);
      const profile = profileId ? await readProfile(read, profileId) : null;
      if (get().address !== address) return;
      set({ registry, profileId, profile, minted });
      // Every Steam game with achievements: is it on Rare Trophies, and with which trophies.
      if (!get().steam) await get().loadSteam();
      const steam = get().steam ?? [];
      const games: Record<number, ChainGame | null> = {};
      await Promise.all(
        steam.map(async (g) => {
          const id = await lookupGame(read, registry, steamSlug(g.appid)).catch(() => null);
          games[g.appid] = id ? await readGame(read, id).catch(() => null) : null;
        }),
      );
      if (get().address === address) set({ games, chainAt: Date.now() });
    } catch (e) {
      set({ chainError: String((e as Error)?.message ?? e) });
    } finally {
      set({ chainLoading: false });
    }
  },

  claim: async (appid, keys) => {
    const { setup, address, games, profileId } = get();
    const toast = useApp.getState().toast;
    const game = games[appid];
    if (!setup || !address || !game || !keys.length) return;
    if (get().claiming[appid]) return;
    set({ claiming: { ...get().claiming, [appid]: keys } });
    try {
      const { claimSteam, walletSubmit } = await import("./attest");
      const r = await claimSteam(
        {
          config: setup.config,
          player: address,
          recheck: (id) => trophiesNative.steamApp(id),
          attest: (body) => trophiesNative.attest(body),
          submit: setup.mock ? (_tx, _title, info) => mockSubmit(address, info.game, info.trophyIds) : walletSubmit(setup.config, address),
        },
        appid,
        keys,
        game,
        profileId,
      );
      // One pop-up per minted trophy; a big batch folds into the last card ("and 9 more").
      for (const d of r.claimed) get().popup({ kind: "minted", name: d.name, game: game.name, tier: d.tier, points: d.points, iconUrl: d.imageUrl || null, source: 1 });
      if (r.skipped.length) toast(`${r.skipped.length} not claimed: ${r.skipped[0].reason.toLowerCase()}`);
      await get().loadChain(address, true);
    } catch (e) {
      const err = e as Error;
      if (err?.name === "SignRejected") toast("Claim cancelled");
      else toast(failed("Couldn't claim the trophies.", e), "error");
    } finally {
      const c = { ...get().claiming };
      delete c[appid];
      set({ claiming: c });
    }
  },

  popup: (p) => {
    const q = get().popups;
    // A burst (a game syncing many unlocks at once) folds into the last card.
    if (q.length >= 4) {
      const last = q[q.length - 1];
      set({ popups: [...q.slice(0, -1), { ...last, more: last.more + 1 }] });
      return;
    }
    set({ popups: [...q, { ...p, id: ++popupSeq, more: 0 }] });
  },
  dropPopup: (id) => set({ popups: get().popups.filter((p) => p.id !== id) }),
}));

/** A local Steam unlock as a pop-up, with its tier when the game is on Rare Trophies. */
export function unlockPopup(u: NewUnlock) {
  const s = useTrophies.getState();
  const def = s.games[u.appid]?.defs.find((d) => d.key === u.apiName) ?? null;
  const title = useApp.getState().library?.games.find((g) => g.id === `steam:${u.appid}`)?.title;
  s.popup({ kind: "steam", name: u.name, game: u.gameName || title || "Steam", tier: def?.tier ?? null, points: def?.points ?? null, iconUrl: u.iconUrl, source: null });
}

let started = false;
/** Listens for new local unlocks for the life of the app. */
export function startTrophies() {
  if (started) return;
  started = true;
  void onSteamUnlocks((unlocks) => {
    for (const u of unlocks) unlockPopup(u);
    void useTrophies.getState().loadSteam();
  });
  // Dev and screenshots: ?trophytoast shows an unlock and a mint pop-up.
  if (!isTauri && param("trophytoast") != null) {
    setTimeout(() => {
      const g = mockSteam()[0];
      const a = g.achievements.find((x) => x.unlocked)!;
      unlockPopup({ appid: g.appid, gameName: g.name, apiName: a.apiName, name: a.name, description: a.description, iconUrl: a.iconUrl, unlockTime: a.unlockTime });
      useTrophies.getState().popup({ kind: "minted", name: "Test of Resolve", game: g.name, tier: 1, points: 30, iconUrl: g.achievements.find((x) => x.apiName === "HORNET_1")?.iconUrl ?? null, source: 1 });
    }, 900);
  }
}

/** Where claiming stands for a game, as one plain sentence, or null when Claim works. */
export function claimBlocker(s: Pick<TrophyState, "setup" | "address" | "games" | "registry">, appid: number): string | null {
  if (!s.setup) return "Claiming opens soon";
  if (!s.address) return "Add a Rare Wallet account to claim";
  if (s.registry?.paused) return "Claims are paused";
  if (s.registry && !s.registry.issuerKeySet) return "Claiming opens soon";
  if (!(appid in s.games)) return "Checking Rare Trophies";
  const g = s.games[appid];
  if (!g) return "This game is not on Rare Trophies yet";
  if (!g.attested) return "This game's studio issues its own trophies";
  if (g.paused) return "Claims for this game are paused";
  return null;
}
