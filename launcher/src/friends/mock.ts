// Sample friends for `?friends=mock` (browser only): UI work and screenshots without a wallet
// signature. State lives in memory and changes with the actions, like the real API.

import type { Friend, GameRef, GraphOp, Message, Overview, Party, Pending, Person, Presence, Privacy } from "./types";

const ME = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const hex = (seed: string) => {
  let h = 2166136261;
  let out = "";
  for (let i = 0; out.length < 64; i++) {
    h = Math.imul(h ^ seed.charCodeAt(i % seed.length) ^ i, 16777619) >>> 0;
    out += h.toString(16).padStart(8, "0");
  }
  return `0x${out.slice(0, 64)}`;
};
const min = 60_000;
const now = () => Date.now();

export const ACT: GameRef = { id: "web:act", title: "ACT", kind: "web", url: "https://play.actuniverse.com/" };
const BALATRO: GameRef = { id: "steam:2379780", title: "Balatro", kind: "steam", appid: 2379780 };
const HADES: GameRef = { id: "steam:1145360", title: "Hades II", kind: "steam", appid: 1145360 };

const on = (game: GameRef | null, since: number): Presence => ({ state: "online", game, since: now() - since, lastSeen: now() });

/** Names ending in .sui are SuiNS names; a name with a username is shown as that username with the SuiNS line under it. */
function friend(name: string, avatarUrl: string | null, presence: Presence, days = 20, username: string | null = null): Friend {
  const user = username ?? (name.endsWith(".sui") ? null : name);
  return { address: hex(name), name: user ?? name, username: user, suins: name.endsWith(".sui") ? name : null, avatarUrl, since: now() - days * 86_400_000, presence };
}

let settings: Privacy = { showPlaying: true, appearOffline: false, visibility: "friends" };

let friends: Friend[] = [
  friend("mira.sui", "https://storage.claynosaurz.com/popkins/images/0x817dac94fff3a567b80e23a3b55fca2fd108641e0dd06fc614e5ecc0acbb4e03", on(ACT, 42 * min), 20, "mira"),
  friend("kenji.sui", "https://img.pawtato.app/hero/7842.png", on(BALATRO, 18 * min), 20, "kenji"),
  friend("lowkey.sui", "https://byzantion.mypinata.cloud/ipfs/QmbUWeu1ryJX5aKz8wF3bnoYyabw9A2abCfjDsJVDdJtio/5822.png", on(HADES, 75 * min), 20, "lowkey"),
  friend("otter", null, on(null, 9 * min)),
  friend("vex.sui", "https://indexer.mypinata.cloud/ipfs/bafkreid6owvnhp2v2muf4yrk4v3hjonhposfgsnd5zirfzgbyutxoasxay", { state: "away", game: null, since: now() - 3 * 3_600_000, lastSeen: now() }),
  friend("sable.sui", null, { state: "offline", game: null, since: null, lastSeen: now() - 3 * 3_600_000 }),
  friend("quill.sui", "https://img.pawtato.app/land/15439.png", { state: "offline", game: null, since: null, lastSeen: now() - 2 * 86_400_000 }),
];
let received: Pending[] = [{ address: hex("dune.sui"), name: "dune", username: "dune", suins: null, avatarUrl: null, at: now() - 6 * min }];
let sent: Pending[] = [{ address: hex("pixel.sui"), name: "pixel.sui", username: null, suins: "pixel.sui", avatarUrl: null, at: now() - 2 * 86_400_000 }];
let blocked: Pending[] = [];

const member = (f: Friend, state: "invited" | "joined") => ({ address: f.address, name: f.name, username: f.username, suins: f.suins, avatarUrl: f.avatarUrl, state, presence: f.presence });
const meMember = () => ({ address: ME, name: "you", username: "you", suins: null, avatarUrl: null, state: "joined" as const, presence: on(null, 30 * min) });

const withParty = typeof location !== "undefined" && new URLSearchParams(location.search).has("party");
let parties: Party[] = withParty
  ? [{ id: "mockparty01", host: friends[0].address, game: ACT, createdAt: now() - 20 * min, members: [member(friends[0], "joined"), meMember(), member(friends[3], "invited")] }]
  : [];
