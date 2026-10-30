// Ways into Share from elsewhere: quick settings, Settings, Home and (later) the in-game overlay.

import { Images } from "lucide-react";
import { useApp } from "../lib/store";
import { shareLink } from "./format";
import { newCount, useShare } from "./store";

export function openShare() {
  useApp.getState().open({ kind: "share" });
}

/** Takes a screenshot of what's under the launcher's own panels: they close first. */
export async function takeScreenshot(opts: { closeFirst?: boolean } = {}) {
  if (opts.closeFirst) {
    useApp.getState().close();
    await new Promise((r) => setTimeout(r, 350));
  }
  return useShare.getState().takeScreenshot();
}

export function saveClip() {
  return useShare.getState().saveClip();
}

/** A quick-settings style button that opens Share, for the in-game overlay. */
export function ShareEntry({ onOpen, className = "" }: { onOpen?: () => void; className?: string }) {
  const fresh = useShare(newCount);
  return (
    <button
      data-nav
      data-hint-a="Open Share"
      onClick={() => {
        onOpen?.();
        openShare();
      }}
      className={`flex h-[2.9rem] items-center gap-[0.6rem] rounded-card bg-l2 px-[0.9rem] text-small font-medium ${className}`}
    >
      <Images size="1.15rem" />
      Share
      {fresh > 0 && <span className="ml-auto text-meta text-muted">{fresh} new</span>}
    </button>
  );
}

/** Dev and screenshots: ?share=grid|detail|clip|uploading|done opens Share in that state. */
export function shareDevState(mode: string) {
  // The two newest captures show as new.
  useShare.setState({ seenAt: Date.now() - 2 * 3_600_000 });
  openShare();
  if (mode === "grid" || mode === "1" || mode === "") return;
  const until = Date.now() + 8000;
  const tick = () => {
    const s = useShare.getState();
    const list = s.captures ?? [];
    const c = list.find((x) => x.kind === (mode === "clip" ? "clip" : "screenshot"));
    if (!c) return void (Date.now() < until && setTimeout(tick, 150));
    s.select(c.id);
    if (mode === "uploading") useShare.setState({ phase: { at: "uploading", step: "upload" } });
    if (mode === "done") {
      // A made-up quilt id: this state is only for screenshots of the layout.
      const quiltId = "dev-share-layout-only-3kq9VwT2xR7mYb4N8pLcZ";
      useShare.setState({ phase: { at: "done", result: { quiltId, blobObjectId: null, link: shareLink(quiltId, s.network) }, copied: true } });
    }
  };
  setTimeout(tick, 300);
}
