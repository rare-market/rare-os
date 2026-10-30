// Settings > Notifications: a switch per kind of notice, toasts while playing, desktop
// notifications, and this device's price alerts (A deletes one).

import { useEffect } from "react";
import { SettingRow, SwitchRow } from "../screens/settings/rows";
import { Dot, Panel, usdPrice } from "../ui";
import { feedback } from "../nav/feedback";
import { useAlerts } from "./alerts";
import { useNotify } from "./store";
import { KINDS } from "./kinds";
import { CLASSES, CLASS_LABEL, classOf, type NoticeClass } from "./logic";

const CLASS_NOTE: Record<NoticeClass, string> = {
  money: "Pops up over a game. Waits in the list with Do not disturb.",
  invites: "Pops up over a game. Waits in the list with Do not disturb.",
  trophies: "A small toast over a game. Waits in the list with Do not disturb.",
  social: "Pops up in Rare. Waits in the list over a game and with Do not disturb.",
  system: "Pops up in Rare. Waits in the list over a game and with Do not disturb.",
  battery: "Always pops up, even over a game and with Do not disturb.",
};

export function NotificationSettings() {
  const prefs = useNotify((s) => s.prefs);
  const setKind = useNotify((s) => s.setKind);
  const setPrefs = useNotify((s) => s.setPrefs);
  return (
    <div className="fade flex flex-col gap-[1.25rem]">
      <Panel>
        <SwitchRow id="notif-dnd" title="Do not disturb" body="Only battery warnings pop up. Everything else waits in the list." on={prefs.dnd} onChange={(v) => setPrefs({ dnd: v })} />
        <SwitchRow id="notif-toasts" title="Show toasts over a game" body="Money, invites and trophies pop up over a game. Off: they wait in the list until it ends." on={prefs.gameToasts} onChange={(v) => setPrefs({ gameToasts: v })} />
        <SwitchRow id="notif-desktop" title="Desktop notifications" body="When Rare is in the background, notices also show on the desktop." on={prefs.desktop} onChange={(v) => setPrefs({ desktop: v })} />
      </Panel>
      {CLASSES.map((c) => {
        const rows = KINDS.filter((k) => classOf(k.kind) === c);
        if (!rows.length) return null;
        return (
          <Panel key={c}>
            <SettingRow id={`notif-class-${c}`} title={CLASS_LABEL[c]} body={CLASS_NOTE[c]} />
            {rows.map((k) => (
              <SwitchRow key={k.kind} id={`notif-${k.kind}`} title={k.title} body={k.body} on={prefs.kinds[k.kind]} onChange={(v) => setKind(k.kind, v)} />
            ))}
          </Panel>
        );
      })}
      <PriceAlerts />
    </div>
  );
}

function PriceAlerts() {
  const list = useAlerts((s) => s.list);
  const error = useAlerts((s) => s.error);
  const load = useAlerts((s) => s.load);
  const remove = useAlerts((s) => s.remove);
  useEffect(() => {
    void load();
  }, [load]);
  const active = (list ?? []).filter((a) => a.active);
  const fired = (list ?? []).filter((a) => !a.active).slice(0, 5);
  return (
    <Panel>
      <SettingRow id="notif-alerts" title="Your price alerts" body={list && !list.length ? "No price alerts. Set one from a coin's page in Trade." : error && !list ? "Could not load price alerts." : "One notice when the price crosses your target, then the alert stops."} />
      {[...active, ...fired].map((a) => (
        <button
          key={a.id}
          data-nav
          data-hint-a="Delete alert"
          onClick={() => {
            feedback("toggle");
            void remove(a.id);
          }}
          className="flex min-h-[3.75rem] w-full items-center gap-[1.25rem] border-t border-hair px-[1.4rem] py-[0.7rem] text-left last:rounded-b-panel"
        >
          <span className="min-w-0 flex-1">
            <span className="block text-body font-semibold">
              {a.symbol} {a.dir === "above" ? "above" : "below"} {usdPrice(a.target)}
            </span>
            <span className="flex items-center gap-[0.45rem] text-small text-muted">
              <Dot tone={a.active ? "brand" : "muted"} />
              {a.active ? "Watching" : "Hit"}
            </span>
          </span>
          <span className="text-small text-muted">Delete</span>
        </button>
      ))}
    </Panel>
  );
}
