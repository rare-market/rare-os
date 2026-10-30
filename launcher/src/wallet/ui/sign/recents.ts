// Recent recipients, kept on this device only (localStorage), newest first.

const KEY = "rare.wallet.recents";

export interface Recent {
  address: string;
  suins?: string;
  at: number;
}

export function recents(): Recent[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((r) => r && typeof r.address === "string") : [];
  } catch {
    return [];
  }
}

export function noteSent(address: string, suins?: string | null) {
  try {
    const a = address.toLowerCase();
    const list = [{ address: a, suins: suins ?? undefined, at: Date.now() }, ...recents().filter((r) => r.address !== a)].slice(0, 20);
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    /* private mode: recents are a convenience only */
  }
}

/** "Today", "3d ago", "Mar 4" */
export function since(ts: number, now = Date.now()): string {
  const days = Math.floor((now - ts) / 864e5);
  if (days < 1) return "Today";
  if (days < 7) return `${days}d ago`;
  return new Date(ts).toLocaleDateString([], { month: "short", day: "numeric" });
}
