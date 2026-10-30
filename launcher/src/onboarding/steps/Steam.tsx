import { useCallback, useEffect, useState } from "react";
import { IconSteam } from "../../components/icons";
import { native } from "../../lib/bridge";
import { useApp } from "../../lib/store";
import type { FirstRunStepProps } from "../../screens/FirstRun";
import { Button } from "../../ui";
import { DoneLine, Step, useStepB } from "../kit";

export function Steam({ next, back }: FirstRunStepProps) {
  useStepB(back);
  const library = useApp((s) => s.library);
  const [account, setAccount] = useState<string | null | undefined>(undefined);
  const [opened, setOpened] = useState(false);
  const [online, setOnline] = useState(true);

  const check = useCallback(() => void native.steamAccount().then(setAccount).catch(() => setAccount(null)), []);

  // Coming back from Steam (the Rare button) refocuses the launcher: look again.
  useEffect(() => {
    void native.netStatus().then((n) => setOnline(n.online)).catch(() => {});
    check();
    const onFocus = () => {
      check();
      void useApp.getState().scan({ quiet: true });
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [check]);

  const steamGames = library?.games.filter((g) => g.source === "steam").length ?? 0;
  const open = () => {
    setOpened(true);
    void native.steamBigPicture();
  };

  return (
    <Step
      title={account ? "Steam is ready" : "Sign in to Steam"}
      body={
        account
          ? "Your Steam games show up in Library and on Home, and they start straight from Rare."
          : "Steam opens in its big-screen mode. Sign in there (your phone's Steam app can scan the code), then press the Rare button to come back here."
      }
      actions={
        account ? (
          <>
            <Button variant="primary" size="lg" def onClick={next}>Continue</Button>
            <Button variant="ghost" size="lg" icon={<IconSteam size="1.15rem" />} onClick={open}>Open Steam</Button>
          </>
        ) : (
          <>
            <Button variant="primary" size="lg" def icon={<IconSteam size="1.15rem" />} onClick={open}>{opened ? "Open Steam again" : "Open Steam"}</Button>
            <Button variant="ghost" size="lg" onClick={next}>Skip Steam for now</Button>
          </>
        )
      }
    >
      <div className="flex flex-col gap-[0.9rem]">
        {account ? (
          <DoneLine>
            Signed in as <span className="font-semibold">{account}</span>
            {steamGames > 0 && <span className="text-muted">, {steamGames} games found</span>}
          </DoneLine>
        ) : opened ? (
          <div className="text-body text-muted">Waiting for you to sign in to Steam.</div>
        ) : null}
        {!account && !online && <div className="text-small text-muted">You are offline, so Steam cannot sign in yet. Skip this and sign in to Steam later from Library.</div>}
        <ul className="flex max-w-[40rem] flex-col gap-[0.4rem] text-small leading-relaxed text-muted">
          <li>Games you already installed on this device are kept and show up by themselves.</li>
          <li>Games you installed with Heroic (Epic and GOG) or itch.io are listed and start from Rare too.</li>
        </ul>
      </div>
    </Step>
  );
}