let chat: Message[] = withParty
  ? [
      { id: "1", partyId: "mockparty01", from: friends[0].address, text: "squad up in the hub, I'm at the west gate", at: now() - 6 * min },
      { id: "2", partyId: "mockparty01", from: ME, text: "on my way", at: now() - 5 * min },
      { id: "3", partyId: "mockparty01", from: friends[0].address, text: "otter you coming?", at: now() - 2 * min },
    ]
  : [];

const later = <T>(v: T, ms = 220) => new Promise<T>((r) => setTimeout(() => r(structuredClone(v)), ms));

export const overview = (): Promise<Overview> =>
  later({ me: { address: ME, settings, presence: on(null, 30 * min) }, friends, received, sent, blocked, parties });

export function graph(op: GraphOp, address: string) {
  const all = [...friends, ...received, ...sent, ...blocked];
  const p: Person = all.find((x) => x.address === address) ?? { address, name: null, avatarUrl: null };
  const drop = () => {
    friends = friends.filter((x) => x.address !== address);
    received = received.filter((x) => x.address !== address);
    sent = sent.filter((x) => x.address !== address);
  };
  switch (op) {
    case "request":
      drop();
      sent = [{ address, name: p.name, username: p.username, suins: p.suins, avatarUrl: p.avatarUrl, at: now() }, ...sent];
      break;
    case "accept":
      drop();
      friends = [{ address, name: p.name, username: p.username, suins: p.suins, avatarUrl: p.avatarUrl, since: now(), presence: on(null, min) }, ...friends];
      break;
    case "decline":
    case "cancel":
    case "remove":
      drop();
      break;
    case "block":
      drop();
      blocked = [{ address, name: p.name, username: p.username, suins: p.suins, avatarUrl: p.avatarUrl, at: now() }, ...blocked];
      break;
    case "unblock":
      blocked = blocked.filter((x) => x.address !== address);
      break;
  }
  return later({ state: op });
}

export function settingsSave(s: Partial<Privacy>) {
  settings = { ...settings, ...s };
  return later(settings);
}
export { settingsSave as settings };

export const code = () => later({ code: "K7Q4MX", url: "https://rare.market/v1/friends/qr/K7Q4MX", expiresAt: now() + 15 * min });

export function redeem(c: string) {
  if (c.replace(/\s/g, "").length !== 6) return Promise.reject(new Error("That code is not valid or has expired."));
  friends = [friend("nova", null, on(null, min), 0), ...friends];
  return later({ address: friends[0].address });
}

export const heartbeat = (_b: unknown) => later(on(null, 30 * min), 50);

export function createParty(game: GameRef | null, invite: string[]) {
  const p: Party = { id: `mock${Math.random().toString(36).slice(2, 10)}`, host: ME, game, createdAt: now(), members: [meMember(), ...friends.filter((f) => invite.includes(f.address)).map((f) => member(f, "invited"))] };
  parties = [p];
  chat = [];
  return later(p);
}

export function invite(id: string, addresses: string[]) {
  const p = parties.find((x) => x.id === id)!;
  for (const f of friends) if (addresses.includes(f.address) && !p.members.some((m) => m.address === f.address)) p.members.push(member(f, "invited"));
  return later(p);
}

export function join(id: string) {
  const p = parties.find((x) => x.id === id)!;
  const m = p.members.find((x) => x.address === ME);
  if (m) m.state = "joined";
  return later(p);
}

export function leave(id: string) {
  parties = parties.filter((x) => x.id !== id);
  return later({ ok: true as const });
}

export function setGame(id: string, game: GameRef | null) {
  const p = parties.find((x) => x.id === id)!;
  p.game = game;
  return later(p);
}

export const messages = (id: string) => later(chat.filter((m) => m.partyId === id));

export function post(id: string, text: string) {
  const m: Message = { id: String(chat.length + 10), partyId: id, from: ME, text, at: now() };
  chat = [...chat, m];
  return later(m, 80);
}
