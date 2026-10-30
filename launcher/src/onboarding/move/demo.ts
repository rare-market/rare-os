// A made-up Playtron wallet for the browser dev build and screenshots (?demoscan): one of
// everything the scan can find, so every section of the step renders without a real wallet.

import type { RescueScan } from "../rescue";

const SUI = "0x0000000000000000000000000000000000000000000000000000000000000002::sui::SUI";
const USDC = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";
const id = (n: number) => `0x${n.toString(16).padStart(64, "0")}`;

export const DEMO_PLAYTRON = "0x7f3c2a9d1b6e4f5a8c0d2e4f6a8b0c2d4e6f8a0b2c4d6e8f0a2b4c6d8e0f2a4b";

export async function demoScan(address: string, onProgress?: (s: string) => void, left = false): Promise<RescueScan> {
  for (const s of ["Reading coins", "Reading owned objects", "Opening kiosks", "Looking up names"]) {
    onProgress?.(s);
    await new Promise((r) => setTimeout(r, 450));
  }
  const soulbound = [{ objectId: id(9), type: "0x8f9b::suiplay_nft::SuiPlay0x1PreorderNFT", name: "SuiPlay0X1 Eternal", image: null, collection: "SuiPlay0X1" }];
  if (left) {
    return { address, scannedAt: Date.now(), defaultName: null, coins: [], nfts: [], kiosks: [], staked: [], names: [], soulbound, truncated: false };
  }
  return {
    address,
    scannedAt: Date.now(),
    defaultName: "player.sui",
    coins: [
      { coinType: SUI, symbol: "SUI", name: "Sui", decimals: 9, iconUrl: null, raw: 42_180_000_000n, coinRaw: 42_180_000_000n, addressRaw: 0n, amount: 42.18, objectIds: [id(1), id(2)], objectBalances: [40_000_000_000n, 2_180_000_000n] },
      { coinType: USDC, symbol: "USDC", name: "USD Coin", decimals: 6, iconUrl: null, raw: 125_500_000n, coinRaw: 125_500_000n, addressRaw: 0n, amount: 125.5, objectIds: [id(3)], objectBalances: [125_500_000n] },
    ],
    nfts: [
      { objectId: id(10), type: "0xabc::popkins::Popkin", name: "Popkins #2231", image: null, collection: "Popkins" },
      { objectId: id(11), type: "0xabc::popkins::Popkin", name: "Popkins #904", image: null, collection: "Popkins" },
      { objectId: id(12), type: "0xdef::act::Hero", name: "ACT Hero: Vex", image: null, collection: "ACT" },
    ],
    kiosks: [
      {
        kioskId: id(20),
        capId: id(21),
        personal: false,
        personalPackage: null,
        profits: 1_500_000_000n,
        items: [
          { objectId: id(22), type: "0x123::rarelings::Rareling", name: "Rareling #77", image: null, collection: "Rarelings", listed: 12_000_000_000n, locked: true, transferable: true },
          { objectId: id(23), type: "0x123::rarelings::Rareling", name: "Rareling #310", image: null, collection: "Rarelings", listed: null, locked: true, transferable: true },
        ],
      },
    ],
    staked: [{ objectId: id(30), principal: 100_000_000_000n, poolId: id(31), activationEpoch: 812 }],
    names: [{ objectId: id(40), domain: "player.sui", expiresMs: Date.now() + 200 * 86_400_000, subname: false }],
    soulbound,
    truncated: false,
  };
}
