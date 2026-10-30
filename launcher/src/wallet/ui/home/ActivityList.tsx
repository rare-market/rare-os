// Activity grouped by day.

import { ArrowDownLeft, ArrowLeftRight, ArrowUpRight, Repeat } from "lucide-react";
import type { ActivityItem } from "../../data/activity";
import { Addr, cn } from "../kit";
import { dayLabel, shortDigest } from "./lib";

const TITLE = { send: "Sent", receive: "Received", swap: "Swapped", other: "Transaction" } as const;
const ICON = { send: ArrowUpRight, receive: ArrowDownLeft, swap: ArrowLeftRight, other: Repeat } as const;

export function ActivityRow({ a, onOpen, idx }: { a: ActivityItem; onOpen: () => void; idx?: number }) {
  const Icon = ICON[a.kind];
  const failed = a.status === "failed";
  return (
    <button data-nav data-idx={idx} onClick={onOpen} className="flex h-[4.25rem] w-full items-center gap-4 rounded-lg px-3 text-left transition-colors focus-visible:bg-layer-03">
      <span className="flex size-10 shrink-0 items-center justify-center rounded-full bg-layer-03 text-icon-primary">
        <Icon size="1.2rem" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-base font-semibold">{TITLE[a.kind]}</span>
        <span className="block truncate text-sm text-secondary">
          {a.summary}
          {a.counterparty && a.kind !== "swap" && (
            <>
              {" "}
              {a.kind === "receive" ? "from" : "to"} <Addr address={a.counterparty} />
            </>
          )}
        </span>
      </span>
      <span className="shrink-0 text-right">
        <span className={cn("block text-sm", failed ? "text-negative" : "text-secondary")}>{failed ? "Failed" : new Date(a.timestamp).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</span>
        <span className="block font-mono text-xs text-secondary/70">{shortDigest(a.digest)}</span>
      </span>
    </button>
  );
}

/** Day headings with their rows. `focusIdx` numbers rows across the groups for nearEnd. */
export function ActivityGroups({ items, onOpen }: { items: ActivityItem[]; onOpen: (a: ActivityItem) => void }) {
  const groups = new Map<string, ActivityItem[]>();
  for (const a of items) {
    const k = dayLabel(a.timestamp);
    groups.set(k, [...(groups.get(k) ?? []), a]);
  }
  let idx = 0;
  return (
    <div>
      {[...groups].map(([day, list]) => (
        <div key={day} className="mb-3">
          <div className="px-3 pb-1 pt-2 text-sm font-semibold text-secondary">{day}</div>
          {list.map((a) => (
            <ActivityRow key={a.digest} a={a} idx={idx++} onOpen={() => onOpen(a)} />
          ))}
        </div>
      ))}
    </div>
  );
}
