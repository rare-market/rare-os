// The notification centre in Quick settings: recent notices, newest first, with how long ago and
// a Dot for unread. A on a row opens what it is about; Mark all read and Clear sit in the head.
// Also the small counts the shell shows: unread next to the Quick settings hint, and what came
// in while a game runs in the bottom bar's Playing cell.

import { useEffect, useState } from "react";
import { Dot, timeAgo } from "../ui";
import { actionLabel, runAction } from "./actions";
import { noticeIsRead, useNotify, useUnread, useVisibleNotices } from "./store";
import { NoticeArt } from "./Toast";
import { downloadFraction, useDownloads } from "./downloads";

const SHOW = 8;

/** Re-renders every 30 s so "3m" moves on while the sheet is open. */
function useMinuteTick() {
  const [, set] = useState(0);
  useEffect(() => {
    const id = setInterval(() => set((x) => x + 1), 30_000);
    return () => clearInterval(id);
  }, []);
}

/** Steam downloads in progress: a title and a thin bar. A on one opens the library. */
function DownloadRows() {
  const list = useDownloads((s) => s.list);
  if (!list.length) return null;
  return (
    <div className="flex flex-col" data-nav-group="qam-downloads">
      {list.map((d) => {
        const f = downloadFraction(d);
        return (
          <button key={d.id} data-nav data-hint-a="Open library" onClick={() => runAction({ kind: "downloads" })} className="flex min-h-[3.5rem] w-full flex-col justify-center gap-[0.4rem] rounded-card px-[0.6rem] py-[0.45rem] text-left">
            <span className="flex items-center gap-[0.5rem]">
              <span className="min-w-0 flex-1 truncate text-small font-semibold text-fg">{d.title}</span>
              <span className="text-label tabular-nums text-muted">{d.paused ? "Paused" : f === null ? "Downloading" : `${Math.round(f * 100)}%`}</span>
            </span>
            <span className="block h-[3px] w-full overflow-hidden rounded-full bg-l2">
              <span className="block h-full rounded-full bg-fg" style={{ width: `${Math.round((f ?? 0) * 100)}%` }} />
            </span>
          </button>
        );
      })}
    </div>
  );
}

export function NotificationCentre() {
  useMinuteTick();
  const list = useVisibleNotices();
  const unread = useUnread();
  const read = useNotify((s) => s.read);
  const readAt = useNotify((s) => s.readAt);
  const markAllRead = useNotify((s) => s.markAllRead);
  const clear = useNotify((s) => s.clear);
  const markRead = useNotify((s) => s.markRead);
  const now = Date.now();
  const small = "btn h-[1.9rem] rounded-chip bg-l2 px-[0.7rem] text-meta font-medium text-fg";
  return (
    <section className="flex flex-col gap-[0.5rem]">
      <div className="flex items-center gap-[0.5rem]" data-nav-group="qam-notify-head">
        <span className="text-label font-medium text-secondary">Notifications</span>
        {unread > 0 && <span className="text-label tabular-nums text-fg">{unread} new</span>}
        {list.length > 0 && (
          <span className="ml-auto flex gap-[0.4rem]">
            {unread > 0 && (
              <button data-nav data-hint-a="Mark all read" onClick={markAllRead} className={small}>
                Mark all read
              </button>
            )}
            <button data-nav data-hint-a="Clear all" onClick={clear} className={small}>
              Clear
            </button>
          </span>
        )}
      </div>
      <DownloadRows />
      {list.length === 0 ? (
        <div className="rounded-card border border-hair px-[0.95rem] py-[0.8rem] text-small text-muted">No notifications yet.</div>
      ) : (
        <div className="flex flex-col" data-nav-group="qam-notify">
          {list.slice(0, SHOW).map((n) => {
            const isRead = noticeIsRead({ read, readAt }, n);
            return (
              <button
                key={n.id}
                data-nav
                data-hint-a={actionLabel(n.action, n)}
                onClick={() => {
                  markRead(n.id);
                  runAction(n.action, n.digest, n);
                }}
                className="flex min-h-[3.5rem] w-full items-center gap-[0.75rem] rounded-card px-[0.6rem] py-[0.45rem] text-left"
              >
                <NoticeArt n={n} className="h-[2.5rem] w-[2.5rem]" />
                <span className="min-w-0 flex-1">
                  <span className={`block truncate text-small ${isRead ? "text-fg/80" : "font-semibold text-fg"}`}>{n.title}</span>
                  <span className="block truncate text-label text-muted">{[timeAgo(n.createdAt, now), n.body].filter(Boolean).join(" · ")}</span>
                </span>
                {!isRead && <Dot tone="brand" />}
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}

/** Unread count beside the Quick settings hint: a plain number, no badge. */
export function UnreadCount() {
  const n = useUnread();
  if (!n) return null;
  return <span className="tabular-nums text-fg">{n}</span>;
}

/** What came in while a game runs, in the bottom bar's Playing cell. */
export function MissedCount() {
  const n = useNotify((s) => s.missed);
  if (!n) return null;
  return (
    <span className="flex items-center gap-[0.35rem] text-muted">
      <span className="text-icon-2">·</span>
      <span className="tabular-nums">{n} new</span>
    </span>
  );
}
