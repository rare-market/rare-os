// Friends running in the background: the account follows Rare Wallet's selected account, presence
// goes out as a heartbeat (every 30 s and on every game change), friend events from the notify
// stream update the store and become notices (friend request, came online, party invite, party
// message), and a host's party follows the game they are playing.

import { useApp } from "../lib/store";
import { poll } from "../lib/idle";
import { notifyLocal, refreshNotifyStream, setSocialTokenProvider } from "../notify";
import { launcherFocused } from "../notify/desktop";
import { useProfile } from "../sui/profile";
import { useWallet } from "../wallet/ui/store";
import * as api from "./api";
import { runningRef } from "./join";
import { activeParty, apply, currentKey, label, onKeyChange, refresh, setPartyGame, syncAccount, useFriends } from "./store";
import type { GameRef, SocialEvent } from "./types";

export const SOCIAL_EVENT = "rare:social";
const BEAT_MS = 30_000;
const AWAY_MS = 5 * 60_000;

let started = false;
let beatTimer: ReturnType<typeof setTimeout> | null = null;
let lastActive = Date.now();
let lastSentGame: string | null = null;
let lastAvatar: string | null | undefined;

function presenceState(): "online" | "away" {
  if (useApp.getState().running.length) return "online";
  return !launcherFocused() && Date.now() - lastActive > AWAY_MS ? "away" : "online";
}

function avatarNow(): { id: string; imageUrl: string } | null {
  const a = useProfile.getState().avatar;
  return a && /^0x[0-9a-f]{64}$/i.test(a.id) && a.imageUrl.startsWith("https://") ? { id: a.id, imageUrl: a.imageUrl } : null;
}

/** Sends one heartbeat now and schedules the next. */
async function beat() {
  if (beatTimer) clearTimeout(beatTimer);
  beatTimer = setTimeout(() => void beat(), BEAT_MS);
  const k = currentKey();
  if (!k || useFriends.getState().status !== "ready") return;
  const party = activeParty(useFriends.getState());
  const game = runningRef(party?.id);
  const av = avatarNow();
  // The picture goes up only when it changes.
  const avatar = av?.imageUrl !== lastAvatar ? av : undefined;
  try {
    const me = await api.heartbeat(k, { state: presenceState(), game, ...(avatar !== undefined ? { avatar } : {}) });
    lastAvatar = av?.imageUrl ?? null;
    const cur = useFriends.getState().me;
    if (cur) useFriends.setState({ me: { ...cur, presence: me } });
  } catch {
    /* offline: the next beat tries again */
  }
  followGame(game);
}

/** The host's party shows the game the host is playing. */
function followGame(game: GameRef | null) {
  const s = useFriends.getState();
  const party = activeParty(s);
  const id = game?.id ?? null;
  if (id === lastSentGame) return;
  lastSentGame = id;
  if (!party || party.host !== s.address || !game || party.game?.id === game.id) return;
  void setPartyGame(party.id, game).catch(() => undefined);
}

/** Tells friends we left (best effort, also on window close). */
function goodbye() {
  const k = currentKey();
  if (!k || api.useMock) return;
  const body = JSON.stringify({ state: "offline", game: null });
  void k.headers("POST", "/v1/presence", body).then((h) =>
    fetch("https://rare.market/v1/presence", { method: "POST", headers: { ...h, "content-type": "application/json" }, body, keepalive: true }).catch(() => undefined),
  );
}

// ---------- notices ----------

const onlineAt = new Map<string, number>();
const chatAt = new Map<string, number>();

