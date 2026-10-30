// Add a friend: the main way is one box for a username, a SuiNS name or an address (the on-screen
// keyboard shows who it is as you type). Under "Other ways to add": a friend's code, or this
// player's QR code. A friend scans the QR code with their phone: it opens a rare.market page with
// this player's name and the code, which they type in Rare Launcher (Add a friend, Enter a code).
// A code is valid 15 minutes and makes the two of you friends at once (showing it is the consent).

import { ChevronDown, Search } from "lucide-react";
import { useEffect, useState } from "react";
import { askText } from "../../components/Osk";
import { useApp } from "../../lib/store";
import { Qr } from "../../marketplace/BuySheet";
import { Button, Panel } from "../../ui";
import { displayName, useProfile } from "../../sui/profile";
import { pickUsername } from "./pickUsername";
import * as F from "../store";
import { useFriends } from "../store";
import { addByName, toastError } from "./FriendsScreen";

let cached: { code: string; url: string; expiresAt: number } | null = null;

export function AddPane() {
  const address = useFriends((s) => s.address);
  const profile = useProfile();
  const [code, setCode] = useState(cached && cached.expiresAt > Date.now() + 60_000 ? cached : null);
  const [now, setNow] = useState(Date.now());
  const [more, setMore] = useState(false);
  const myName = profile.username;

  const fresh = () =>
    void F.makeCode()
      .then((c) => {
        cached = c;
        setCode(c);
      })
      .catch(toastError);

  useEffect(() => {
    if (!more) return;
    if (!code) fresh();
    const t = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(t);
  }, [more]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    void profile.refresh();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const left = code ? Math.max(0, Math.round((code.expiresAt - now) / 60_000)) : 0;
  const expired = !!code && code.expiresAt <= now;

  const enter = async () => {
    const raw = await askText("Your friend's code", "", { placeholder: "6 letters and numbers" });
    if (!raw?.trim()) return;
    try {
      const r = await F.redeemCode(raw);
      const p = F.personOf(r.address);
      useApp.getState().toast(`${F.label(p, "Your friend")} is now your friend`);
    } catch (e) {
      toastError(e);
    }
  };

  return (
    <div className="fade flex flex-col gap-[1.25rem]">
      <Panel className="flex flex-col gap-[0.9rem] px-[1.4rem] py-[1.2rem]">
        <span>
          <span className="block text-body font-semibold">Add by username</span>
          <span className="block text-small text-muted">Type a username. A SuiNS name like name.sui or a 0x address works too. They get a request to accept.</span>
        </span>
        <button
          data-nav
          data-hint-a="Type"
          onClick={() => void addByName()}
          className="flex h-[3.25rem] w-full items-center gap-[0.8rem] rounded-card border border-border-light bg-bg px-[1rem] text-left text-row text-muted/80"
        >
          <Search size="1.1rem" strokeWidth={1.75} className="shrink-0" />
          <span className="truncate">Username, name.sui or 0x...</span>
        </button>
        {!myName && (
          <div className="flex items-center gap-[1rem] text-small text-muted">
            <span className="min-w-0 flex-1">Friends find you by your username. You have not picked one yet.</span>
            <Button size="sm" onClick={() => void pickUsername()}>
              Pick a username
            </Button>
          </div>
        )}
      </Panel>

      <button
        data-nav
        aria-expanded={more}
        data-hint-a={more ? "Hide" : "Show"}
        onClick={() => setMore((v) => !v)}
        className="flex min-h-[3.25rem] w-full items-center gap-[0.75rem] rounded-card px-[1.4rem] text-left text-body font-semibold text-fg/85"
      >
        Other ways to add
        <ChevronDown size="1.1rem" strokeWidth={1.75} className={`transition-transform duration-200 ${more ? "rotate-180" : ""}`} />
      </button>

      {more && (
        <>
          <Panel>
            <div className="flex min-h-[4.25rem] items-center gap-[1.25rem] px-[1.4rem] py-[0.85rem]">
              <span className="min-w-0 flex-1">
                <span className="block text-body font-semibold">Friend code</span>
                <span className="block text-small text-muted">The code under your friend's QR code. You become friends right away.</span>
              </span>
              <Button onClick={() => void enter()}>Enter a code</Button>
            </div>
          </Panel>

      <Panel className="flex items-center gap-[2rem] px-[1.6rem] py-[1.5rem]">
        <div className="relative size-[12.5rem] shrink-0">
          {code && !expired ? <Qr text={code.url} className="size-full" /> : <div className="size-full rounded-md bg-l3" />}
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-label text-muted">Your QR code</div>
          <div className="mt-[0.2rem] truncate text-row font-semibold">{address ? displayName({ ...profile, address }) : ""}</div>
          <div className="mt-[0.9rem] h-[2.25rem] text-[2.25rem] font-semibold leading-none tracking-[0.12em] tabular-nums">{code && !expired ? `${code.code.slice(0, 3)} ${code.code.slice(3)}` : ""}</div>
          <p className="mt-[0.9rem] max-w-[30rem] text-small text-muted">
            A friend scans it with their phone, or types the code in Rare Launcher. {code && !expired ? `Works for ${left} more ${left === 1 ? "minute" : "minutes"}.` : ""}
          </p>
          <div className="mt-[1rem] flex gap-[0.75rem]">
            <Button onClick={fresh}>New code</Button>
          </div>
        </div>
      </Panel>
        </>
      )}
    </div>
  );
}
