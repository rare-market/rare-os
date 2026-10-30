// The player's Sui profile: SuiNS name, the NFT they picked as their picture, SUI balance and
// portfolio value. Name and balance come from Sui gRPC; collectibles, their value and the SUI price
// from rare.market. The picture choice is local to this device (per address).

import { create } from "zustand";
import { isTauri } from "../lib/bridge";
import type { ItemCard } from "../marketplace/api";
import { rareSui } from "./data";

export interface AvatarPick {
  id: string;
  imageUrl: string;
  name: string;
}

interface ProfileState {
  address: string | null;
  label: string | null;
  /** The default SuiNS name. */
  name: string | null;
  /** The Rare username the player picked. */
  username: string | null;
  avatar: AvatarPick | null;
  /** rare.market's own avatar for the account, used when nothing is picked */
  marketAvatar: string | null;
  suiMist: bigint | null;
  nftMist: bigint | null;
  suiUsd: number | null;
  loading: boolean;
  at: number;
  refresh: (force?: boolean) => Promise<void>;
  pickAvatar: (item: ItemCard | null) => void;
}

const AVATAR_KEY = (a: string) => `rare.sui.avatar.${a}`;

function readPick(address: string): AvatarPick | null {
  try {
    const v = localStorage.getItem(AVATAR_KEY(address));
    return v ? (JSON.parse(v) as AvatarPick) : null;
  } catch {
    return null;
  }
}

/** Dev and screenshots: ?as=0x... shows that address's profile without adding it to the wallet. */
const devAddress = (() => {
  if (isTauri) return null;
  try {
    const v = new URLSearchParams(location.search).get("as");
    return v && /^0x[0-9a-fA-F]{64}$/.test(v) ? v : null;
  } catch {
    return null;
  }
})();

export async function currentAddress(): Promise<{ address: string; label: string | null } | null> {
  if (devAddress) return { address: devAddress, label: null };
  const a = await (await import("../wallet")).keystore.current().catch(() => null);
  return a ? { address: a.address, label: a.label ?? null } : null;
}

export const useProfile = create<ProfileState>((set, get) => ({
  address: null,
  label: null,
  name: null,
  username: null,
  avatar: null,
  marketAvatar: null,
  suiMist: null,
  nftMist: null,
  suiUsd: null,
  loading: false,
  at: 0,
  refresh: async (force) => {
    const s = get();
    if (s.loading) return;
    const cur = await currentAddress();
    if (!cur) return set({ address: null, label: null, name: null, username: null, avatar: null, marketAvatar: null, suiMist: null, nftMist: null, at: Date.now() });
    if (!force && cur.address === s.address && Date.now() - s.at < 60_000) return;
    const changed = cur.address !== s.address;
    set({
      loading: true,
      address: cur.address,
      label: cur.label,
      avatar: readPick(cur.address),
      ...(changed ? { name: null, username: null, marketAvatar: null, suiMist: null, nftMist: null } : {}),
    });
    const a = cur.address;
    const chain = import("./chain");
    const [name, bal, acct, port, status] = await Promise.all([
      chain.then((c) => c.defaultName(a)).catch(() => null),
      chain.then((c) => c.suiBalance(a)).catch(() => null),
      rareSui.account(a).catch(() => null),
      rareSui.portfolio(a).catch(() => null),
      rareSui.status().catch(() => null),
    ]);
    if (get().address !== a) return set({ loading: false });
    set({
      name,
      username: acct?.username ?? (devUsername(a) ?? null),
      suiMist: bal,
      marketAvatar: acct?.avatarUrl ?? null,
      nftMist: port ? BigInt(port.valueMist) : null,
      suiUsd: status?.suiUsd ?? get().suiUsd,
      loading: false,
      at: Date.now(),
    });
  },
  pickAvatar: (item) => {
    const a = get().address;
    if (!a) return;
    const pick = item && item.imageUrl ? { id: item.id, imageUrl: item.imageUrl, name: item.name } : null;
    try {
      if (pick) localStorage.setItem(AVATAR_KEY(a), JSON.stringify(pick));
      else localStorage.removeItem(AVATAR_KEY(a));
    } catch {
      /* private mode: the pick lasts for this session */
    }
    set({ avatar: pick });
  },
}));

/** Portfolio total in SUI (balance plus collectibles at rare.market's value), or null before the first read. */
export function totalMist(p: Pick<ProfileState, "suiMist" | "nftMist">): bigint | null {
  if (p.suiMist === null && p.nftMist === null) return null;
  return (p.suiMist ?? 0n) + (p.nftMist ?? 0n);
}

/** Screenshots: ?username=mira shows that username on the profile without the server. */
const devUsername = (_a: string) => {
  try {
    return new URLSearchParams(location.search).get("username");
  } catch {
    return null;
  }
};

/** What the top bar and profile call the player: Rare username, else SuiNS name, else the wallet label, else the short address. */
export function displayName(p: Pick<ProfileState, "name" | "label" | "address"> & { username?: string | null }): string {
  if (p.username) return p.username;
  if (p.name) return p.name;
  if (p.label && !p.label.startsWith("0x")) return p.label;
  return p.address ? `${p.address.slice(0, 6)}...${p.address.slice(-4)}` : "";
}