async function onSocial(e: SocialEvent) {
  const r = await apply(e);
  if (!r) return;
  switch (r.kind) {
    case "request":
      notifyLocal({
        kind: "friend_request",
        title: `${label(r.from)} wants to be friends`,
        body: "Accept or decline in Friends.",
        imageUrl: r.from?.avatarUrl ?? null,
        action: { kind: "friends", address: r.address },
      });
      return;
    case "accepted":
      useApp.getState().toast(`${label(r.who)} is now your friend`);
      return;
    case "online": {
      // One "came online" per friend per 15 minutes.
      if (Date.now() - (onlineAt.get(r.friend.address) ?? 0) < 15 * 60_000) return;
      onlineAt.set(r.friend.address, Date.now());
      notifyLocal({
        kind: "friend_online",
        title: r.game ? `${label(r.friend)} is playing ${r.game.title}` : `${label(r.friend)} is online`,
        body: null,
        imageUrl: r.friend.avatarUrl,
        action: { kind: "friends", address: r.friend.address },
      });
      return;
    }
    case "invite": {
      const from = r.party.members.find((m) => m.address === r.from) ?? r.party.members.find((m) => m.address === r.party.host) ?? null;
      notifyLocal({
        kind: "party_invite",
        title: `${label(from)} invited you to a party`,
        body: r.party.game ? `Playing ${r.party.game.title}` : `${r.party.members.length} players`,
        imageUrl: from?.avatarUrl ?? null,
        action: { kind: "party", partyId: r.party.id },
      });
      return;
    }
    case "message": {
      // A busy chat makes one notice a minute, not one per line.
      if (Date.now() - (chatAt.get(r.message.partyId) ?? 0) < 60_000) return;
      chatAt.set(r.message.partyId, Date.now());
      const from = r.party?.members.find((m) => m.address === r.message.from) ?? null;
      notifyLocal({
        kind: "party_message",
        title: `${label(from)}: ${r.message.text}`,
        body: r.party?.game ? `Party, ${r.party.game.title}` : "Party chat",
        imageUrl: from?.avatarUrl ?? null,
        action: { kind: "party", partyId: r.message.partyId },
      });
      return;
    }
  }
}

/** Starts friends once, from App. */
export function startFriends() {
  if (started) return;
  started = true;
  setSocialTokenProvider(async () => {
    const k = currentKey();
    return k ? k.streamToken() : null;
  });
  onKeyChange(() => {
    refreshNotifyStream();
    void beat();
  });
  window.addEventListener(SOCIAL_EVENT, (e) => void onSocial((e as CustomEvent<SocialEvent>).detail));
  void syncAccount();
  // Accounts added without the wallet store (?watch=, first run): check again soon and then slowly.
  for (const ms of [800, 2500]) setTimeout(() => void syncAccount(), ms);
  poll(() => syncAccount(), { active: 15_000, idle: 120_000, away: 300_000 });
  window.addEventListener("storage", (e) => e.key?.startsWith("rare.wallet.") && void syncAccount());
  useWallet.subscribe((s, prev) => {
    if (s.current?.address !== prev.current?.address || s.accounts !== prev.accounts) void syncAccount();
  });
  // A game started or stopped: tell friends now, not at the next beat.
  useApp.subscribe((s, prev) => {
    if (s.running.join() !== prev.running.join()) void beat();
  });
  useFriends.subscribe((s, prev) => {
    if (s.status === "ready" && prev.status !== "ready") void beat();
  });
  for (const ev of ["keydown", "pointerdown", "focus"]) window.addEventListener(ev, () => (lastActive = Date.now()), { passive: true });
  // The stream token lasts 12 hours on the server: reconnect with a fresh one every 6.
  setInterval(() => currentKey() && refreshNotifyStream(), 6 * 3_600_000);
  // A slow safety net for anything the stream missed while offline.
  setInterval(() => void refresh(), 5 * 60_000);
  window.addEventListener("pagehide", goodbye);
  void beat();
  if (api.useMock) mockEvents();
}

/** `?friends=mock&friendsping`: a friend request and a party invite arrive, for screenshots. */
function mockEvents() {
  if (!new URLSearchParams(location.search).has("friendsping")) return;
  const emit = (detail: SocialEvent) => window.dispatchEvent(new CustomEvent(SOCIAL_EVENT, { detail }));
  setTimeout(() => {
    const s = useFriends.getState();
    const mira = s.friends[0];
    if (!mira || !s.address) return;
    const me = { address: s.address, name: null, avatarUrl: null, state: "invited" as const, presence: mira.presence };
    emit({
      event: "party",
      data: {
        change: "invite",
        from: mira.address,
        party: { id: "mockinvite1", host: mira.address, game: mira.presence.game, createdAt: Date.now(), members: [{ ...mira, state: "joined" }, me] },
      },
    });
  }, 2500);
}
