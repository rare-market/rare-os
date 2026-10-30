// Journey probe for tools/journeys, the automated experience runner. Off unless the runner made
// $XDG_RUNTIME_DIR/rare-journey (checked on a pad press, at most every 2 s, never on a timer while
// off). While on it logs, stamped with CLOCK_MONOTONIC on the native side:
//   screen tab=<tab> overlay=<kind[:id]|-> running=<n> launching=<id|->   on every change
//   painted                                                             second frame after a change
//   focus <element>                                                     focused element (see describe)
//   pad <button>[ repeat]                                               every press the UI handled
//   toast <text>
//   ingame mode=<panel|running|-> game=<id|->                            the in-game panel / Running list
// and takes setup commands from rare-journey.cmd: tab:<tab>, open:game:<id>, open:app:<id>,
// open:<qam|running|search|profile>, close, quit:<id|all> (cleanup), state, echo:<text>.

import { native } from "../lib/bridge";
import { useApp, type Overlay, type Tab } from "../lib/store";
import { useOverlay } from "../overlay/session";
import { describe } from "./debug";
import { onDispatch } from "./input";

let on = false;
let lastCheck = 0;
let pending: string[] = [];
let queued = false;
let timer: ReturnType<typeof setInterval> | null = null;
let unsubs: Array<() => void> = [];

function log(line: string) {
  if (!on) return;
  pending.push(line);
  if (queued) return;
  queued = true;
  queueMicrotask(() => {
    queued = false;
    const batch = pending;
    pending = [];
    void native.journeyLog(batch).catch(() => {});
  });
}

function overlayName(o: Overlay): string {
  if (!o) return "-";
  const id =
    "id" in o
      ? o.id
      : "slug" in o
        ? o.slug
        : "appid" in o && o.appid
          ? String(o.appid)
          : "";
  return id ? `${o.kind}:${id}` : o.kind;
}

function screenLine(): string {
  const s = useApp.getState();
  return `screen tab=${s.tab} overlay=${overlayName(s.overlay)} running=${s.running.length} launching=${s.launching ?? "-"}`;
}

const painted = () =>
  requestAnimationFrame(() => requestAnimationFrame(() => log("painted")));

let focusQueued = false;
function onFocus() {
  if (focusQueued) return;
  focusQueued = true;
  requestAnimationFrame(() => {
    focusQueued = false;
    log(`focus ${describe(document.activeElement)}`);
  });
}

function command(c: string) {
  const s = useApp.getState();
  if (c.startsWith("tab:")) s.setTab(c.slice(4) as Tab);
  else if (c.startsWith("open:game:"))
    s.open({ kind: "game", id: c.slice(10) });
  else if (c.startsWith("open:app:")) s.open({ kind: "app", id: c.slice(9) });
  else if (
    ["open:qam", "open:running", "open:search", "open:profile"].includes(c)
  )
    s.open({ kind: c.slice(5) } as Overlay);
  else if (c === "close") s.close();
  else if (c.startsWith("quit:")) {
    const id = c.slice(5);
    for (const r of id === "all" ? s.running : [id])
      void native.quit(r).then(
        () => log(`quit ${r}`),
        (e) => log(`quit ${r} failed ${String(e)}`),
      );
  } else if (c.startsWith("echo:")) log(`echo ${c.slice(5)}`);
  else if (c !== "state") log(`unknown command ${c}`);
  log(screenLine());
  log(`focus ${describe(document.activeElement)}`);
}

function enable() {
  on = true;
  log("probe on");
  log(screenLine());
  log(`focus ${describe(document.activeElement)}`);
  let last = screenLine();
  let toasts = useApp.getState().toasts.length;
  const ingame = () => {
    const o = useOverlay.getState().session;
    return `ingame mode=${o ? o.mode : "-"} game=${o?.gameId ?? "-"}`;
  };
  let lastIngame = ingame();
  log(lastIngame);
  unsubs.push(
    useOverlay.subscribe(() => {
      const now = ingame();
      if (now !== lastIngame) {
        lastIngame = now;
        log(now);
        painted();
      }
    }),
  );
  unsubs.push(
    useApp.subscribe((st) => {
      const now = screenLine();
      if (now !== last) {
        last = now;
        log(now);
        painted();
      }
      if (st.toasts.length > toasts)
        st.toasts
          .slice(toasts)
          .forEach((t) => log(`toast ${t.tone} ${t.text}`));
      toasts = st.toasts.length;
    }),
  );
  document.addEventListener("focusin", onFocus);
  let tick = 0;
  timer = setInterval(() => {
    void native.journeyCmd().then(
      (c) => c && c.split("\n").forEach((l) => command(l.trim())),
      () => {},
    );
    if (++tick % 8 === 0)
      void native.journeyProbe().then(
        (still) => !still && disable(),
        () => {},
      );
  }, 250);
}

function disable() {
  on = false;
  unsubs.forEach((u) => u());
  unsubs = [];
  if (timer) clearInterval(timer);
  timer = null;
  document.removeEventListener("focusin", onFocus);
}

function check() {
  const now = performance.now();
  if (on || now - lastCheck < 2000) return;
  lastCheck = now;
  void native.journeyProbe().then(
    (yes) => yes && !on && enable(),
    () => {},
  );
}

export function startJourneyProbe() {
  onDispatch((b, repeat) => {
    if (on) log(`pad ${b}${repeat ? " repeat" : ""}`);
    else check();
  });
  check();
}
