// "Hades II didn't start.": one sentence, what happened, Try again and B.
// Try again closes what is left of the run (a start with no window keeps running) and starts it
// again the way the player did: a game, an app or Steam.

import { useEffect, useRef } from "react";
import { native } from "../lib/bridge";
import { gameById, launchFailText, runName, useApp } from "../lib/store";
import { appByRunId } from "../apps/catalog";
import { useApps } from "../apps/state";
import { focusFirst } from "../nav/focus";
import { Sheet } from "../trade/parts";
import { Button } from "../ui";

async function tryAgain(id: string) {
  const s = useApp.getState();
  s.close();
  if (s.running.includes(id)) {
    await native.quit(id).catch(() => {});
    // Wait for it to be gone (Quit gives a program a few seconds) before starting it again.
    for (let i = 0; i < 40 && useApp.getState().running.includes(id); i++) await new Promise((r) => setTimeout(r, 250));
  }
  const app = appByRunId(id);
  if (app) return useApps.getState().open(app);
  const game = gameById(id);
  if (game) return useApp.getState().launch(game);
}

export function LaunchFail({ id, reason, code }: { id: string; reason: string; code: number | null }) {
  const root = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const t = setTimeout(() => root.current && focusFirst(root.current), 20);
    return () => clearTimeout(t);
  }, []);
  const retry = !!appByRunId(id) || !!gameById(id);
  return (
    <Sheet
      onRef={(el) => (root.current = el)}
      title={`${runName(id)} didn't start.`}
      footer={
        <>
          {retry && (
            <Button variant="primary" def hint="Try again" onClick={() => void tryAgain(id)}>
              Try again
            </Button>
          )}
          <Button def={!retry} hint="Close" onClick={() => useApp.getState().close()}>
            Close
          </Button>
        </>
      }
    >
      <p className="text-body text-secondary">{launchFailText({ reason: reason as never, code })}</p>
    </Sheet>
  );
}
