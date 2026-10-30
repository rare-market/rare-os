// The power button held, over anything (powerbtn.rs, overlay::power_menu): Sleep, Restart, Turn
// off. B goes back. Sleep hands the screen back first, so waking lands on what was in front.

import { useEffect, useMemo, useRef } from "react";
import { fileSrc, native } from "../lib/bridge";
import { useApp } from "../lib/store";
import { focusFirst } from "../nav/focus";
import { feedback } from "../nav/feedback";
import { usePadHandler } from "../nav/input";
import { Button, Hint } from "../ui";
import { resolveGame } from "./game";
import { Frozen, useGames } from "./panels";
import { closeOverlay, type Session } from "./session";

export function PowerMenu({ session }: { session: Session }) {
  const library = useApp((s) => s.library);
  const games = useGames();
  const game = useMemo(() => resolveGame(session.gameId, library, games), [session.gameId, library, games]);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    feedback("open");
    const t = setTimeout(() => root.current && focusFirst(root.current), 40);
    return () => clearTimeout(t);
  }, []);
  usePadHandler((b) => {
    switch (b) {
      case "b":
        feedback("back");
        void closeOverlay();
        return true;
      case "lb":
      case "rb":
      case "x":
      case "y":
      case "select":
      case "start":
        return true;
      default:
        return false;
    }
  });
  const act = (action: "suspend" | "reboot" | "poweroff") => {
    feedback("select");
    void closeOverlay().then(() => native.power(action));
  };
  const backdrop = session.frame ? fileSrc(session.frame) : game.banner;
  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center">
      <Frozen src={backdrop} />
      <div ref={root} data-nav-scope className="fade-in relative flex w-[22rem] flex-col gap-[0.5rem] rounded-card border border-hair bg-l1 p-[1rem] shadow-[0_2rem_4rem_rgb(0_0_0/0.45)]">
        <span className="px-[0.4rem] pb-[0.3rem] text-row font-semibold">Power</span>
        <Button onClick={() => act("suspend")}>Sleep</Button>
        <Button onClick={() => act("reboot")}>Restart</Button>
        <Button onClick={() => act("poweroff")}>Turn off</Button>
        <div className="flex justify-end px-[0.4rem] pt-[0.3rem] text-small text-fg/75">
          <Hint b="b">Back</Hint>
        </div>
      </div>
    </div>
  );
}
