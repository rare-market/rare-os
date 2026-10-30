// Plugs Rare Wallet's real flows into the launcher's first-run overlay (screens/FirstRun.tsx):
// the passkey step (the move-assets step lives in src/onboarding). Imported once for its side effect.

import { Fingerprint, KeyRound, Smartphone, Usb } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { registerFirstRunStep, type FirstRunStepProps } from "../../../screens/FirstRun";
import { native } from "../../../lib/bridge";
import { usePadHandler } from "../../../nav/input";
import { Button, Panel, WalletAvatar } from "../../../ui";
import { pickUsername } from "../../../friends/ui/pickUsername";
import { check, claim } from "../../../friends/username";
import { useProfile } from "../../../sui/profile";
import { useApp } from "../../../lib/store";
import { devSign } from "../dev";
import { keystore } from "../../../wallet";
import { useWallet, useWalletNav } from "../store";
import { WalletScreen } from "../WalletScreen";
import { PasskeyFlow, type PasskeyStep } from "./PasskeyFlow";

function Copy({ title, body, children }: { title: string; body?: ReactNode; children?: ReactNode }) {
  return (
    <div className="flex flex-col gap-[1rem]">
      <h1 className="text-[2.5rem] font-semibold leading-[1.1] tracking-[-0.03em]">{title}</h1>
      {body && <p className="text-row font-normal leading-relaxed text-muted">{body}</p>}
      {children}
    </div>
  );
}

function Way({ icon, title, body }: { icon: ReactNode; title: string; body: string }) {
  return (
    <div className="flex items-center gap-[1rem] border-t border-hair px-[1.4rem] py-[1rem] first:border-t-0">
      <span className="flex h-[2.75rem] w-[2.75rem] shrink-0 items-center justify-center rounded-full bg-l3 text-fg">{icon}</span>
      <span>
        <span className="block text-body font-semibold">{title}</span>
        <span className="block text-small text-muted">{body}</span>
      </span>
    </div>
  );
}

type Mode = "start" | "signin" | "create" | "other" | "named";

/** A free name to offer, from the address so it is stable: player-xxxx. */
async function suggest(address: string): Promise<string | null> {
  const hex = address.slice(2).toLowerCase();
  for (let k = 0; k < 4; k++) {
    const name = `player${hex.slice(k * 4, k * 4 + 4)}`;
    try {
      if ((await check(name, address.toLowerCase())).available) return name;
    } catch {
      return null;
    }
  }
  return null;
}

/** After sign-in: "You're @name" with a free suggestion and Change. Continue saves it with one signature. */
function Named({ next }: { next: () => void }) {
  const prof = useProfile();
  const [name, setName] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    void prof.refresh(true).then(async () => {
      const p = useProfile.getState();
      if (p.username) return setName(p.username);
      const acct = await keystore.current();
      if (!acct || acct.kind === "watch" && !devSign) return next();
      setName(await suggest(acct.address));
    });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const saved = !!prof.username;
  const go = async () => {
    if (saved || !name) return next();
    setBusy(true);
    try {
      await claim(name);
    } catch {
      useApp.getState().toast("Could not save the username. You can pick one from your profile.", "error");
    }
    next();
  };
  const change = async () => {
    const m = await pickUsername();
    if (m) setName(m.username);
  };
  return (
    <div className="grid h-full grid-cols-[minmax(0,26rem)_minmax(0,1fr)] items-center gap-[4rem]">
      <Copy title={name ? `You're @${name}` : "You're signed in"} body="Friends find you by your username. It is free, and you can change it from your profile.">
        <div data-nav-group="firstrun-actions" data-nav-remember className="mt-[0.75rem] flex flex-wrap gap-[0.75rem]">
          <Button variant="primary" size="lg" def disabled={busy} onClick={() => void go()} className="min-w-[10rem]">
            Continue
          </Button>
          {prof.address && (
            <Button size="lg" variant="ghost" disabled={busy} onClick={() => void change()}>
              Change
            </Button>
          )}
        </div>
      </Copy>
      <div className="min-w-0">
        {prof.address && (
          <Panel className="flex max-w-[30rem] items-center gap-[1.25rem] px-[1.6rem] py-[1.5rem]">
            <WalletAvatar address={prof.address} size="4.5rem" />
            <span className="min-w-0">
              <span className="block text-label text-muted">How friends see you</span>
              <span className="block truncate text-title font-semibold">{name ? `@${name}` : "Your username"}</span>
            </span>
          </Panel>
        )}
      </div>
    </div>
  );
}

