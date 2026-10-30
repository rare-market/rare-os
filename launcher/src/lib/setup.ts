// What first run leaves behind for Home: when setup finished, so the "move from your old SuiPlay
// wallet" card shows for 14 days, and whether the player dismissed it or finished moving.

const DONE = "rare.setup.done";
const HIDDEN = "rare.move.card.hidden";
const DAYS = 14;

export function markSetupDone() {
  try {
    localStorage.setItem(DONE, String(Date.now()));
  } catch {
    /* private mode: no move card */
  }
}

export function moveCardVisible(now = Date.now()): boolean {
  if (new URLSearchParams(location.search).has("setupdone")) return true; // screenshots
  try {
    if (localStorage.getItem(HIDDEN)) return false;
    const t = Number(localStorage.getItem(DONE));
    return t > 0 && now - t < DAYS * 86_400_000;
  } catch {
    return false;
  }
}

export function hideMoveCard() {
  try {
    localStorage.setItem(HIDDEN, "1");
  } catch {
    /* ignore */
  }
}
