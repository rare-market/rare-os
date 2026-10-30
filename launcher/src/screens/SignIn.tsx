import { useEffect, useRef } from "react";
import { BrandMark } from "../components/Chrome";
import { Button } from "../components/ui";
import { native } from "../lib/bridge";
import { useApp } from "../lib/store";
import { focusFirst } from "../nav/focus";
import { openWalletPage } from "../wallet/signin";

/**
 * First-run sign-in. A passkey is the account; the choices open Rare Wallet on the matching page
 * (the same screens the wallet's own welcome uses), so there is one set of flows to keep right.
 */
export function SignIn() {
  const closeOverlay = useApp((s) => s.close);
  const close = () => {
    void native.config().then((c) => native.setConfig({ ...c, onboarded: true }));
    closeOverlay();
  };
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    setTimeout(() => root.current && focusFirst(root.current), 20);
  }, []);

  return (
    <div ref={root} data-nav-scope className="fade-in fixed inset-0 z-40 flex items-center justify-center bg-bg">
      <div className="flex w-[26rem] flex-col items-center gap-[0.8rem]">
        <BrandMark size="4.2rem" />
        <h1 className="mt-[0.6rem] text-[1.9rem] font-bold tracking-[-0.03em]">Sign in to Rare</h1>
        <p className="mb-[0.8rem] text-center text-[0.92rem] leading-relaxed text-muted">One account for Rare Wallet and Rare Marketplace. Your games work without signing in.</p>
        <Button variant="primary" def className="w-full" onClick={() => openWalletPage({ kind: "passkey", mode: "signin" })}>
          Sign in with a passkey
        </Button>
        <Button className="w-full" onClick={() => openWalletPage({ kind: "passkey", mode: "create" })}>
          Create account
        </Button>
        <Button className="w-full" onClick={() => openWalletPage({ kind: "add" })}>
          Other ways to sign in
        </Button>
        <Button variant="ghost" className="mt-[0.4rem]" onClick={close}>Not now</Button>
      </div>
    </div>
  );
}