function PasskeyStepBody({ next, back }: FirstRunStepProps) {
  const [mode, setMode] = useState<Mode>(() => (devSign && new URLSearchParams(location.search).has("named") ? "named" : "start")); // ?named&devsign: screenshots
  const [step, setStep] = useState<PasskeyStep>("choose");
  const ico = { size: "1.25rem", strokeWidth: 1.75 } as const;

  const landed = async (address: string) => {
    await useWallet.getState().select(address);
    setMode("named");
  };
  // Other ways (recovery phrase, Ledger, Keystone, watch) are Rare Wallet's own pages, shown inside setup.
  // B on the first of them comes back here; an account landing moves setup on.
  const [online, setOnline] = useState(true);
  useEffect(() => {
    void native.netStatus().then((n) => setOnline(n.online)).catch(() => {});
  }, []);
  const entry = useRef<string | null>(null);
  const other = () => {
    entry.current = useWallet.getState().current?.address ?? null;
    useWalletNav.getState().reset([{ kind: "add" }]);
    setMode("other");
  };
  const current = useWallet((w) => w.current);
  useEffect(() => {
    if (mode === "other" && current && current.address !== entry.current) {
      useWalletNav.getState().reset();
      setMode("named");
    }
  }, [mode, current]); // eslint-disable-line react-hooks/exhaustive-deps
  usePadHandler((b) => {
    if (b === "b" && mode === "named") return true; // signed in already: nothing to go back to
    if (b !== "b" || mode !== "other") return false;
    if (useWalletNav.getState().stack.length <= 2) {
      useWalletNav.getState().reset();
      setMode("start");
      return true;
    }
    return false;
  });

  const create = mode === "create";
  const title = mode === "start" ? "Sign in with a passkey" : create ? "Create account" : "Sign in with a passkey";
  const body =
    mode === "start"
      ? "One account for Rare Wallet and Rare Marketplace. No password to remember and no seed phrase to write down."
      : step === "pin"
        ? "This device signs everyday transactions with the PIN. Your passkey can always sign too."
        : step === "run"
          ? undefined
          : create
            ? "Make a new wallet protected by a passkey."
            : "Use the passkey you made before.";

  if (mode === "named") return <Named next={next} />;
  if (mode === "other") return <div className="mx-auto h-full w-full max-w-[56rem]"><WalletScreen /></div>;

  return (
    <div className="grid h-full grid-cols-[minmax(0,26rem)_minmax(0,1fr)] items-center gap-[4rem]">
      <Copy title={title} body={body}>
        {mode === "start" && !online && (
          <div className="flex max-w-[24rem] flex-col items-start gap-[0.6rem] text-small text-muted">
            No internet. A phone passkey needs it, a security key does not.
            <Button size="lg" variant="secondary" onClick={back}>
              Connect to Wi-Fi
            </Button>
          </div>
        )}
        {mode === "start" && (
          <div data-nav-group="firstrun-actions" data-nav-remember className="mt-[0.75rem] flex flex-col items-start gap-[0.75rem]">
            <div className="flex w-full max-w-[22rem] flex-col gap-[0.75rem]">
              <Button variant="primary" size="lg" def icon={<KeyRound size="1.1rem" strokeWidth={1.75} />} onClick={() => setMode("signin")} className="w-full">
                Sign in with a passkey
              </Button>
              <Button size="lg" onClick={() => setMode("create")} className="w-full">
                Create account
              </Button>
            </div>
            {/* Quiet links: their text lines up with the buttons' left edge. */}
            <div className="-ml-[1.75rem] flex flex-wrap gap-[0.25rem]">
              <Button size="lg" variant="ghost" onClick={other}>
                Other ways to sign in
              </Button>
              <Button size="lg" variant="ghost" onClick={next}>
                Not now
              </Button>
            </div>
          </div>
        )}
      </Copy>
      <div className="min-w-0">
        {mode === "start" ? (
          <Panel className="max-w-[30rem]">
            <Way icon={<Smartphone {...ico} />} title="Your phone" body="Scan a code and confirm with your face or fingerprint" />
            <Way icon={<Usb {...ico} />} title="A security key" body="Plug it into the USB-C port and touch it" />
            <Way icon={<Fingerprint {...ico} />} title="This device" body="A PIN that never leaves your SuiPlay" />
          </Panel>
        ) : (
          <div className="max-w-[34rem]" key={mode}>
            <PasskeyFlow create={create} active onStep={setStep} onFinish={(a) => void landed(a)} onExit={() => setMode("start")} />
          </div>
        )}
      </div>
    </div>
  );
}

registerFirstRunStep("passkey", PasskeyStepBody);
