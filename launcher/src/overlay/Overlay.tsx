// Rare over a game: the Rare menu (press the Rare button), the Switcher (hold it) or Quick Settings
// (the Quick button), as the native
// overlay session says (overlay/mod.rs, docs/OVERLAY.md). The same two surfaces open over the
// launcher as its own sheets (App.tsx: LauncherMenu, LauncherSwitcher). Always mounted, so it also
// holds "Close a paused game?" (quick resume's two-game limit).

import { fileSrc } from "../lib/bridge";
import { useApp } from "../lib/store";
import { useGameSheet } from "../localapi/sheets";
import { QuickSettings } from "../components/QuickSettings";
import { Frozen } from "./panels";
import { RareMenu } from "./RareMenu";
import { closeOverlay, useOverlay } from "./session";
import { PausedFull, Switcher } from "./Switcher";
import { PowerMenu } from "./PowerMenu";
import { usePadHandler } from "../nav/input";

/** Quick Settings over the game (the Quick button): the frozen frame behind, B or Quick closes. */
function QuickOverlay() {
  const frame = useOverlay((s) => s.session?.frame ?? null);
  // Registered after QuickSettings' own handler, so this sees presses first: only what must not
  // reach the launcher underneath (its tabs, search) stops here.
  usePadHandler((b) => {
    switch (b) {
      case "lb":
      case "rb":
      case "y":
      case "select":
        return true;
      default:
        return false;
    }
  });
  return (
    <div className="fixed inset-0 z-40">
      <Frozen src={frame ? fileSrc(frame) : null} />
      <QuickSettings onClose={() => void closeOverlay()} />
    </div>
  );
}

// The Local API's sell sheet reuses the menu's item page.
export { ItemDetail, type ItemDetailResult } from "./panels";

export function InGameOverlay() {
  const session = useOverlay((s) => s.session);
  // A game asked for a Rare sheet (Local API): the overlay is only the dimmed game behind it.
  const sheetOnly = useGameSheet((s) => !!s.sheet?.closeAfter);
  if (session && sheetOnly && session.mode === "menu") {
    return (
      <div className="fixed inset-0 z-40">
        <Frozen src={session.frame ? fileSrc(session.frame) : null} />
      </div>
    );
  }
  return (
    <>
      {session?.mode === "quick" ? (
        <QuickOverlay key="quick" />
      ) : session?.mode === "power" ? (
        <PowerMenu key={`power:${session.gameId}:${session.frame}`} session={session} />
      ) : session?.mode === "switcher" ? (
        <Switcher key={`switcher:${session.gameId}:${session.frame}`} session={session} />
      ) : session ? (
        <RareMenu key={`menu:${session.gameId}:${session.frame}`} session={session} />
      ) : null}
      <PausedFull />
    </>
  );
}

/** The Rare button over the launcher: the Rare menu as the launcher's own sheet. */
export function LauncherMenu() {
  const close = useApp((s) => s.close);
  return <RareMenu session={null} onClose={close} />;
}

/** The Rare button held over the launcher (or View + Menu): the Switcher as the launcher's own sheet. */
export function LauncherSwitcher() {
  const close = useApp((s) => s.close);
  return <Switcher session={null} onClose={close} />;
}
