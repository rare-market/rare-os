// Pyth SUI/USD update for buy_sui, ported from rare.market web/lib/coinflip/pyth.ts (the coin
// the proven sequence: Wormhole VAA verify, Pyth accumulator, update_single_price_feed, destroy
// the hot potato). The update bytes come from rare.market's /v1/store/price/sui, which holds the
// Hermes key, so the launcher never talks to Hermes. Ids are Sui mainnet.

import { bcs } from "@mysten/sui/bcs";
import type { Transaction } from "@mysten/sui/transactions";
import { fromBase64 } from "@mysten/sui/utils";

export const PYTH = {
  package: "0x04e20ddf36af412a4096f9014f4a565af9e812db9a05cc40254846cf6ed0ad91",
  original: "0x8d97f1cd6ac663735be08d1d2b6d02a159e711586461306ce60a2b7a6a565a9e",
  state: "0x1f9310238ee9298fb703c3419030b35b22bb1cc37113e3bb5007c99aec79e5b8",
  wormholePackage: "0x5306f64e312b581766351c07af79c72fcb1cd25147157fdc2f8ad76de9a3fb6a",
  wormholeState: "0xaeab97f96cf9877fee2883315d459552b2b921edc16d7ceac6eab944dd88919c",
  suiUsdFeed: "0x23d7315113f5b1d3ba7a83604c44b94d79f4fd69af77f804fc7f920a6dc65744",
  suiUsdPriceInfoObject: "0x801dbc2f0053d34734814b2d6df491ce7807a725fe9a01ad74a07e9c51396c37",
  baseUpdateFee: 1n,
} as const;

/** The VAA inside a Pyth accumulator ("PNAU") message. */
export function vaaFromAccumulator(msg: Uint8Array): Uint8Array {
  const trailing = msg[6];
  const sizeAt = 7 + trailing + 1; // header, trailing payload, proof type
  const size = (msg[sizeAt] << 8) | msg[sizeAt + 1];
  return msg.subarray(sizeAt + 2, sizeAt + 2 + size);
}

export const updateBytes = (b64: string): Uint8Array => fromBase64(b64);

/** Appends the update calls for an already fetched accumulator update. */
export function addSuiUsdUpdateFrom(tx: Transaction, update: Uint8Array): void {
  const bytes = (b: Uint8Array) => tx.pure(bcs.vector(bcs.u8()).serialize(Array.from(b)));
  const [vaa] = tx.moveCall({
    target: `${PYTH.wormholePackage}::vaa::parse_and_verify`,
    arguments: [tx.object(PYTH.wormholeState), bytes(vaaFromAccumulator(update)), tx.object.clock()],
  });
  let [hot] = tx.moveCall({
    target: `${PYTH.package}::pyth::create_authenticated_price_infos_using_accumulator`,
    arguments: [tx.object(PYTH.state), bytes(update), vaa, tx.object.clock()],
  });
  const [fee] = tx.splitCoins(tx.gas, [PYTH.baseUpdateFee]);
  [hot] = tx.moveCall({
    target: `${PYTH.package}::pyth::update_single_price_feed`,
    arguments: [tx.object(PYTH.state), hot, tx.object(PYTH.suiUsdPriceInfoObject), fee, tx.object.clock()],
  });
  tx.moveCall({
    target: `${PYTH.package}::hot_potato_vector::destroy`,
    typeArguments: [`${PYTH.original}::price_info::PriceInfo`],
    arguments: [hot],
  });
}
