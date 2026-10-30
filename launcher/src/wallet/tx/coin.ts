// Coin send builder. SUI is split from the gas coin; "max" SUI pays with every SUI coin and
// sends the gas coin itself, so the recipient gets everything minus the exact fee. Other coins
// use coinWithBalance, which merges and splits at build time. Builders never sign.

import { coinWithBalance, Transaction } from "@mysten/sui/transactions";
import { isValidSuiAddress, normalizeStructTag, normalizeSuiAddress } from "@mysten/sui/utils";
import { grpcClient, SUI_TYPE } from "../grpc";

/** The slice of the gRPC client the builder reads with (stubbed in tests). */
export interface CoinSendClient {
  core: {
    getBalance(o: { owner: string; coinType?: string }): Promise<{ balance: { balance: string; coinBalance?: string } }>;
    listCoins(o: { owner: string; coinType?: string; limit?: number; cursor?: string | null }): Promise<{
      objects: Array<{ objectId: string; version: string; digest: string; balance: string }>;
      hasNextPage: boolean;
      cursor: string | null;
    }>;
  };
}

export interface CoinSendParams {
  sender: string;
  to: string;
  coinType: string;
  /** base units; ignored when `max` is set */
  amount: bigint;
  /** send the whole balance (SUI: minus the network fee) */
  max?: boolean;
  /** known balance in base units; fetched over gRPC when left out */
  balance?: bigint;
  client?: CoinSendClient;
}

export class SendError extends Error {
  constructor(
    readonly code: "bad-address" | "same-address" | "bad-amount" | "no-balance" | "fee-reserve",
    message: string,
  ) {
    super(message);
    this.name = "SendError";
  }
}

/** Sui caps gas payment at 256 coins. */
const MAX_GAS_COINS = 255;

export function checkAddress(a: string): string {
  const v = a.trim();
  if (!/^0x[0-9a-fA-F]{1,64}$/.test(v) || !isValidSuiAddress(normalizeSuiAddress(v))) throw new SendError("bad-address", "Enter a valid Sui address");
  return normalizeSuiAddress(v);
}

const isSuiType = (t: string) => {
  try {
    return normalizeStructTag(t) === SUI_TYPE;
  } catch {
    return false;
  }
};

/** Build a coin transfer. Throws SendError with a plain message on bad input. */
export async function buildCoinSend(p: CoinSendParams): Promise<Transaction> {
  const sender = checkAddress(p.sender);
  const to = checkAddress(p.to);
  if (to === sender) throw new SendError("same-address", "Pick an address other than this account");
  if (!p.max && p.amount <= 0n) throw new SendError("bad-amount", "Enter an amount above zero");
  const client = p.client ?? (grpcClient as unknown as CoinSendClient);
  const sui = isSuiType(p.coinType);
  const balance = p.balance ?? BigInt((await client.core.getBalance({ owner: sender, coinType: p.coinType })).balance.balance);
  if (balance <= 0n) throw new SendError("no-balance", "Nothing to send");
  if (!p.max && p.amount > balance) throw new SendError("no-balance", "Not enough balance");
  // splitting all of it out of the gas coin leaves nothing for the fee
  if (sui && !p.max && p.amount >= balance) throw new SendError("fee-reserve", "Leave some SUI for the network fee, or use Max");

  const tx = new Transaction();
  tx.setSender(sender);
  if (sui && p.max) {
    const coins: Array<{ objectId: string; version: string; digest: string }> = [];
    let cursor: string | null = null;
    do {
      const r = await client.core.listCoins({ owner: sender, coinType: SUI_TYPE, limit: 50, cursor });
      coins.push(...r.objects);
      cursor = r.hasNextPage ? r.cursor : null;
    } while (cursor && coins.length < MAX_GAS_COINS);
    if (!coins.length) throw new SendError("no-balance", "Nothing to send");
    tx.setGasPayment(coins.slice(0, MAX_GAS_COINS).map((c) => ({ objectId: c.objectId, version: c.version, digest: c.digest })));
    tx.transferObjects([tx.gas], to);
  } else if (sui) {
    const [coin] = tx.splitCoins(tx.gas, [p.amount]);
    tx.transferObjects([coin], to);
  } else {
    tx.transferObjects([coinWithBalance({ type: p.coinType, balance: p.max ? balance : p.amount })], to);
  }
  return tx;
}
