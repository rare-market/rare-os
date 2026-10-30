// Global Rare Wallet overlays, mounted once by the app: the transaction review sheet (any tab,
// web games), the PIN prompt, and the web-game connect prompt. Also answers web-game requests
// (Wallet Standard in the game window) by sending them through the same signing queue.

// registers the passkey signer so passkey accounts can sign from any tab
import "../passkey";
import "./onboard/firstrun";
import { useEffect, useRef, useState } from "react";
import { normalizeSuiAddress } from "@mysten/sui/utils";
import { Transaction } from "@mysten/sui/transactions";
import { keystore } from "../index";
import { isTauri, webgameWallet, type WebGameRequest } from "../native";
import { signAndExecuteForGame, signPersonalMessage, signTransaction, type SignOrigin } from "../sign";
import { ConnectSheet } from "./sign/ConnectSheet";
import { ReviewHost } from "./sign/ReviewSheet";
import { UnlockSheet } from "./sign/UnlockSheet";
import { installDevHook } from "./sign/devHook";
import { errorText, mapGameRequest, normOrigin, replyValue, silentConnectAccount } from "./sign/webgame";
import { useWallet } from "./store";

const APPROVED_KEY = "rare.wallet.approvedOrigins";

export function approvedOrigins(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(APPROVED_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}

function saveApproved(list: string[]) {
  try {
    localStorage.setItem(APPROVED_KEY, JSON.stringify(list));
  } catch {
    /* kept for this session only */
  }
  void webgameWallet.setApproved(list).catch(() => {});
}

/** The sender written in a game's transaction, if any. */
function senderOf(tx: Transaction | Uint8Array): string | null {
  try {
    const s = (tx instanceof Uint8Array ? Transaction.from(tx) : tx).getData().sender;
    return s ? normalizeSuiAddress(s) : null;
  } catch {
    return null;
  }
}

export function WalletHost() {
  const accounts = useWallet((w) => w.accounts);
  const current = useWallet((w) => w.current);
  const [connects, setConnects] = useState<WebGameRequest[]>([]);
  const approved = useRef<string[]>(approvedOrigins());
  const cur = useRef(current);
  cur.current = current;

  // keep the account list and the Rust side's view of the approved origins fresh
  useEffect(() => {
    void useWallet.getState().refresh();
    void webgameWallet.setApproved(approved.current).catch(() => {});
  }, []);

  const handle = useRef<(r: WebGameRequest) => void>(() => {});
  handle.current = (r) => {
    const origin = normOrigin(r.origin);
    const reply = (ok: boolean, value: unknown) => void webgameWallet.reply(r.id, ok, value).catch(() => {});
    const m = mapGameRequest(r);
    if (!m.ok) return reply(false, m.error);
    if (m.call.kind === "connect") {
      if (!(accounts && accounts.length)) return reply(false, "Add an account to Rare Wallet first");
      // A site the player already approved connects without a sheet, so the game keeps the screen.
      const known = silentConnectAccount(approved.current, origin, accounts, cur.current);
      if (known) {
        void (async () => {
          try {
            if (cur.current?.address !== known.address) await useWallet.getState().select(known.address);
            reply(true, { accounts: [{ address: known.address, label: known.label, publicKey: (await keystore.publicKey(known.address)) ?? null }] });
          } catch (e) {
            reply(false, errorText(e));
          }
        })();
        return;
      }
      // A sheet has to show: it must be in front of the game (Rust only brings up what it expected to need).
      void webgameWallet.focus(r.id).catch(() => {});
      setConnects((q) => [...q, r]);
      return;
    }
    if (!approved.current.includes(origin)) return reply(false, "Connect this game to Rare Wallet first");
    const acct = cur.current;
    if (!acct) return reply(false, "Add an account to Rare Wallet first");
    const from: SignOrigin = { name: r.game, url: r.origin, kind: "game" };
    const call = m.call;
    void (async () => {
      try {
        if (call.kind === "transaction") {
          const sender = senderOf(call.tx);
          if (sender && sender !== normalizeSuiAddress(acct.address)) return reply(false, "This transaction is for a different account");
          if (call.execute) {
            // The game's gas budget pays when it covers this (game:<slug>); else the player does.
            const { payingGame } = await import("../../localapi/games");
            const game = await payingGame(r.gameId, r.game);
            reply(true, replyValue(call, await signAndExecuteForGame(call.tx, game, { account: acct.address, origin: from })));
          } else {
            // The game submits it itself, so only the player can pay its fee.
            reply(true, replyValue(call, await signTransaction(call.tx, { account: acct.address, origin: from })));
          }
        } else {
          reply(true, replyValue(call, await signPersonalMessage(call.message, { account: acct.address, origin: from })));
        }
      } catch (e) {
        reply(false, errorText(e));
      }
    })();
  };

  useEffect(() => {
    let off = () => {};
    let gone = false;
    void webgameWallet.onRequest((r) => handle.current(r)).then((f) => (gone ? f() : (off = f)));
    // browser development: let screenshots and tests inject requests and signing calls
    if (!isTauri) installDevHook((r) => handle.current(r));
    return () => {
      gone = true;
      off();
    };
  }, []);

  const connect = connects[0];
  const answer = async (allow: boolean, address?: string) => {
    if (!connect) return;
    setConnects((q) => q.slice(1));
    const reply = (ok: boolean, value: unknown) => void webgameWallet.reply(connect.id, ok, value).catch(() => {});
    if (!allow || !address) return reply(false, "User rejected the request");
    const acct = accounts?.find((a) => a.address === address);
    if (!acct) return reply(false, "Account not found");
    const origin = normOrigin(connect.origin);
    if (!approved.current.includes(origin)) {
      approved.current = [...approved.current, origin];
      saveApproved(approved.current);
    }
    if (cur.current?.address !== address) await useWallet.getState().select(address);
    reply(true, { accounts: [{ address: acct.address, label: acct.label, publicKey: (await keystore.publicKey(acct.address)) ?? null }] });
  };

  return (
    <>
      <ReviewHost />
      <UnlockSheet />
      {connect && accounts && (
        <ConnectSheet
          key={connect.id}
          game={connect.game}
          origin={connect.origin}
          accounts={accounts}
          initial={current?.address}
          onAllow={(a) => void answer(true, a)}
          onDeny={() => void answer(false)}
        />
      )}
    </>
  );
}
