// Notice toasts, console style, in the shell's toast row under the top bar: art, the notice's
// sentence and second line, and one action on Menu (the Quick settings button), never on A, so a
// toast can never steal a press meant for Play or the focused control. Menu takes the newest
// toast only while nothing modal is up (no overlay, sheet or keyboard) and only once it has been
// on screen for a moment; with no toast up, Menu opens Quick settings as always.

import { useEffect } from "react";
import { useApp } from "../lib/store";
import { feedback } from "../nav/feedback";
import { usePadHandler } from "../nav/input";
import { Hint, MImg } from "../ui";
import { actionLabel, runAction } from "./actions";
import { recordPaint } from "./latency";
import { useNotify } from "./store";
import type { Notice } from "./types";

/** A press this soon after a toast appears is meant for the page, not the toast. */
const GUARD_MS = 400;

export function NoticeArt({ n, className = "" }: { n: Notice; className?: string }) {
  const src = n.imageUrl ?? n.item?.imageUrl ?? null;
  const label = n.item?.name ?? n.amount?.symbol ?? n.title;
  return (
    <span className={`relative block shrink-0 overflow-hidden rounded-card bg-l3 ${className}`}>
      <MImg src={src} label={label} width={128} className="absolute inset-0 size-full" />
    </span>
  );
}

function open(n: Notice) {
  const st = useNotify.getState();
  st.dismissToast(n.id);
  st.markRead(n.id);
  feedback("select");
  runAction(n.action, n.digest, n);
}

function NoticeToast({ n, hint, small }: { n: Notice; hint: boolean; small: boolean }) {
  useEffect(() => {
    // Paint latency: the frame after this toast first mounted.
    const raf = requestAnimationFrame(() => recordPaint(n.id));
    return () => cancelAnimationFrame(raf);
  }, [n.id]);
  return (
    <button
      type="button"
      tabIndex={-1}
      onClick={() => open(n)}
      className={`pop pointer-events-auto flex items-center gap-[0.85rem] rounded-card border border-border-light bg-l3 text-left shadow-xl shadow-black/30 ${small ? "w-[22rem] py-[0.45rem] pl-[0.45rem] pr-[0.8rem]" : "w-[28rem] py-[0.7rem] pl-[0.7rem] pr-[1rem]"}`}
    >
      <NoticeArt n={n} className={small ? "h-[2.2rem] w-[2.2rem]" : "h-[3rem] w-[3rem]"} />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-small font-semibold text-fg">{n.title}</span>
        {n.body && <span className="block truncate text-meta text-muted">{n.body}</span>}
      </span>
      {/* Only the newest toast takes Menu, so only it shows the glyph. */}
      <span className={`shrink-0 text-meta ${hint ? "text-fg/80" : "invisible"}`}>
        <Hint b="start">{actionLabel(n.action, n)}</Hint>
      </span>
    </button>
  );
}

export function NoticeToasts() {
  const toasts = useNotify((s) => s.toasts);
  const newest = toasts[toasts.length - 1];
  usePadHandler((b, repeat) => {
    if (b !== "start" || repeat || !newest) return false;
    if (useApp.getState().overlay || document.querySelector("[data-nav-scope]")) return false;
    if (Date.now() - newest.at < GUARD_MS) return false;
    open(newest.notice);
    return true;
  }, !!newest);
  return (
    <>
      {toasts.map((t) => (
        <NoticeToast key={t.notice.id} n={t.notice} hint={t === newest} small={!!t.small} />
      ))}
    </>
  );
}
