// Inventory fixtures for the browser (`?inv=mock`, with `?watch=0x..&devsign` for an account that
// can "sign" and `?notify=mock` for the offer notice). Three games, a listed item, items with
// offers, one the indexer is slow on: a sale or listing shows in the served data 4 s after it.

import { isTauri } from "../lib/bridge";
import type { Offer } from "../marketplace/api";
import type { RareGet } from "./api";
import type { InvGroup, InvItem } from "./types";
import type { Quote, TradeApi } from "./trade";

export const useMock = !isTauri && typeof location !== "undefined" && new URLSearchParams(location.search).get("inv") === "mock";

const POPKIN = "0x89d249c6a43ff4490902247a950e2e69ba61b74089151166169cfe3fbfea8611";
const art = (id: string) => `https://storage.claynosaurz.com/popkins/images/${id}`;
const SUI = (n: number) => String(Math.round(n * 1e9));
const hex = (n: number) => `0x${n.toString(16).padStart(64, "0")}`;

const GAMES = {
  pawtato: { slug: "pawtato-heroes", name: "Pawtato Heroes", logoUrl: "" },
  popkins: { slug: "popkins", name: "Popkins", logoUrl: "" },
  warped: { slug: "warped-universe", name: "Warped Universe", logoUrl: "" },
};

const T = { hero: "0xa1::hero::Hero", pop: "0xb2::popkin::Popkin", ship: "0xc3::ship::Ship", misc: "0xd4::art::Piece" };

function item(n: number, name: string, type: string, coll: { slug: string; name: string }, o: Partial<InvItem> & { floor: number }): InvItem {
  const { floor, ...rest } = o;
  return {
    id: n === 412 ? POPKIN : hex(n),
    name,
    type,
    imageUrl: n === 412 ? art(POPKIN) : null,
    collection: { ...coll, imageUrl: null, verified: true },
    rarity: null,
    listing: null,
    lastSaleMist: null,
    topOfferMist: null,
    owner: "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e",
    floorMist: SUI(floor),
    ...rest,
  } as InvItem;
}

const initial = (): InvGroup[] => {
  const hero = { slug: "pawtato-heroes", name: "Pawtato Heroes" };
  const pop = { slug: "popkins", name: "Popkins" };
  const ship = { slug: "warped-ships", name: "Warped Ships" };
  const groups: Array<[keyof typeof GAMES | null, InvItem[]]> = [
    ["pawtato", [
      item(101, "Hero #1204", T.hero, hero, { floor: 4.2, lastSaleMist: SUI(4.6), topOfferMist: SUI(3.9) }),
      item(102, "Hero #88", T.hero, hero, { floor: 4.2, lastSaleMist: SUI(5.1), listing: { venue: "kiosk", priceMist: SUI(6), allInMist: SUI(6), kioskId: hex(900) } }),
      item(103, "Hero #3017", T.hero, hero, { floor: 4.2, lastSaleMist: null }),
    ]],
    ["popkins", [
      item(412, "Popkins #412", T.pop, pop, { floor: 58, lastSaleMist: SUI(70), topOfferMist: SUI(64) }),
      item(413, "Popkins #1190", T.pop, pop, { floor: 58, lastSaleMist: SUI(61.5), listing: { venue: "tradeport", priceMist: SUI(66), allInMist: SUI(67.2) } }),
      item(414, "Popkins #77", T.pop, pop, { floor: 58, lastSaleMist: SUI(60), staked: { protocol: "0x38f0bad7a60a8c4399a810430b7625b60c93983321b3cd8723f201cc559de5c1::staking_contract", name: "staking_contract", via: "staked" } }),
    ]],
    ["warped", [item(201, "Scout Ship 07", T.ship, ship, { floor: 12.5, lastSaleMist: SUI(11), topOfferMist: SUI(10.2) })]],
    [null, [item(301, "Suifrens Cap", T.misc, { slug: "misc", name: "Misc art" }, { floor: 1.1, lastSaleMist: SUI(1.4) })]],
  ];
  return groups.map(([g, items]) => ({ game: g ? GAMES[g] : null, items, floorValueMist: String(items.reduce((s, i) => s + Number(i.floorMist), 0)) }));
};

let db = initial();
const later = (f: () => void) => setTimeout(f, 4000);
const all = () => db.flatMap((g) => g.items);
const wait = (ms = 700) => new Promise<void>((r) => setTimeout(r, ms));

export const resetMock = () => {
  db = initial();
};

const offerFor = (it: InvItem): Offer | null =>
  it.topOfferMist
    ? ({
        id: `o-${it.id}`, venue: "tradeport", source: "tradeport/bids", nftType: it.type, feeMist: "0", royaltyMist: "0", commissionMist: "0", poolId: null, fillable: true, extra: {},
        kind: "item", collection: it.collection!, item: { id: it.id, name: it.name, imageUrl: it.imageUrl ?? "" }, trait: null, priceMist: it.topOfferMist, quantity: 1, filled: 0,
        bidder: hex(77), destKioskId: null, createdAt: new Date().toISOString(), expiresAt: null, status: "open",
      } as Offer)
    : null;

const get: RareGet = async <T,>(path: string): Promise<T> => {
  await wait(250);
  if (/\/inventory$/.test(path)) return { data: db.filter((g) => g.items.length), total: all().length } as T;
  const m = /\/v1\/items\/(0x[0-9a-f]+)/.exec(path);
  if (m) {
    const it = all().find((x) => x.id === m[1]);
    if (!it) throw new Error("rare.market 404");
    return it as T;
  }
  const o = /\/v1\/offers\?item=(0x[0-9a-f]+)/.exec(path);
  if (o) {
    const it = all().find((x) => x.id === o[1]);
    const off = it && offerFor(it);
    return { data: off ? [off] : [], nextCursor: null } as T;
  }
  throw new Error("rare.market 404");
};

const edit = (id: string, f: (i: InvItem) => InvItem | null) =>
  later(() => {
    db = db.map((g) => ({ ...g, items: g.items.flatMap((i) => (i.id === id ? [f(i)].filter((x): x is InvItem => !!x) : [i])) }));
  });

const trade: TradeApi = {
  async quote(it): Promise<Quote | null> {
    await wait(300);
    const offer = offerFor(all().find((x) => x.id === it.id) ?? it);
    return offer ? { offer, netMist: BigInt(offer.priceMist), blocked: null, sponsored: true } : null;
  },
  async sell(it) {
    await wait();
    edit(it.id, () => null);
  },
  async list(it, price) {
    await wait();
    edit(it.id, (i) => ({ ...i, listing: { venue: "kiosk", priceMist: price.toString(), allInMist: price.toString(), kioskId: hex(900) } }));
  },
  async cancel(it) {
    await wait();
    edit(it.id, (i) => ({ ...i, listing: null }));
  },
};

export const mock = { get, trade };
