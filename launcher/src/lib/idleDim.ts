import { native, onPad, onPadStick } from "./bridge";
import { useApp } from "./store";

const IDLE_MS = 60_000;

/** Dim after idle: 60 s without input in the launcher, never with a game running or a video playing.
 * The backend ignores it unless the setting is on and never raises the brightness. Any input restores it. */
export function startIdleDim(): () => void {
  let dimmed = false;
  let timer: number | undefined;
  const arm = () => {
    clearTimeout(timer);
    timer = window.setTimeout(fire, IDLE_MS);
  };
  const fire = () => {
    const playing = [...document.querySelectorAll("video")].some((v) => !v.paused && !v.ended);
    if (useApp.getState().running.length > 0 || playing || !document.hasFocus()) return arm();
    dimmed = true;
    void native.idleDim(true).catch(() => {});
  };
  const poke = () => {
    if (dimmed) {
      dimmed = false;
      void native.idleDim(false).catch(() => {});
    }
    arm();
  };
  const evs = ["pointerdown", "pointermove", "keydown", "touchstart", "wheel"] as const;
  evs.forEach((e) => window.addEventListener(e, poke, { passive: true }));
  const unlisten: Array<Promise<() => void>> = [onPad(poke), onPadStick((x, y) => (Math.abs(x) > 0.2 || Math.abs(y) > 0.2) && poke())];
  arm();
  return () => {
    clearTimeout(timer);
    evs.forEach((e) => window.removeEventListener(e, poke));
    unlisten.forEach((u) => void u.then((f) => f()));
    if (dimmed) void native.idleDim(false).catch(() => {});
  };
}
