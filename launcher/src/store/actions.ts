// What the Store's buttons do. Purchases, refunds and Make tradable are built in ./tx and signed
// through Rare Wallet's review sheet; installs read the release from the chain, have the wallet
// sign the download challenge, and hand the expected hash to the Rust installer, which checks it
// again. Nothing is signed without the review sheet.

import { useApp } from "../lib/store";
import { buildListTransaction } from "../overlay/trade";
import { signAndExecute, signPersonalMessage } from "../wallet/market";
import type { RareAccount } from "../wallet";
import { api, canResell, canSell } from "./api";
import { ownsOnChain, readGame, releaseFor, currentPriceUsd } from "./chain";
import { pickBuild } from "./logic";
import { storeNative } from "./native";
import { useStore } from "./state";
import { buildBuySui, buildBuyUsdc, buildClaimFree, buildMakeTradable, buildRefund, idsOf, senderKiosks } from "./tx";
import type { Installed, Licence, StoreGame } from "./types";

const ORIGIN = { name: "Rare Store", kind: "market" as const };

/** Build and network errors in plain words. */
export function plain(e: unknown): string {
  const m = (e as Error)?.message ?? String(e);
  if (/^rare\.market \d+|failed to fetch|networkerror|load failed|error sending request|dns/i.test(m)) return "Couldn't reach rare.market. Check the connection and try again.";
  if (/^no_licence/.test(m)) return "This wallet doesn't hold a licence for this game.";
  if (/^no_build/.test(m)) return "There's no build for this device yet.";
  if (/^uploads_closed/.test(m)) return "Downloads open soon.";
  return m.replace(/^[a-z_]+: /, "") || "That didn't go through. Try again.";
}

async function after(digest: string | undefined) {
  // rare.market indexes the purchase within seconds: read licences now and again shortly.
  void useStore.getState().refresh();
  setTimeout(() => void useStore.getState().refresh(), 4000);
  return digest ?? "";
}

export type Coin = "usdc" | "sui" | "free";

/** Buy (or claim) a game. Resolves with the digest; SignRejected passes through. */
export async function buy(game: StoreGame, coin: Coin, account: RareAccount): Promise<string> {
  const cfg = useStore.getState().config;
  if (!canSell(cfg) || !game.game_id) throw new Error("Coming soon.");
  const ids = idsOf(cfg);
  const chain = await readGame(game.game_id);
  if (!chain.listed || chain.blocked) throw new Error("This game isn't for sale right now.");
  const priceUsd = currentPriceUsd(chain);
  let tx;
  if (coin === "free" || priceUsd === 0n) tx = buildClaimFree(ids, { sender: account.address, gameId: game.game_id });
  else if (coin === "usdc") tx = buildBuyUsdc(ids, { sender: account.address, gameId: game.game_id, priceUsd });
  else {
    if (!chain.acceptSui) throw new Error("This game takes USDC only.");
    tx = buildBuySui(ids, { sender: account.address, gameId: game.game_id, priceUsd, price: await api.suiPrice() }).tx;
  }
  const res = await signAndExecute(tx, { account: account.address, origin: ORIGIN, title: priceUsd === 0n ? `Get ${game.name}` : `Buy ${game.name}` });
  if (res.status === "failure") throw new Error("The network rejected this purchase.");
  return after(res.digest);
}

export async function refund(game: StoreGame, licence: Licence, account: RareAccount): Promise<string> {
  const cfg = useStore.getState().config;
  if (!canSell(cfg)) throw new Error("Coming soon.");
  const tx = buildRefund(idsOf(cfg), { sender: account.address, gameId: licence.game_id, licenceId: licence.id });
  const res = await signAndExecute(tx, { account: account.address, origin: ORIGIN, title: `Refund ${game.name}` });
  if (res.status === "failure") throw new Error("The network rejected this refund.");
  // The licence is gone: so is the install.
  await storeNative.uninstall(licence.slug).catch(() => undefined);
  void useApp.getState().scan({ quiet: true });
  return after(res.digest);
}

export async function makeTradable(game: StoreGame, licence: Licence, account: RareAccount): Promise<string> {
  const cfg = useStore.getState().config;
  if (!canResell(cfg)) throw new Error("Resale opens soon.");
  const kiosks = await senderKiosks(account.address);
  const tx = buildMakeTradable(idsOf(cfg!), { sender: account.address, gameId: licence.game_id, licenceId: licence.id, kiosks });
  const res = await signAndExecute(tx, { account: account.address, origin: ORIGIN, title: `Make ${game.name} tradable` });
  if (res.status === "failure") throw new Error("The network rejected this transaction.");
  return after(res.digest);
}

/** List a tradable licence in the owner's kiosk with the native Marketplace's listing builder. */
export async function listLicence(game: StoreGame, licence: Licence, priceMist: bigint, account: RareAccount): Promise<string> {
  const cfg = useStore.getState().config;
  const origin = cfg?.origin ?? cfg?.package;
  if (!origin) throw new Error("Resale opens soon.");
  if (licence.type !== "resale" || !licence.kiosk_id) throw new Error("Make it tradable first.");
  const item = { id: licence.id, name: licence.name, type: `${origin}::licence::ResaleLicence`, imageUrl: licence.image_url || null, collection: null, kioskId: licence.kiosk_id, listing: null };
  const tx = await buildListTransaction(item, priceMist, account.address);
  const res = await signAndExecute(tx, { account: account.address, origin: { name: "Rare Marketplace", kind: "market" }, title: `List ${game.name}` });
  if (res.status === "failure") throw new Error("The network rejected this listing.");
  return after(res.digest);
}

/**
 * Download, verify and install the build of the licence's edition for this device. The chain
 * decides the expected SHA-256 (Game.editions[licence.edition]); Rust compares the download
 * against it before unpacking anything.
 */
export async function install(game: StoreGame, licence: Licence, account: RareAccount): Promise<Installed> {
  const st = useStore.getState();
  const slug = licence.slug;
  st.setBusy(slug, "Getting ready");
  try {
    const chain = await readGame(licence.game_id);
    const release = await releaseFor(chain, licence.edition);
    if (!release) throw new Error("There's no release for your edition yet.");
    const { platforms } = await storeNative.platforms();
    const build = pickBuild(release, platforms);
    if (!build) throw new Error(platforms.length ? "This game has no build for this device." : "Store games install on Rare OS devices.");
    const ch = await storeNative.challenge(account.address, slug, build.platform);
    st.setBusy(slug, "Approve in Rare Wallet");
    const sig = await signPersonalMessage(new TextEncoder().encode(ch.message), { account: account.address, origin: ORIGIN, title: `Download ${game.name}` });
    st.setBusy(slug, "Downloading");
    const inst = await storeNative.install({
      slug,
      name: game.name,
      platform: build.platform,
      address: account.address,
      message: ch.message,
      signature: sig.signature,
      expectedSha256: build.sha256,
      expectedSize: Number(build.size),
      version: release.version,
      label: release.label,
      edition: release.edition,
      launchCheck: chain.launchCheck,
      images: game.images,
    });
    await st.refreshInstalled();
    void useApp.getState().scan({ quiet: true });
    return inst;
  } finally {
    useStore.getState().setBusy(slug, null);
    const progress = { ...useStore.getState().progress };
    delete progress[slug];
    useStore.setState({ progress });
  }
}

/**
 * The launch check for games with launch_check: ask rare.market (then the chain) whether the
 * install's wallet still holds a licence. A positive answer is cached by the Rust side for 7 days,
 * which is what lets the game start offline. Throws when the licence is gone.
 */
export async function beforeLaunch(gameId: string): Promise<void> {
  if (!gameId.startsWith("rare:")) return;
  const slug = gameId.slice(5);
  const inst = (await storeNative.installed().catch(() => [] as Installed[])).find((i) => i.slug === slug);
  if (!inst?.launchCheck || !inst.owner) return;
  let owns: boolean | null = null;
  try {
    owns = (await api.check(inst.owner, slug)).owns;
  } catch {
    const cfg = useStore.getState().config ?? (await api.config().catch(() => null));
    const origin = cfg?.origin ?? cfg?.package;
    const licence = useStore.getState().licences.find((l) => l.slug === slug);
    if (origin && licence) owns = await ownsOnChain(inst.owner, licence.game_id, origin).catch(() => null);
  }
  // Offline: the Rust side lets it start inside the 7-day grace.
  if (owns === null) return;
  await storeNative.markChecked(slug, owns);
  if (!owns) throw new Error(`The licence for ${inst.name} isn't in your wallet any more.`);
}
