// How a Rare player is shown, everywhere: username first. A SuiNS name the player also owns is a
// quiet second line (muted text, no pill). With neither, a short address. Pure (display.test.ts).

export interface Identity {
  address: string;
  /** What rare.market sends to show: the username, else the SuiNS name. */
  name?: string | null;
  username?: string | null;
  suins?: string | null;
}

export const shortAddr = (a: string) => `${a.slice(0, 6)}...${a.slice(-4)}`;

export interface Shown {
  primary: string;
  /** The SuiNS name, only when a username leads. */
  secondary: string | null;
  kind: "username" | "suins" | "address";
}

export function shown(p: Identity | null | undefined, fallback = "Someone"): Shown {
  if (!p) return { primary: fallback, secondary: null, kind: "address" };
  const username = p.username ?? null;
  const suins = p.suins ?? null;
  if (username) return { primary: username, secondary: suins && suins !== username ? suins : null, kind: "username" };
  // Older answers carry only `name`; it is a SuiNS name when it ends in .sui, else a username.
  const name = p.name ?? null;
  if (name && !suins && !/\.sui$/i.test(name)) return { primary: name, secondary: null, kind: "username" };
  if (suins ?? name) return { primary: (suins ?? name)!, secondary: null, kind: "suins" };
  return { primary: shortAddr(p.address), secondary: null, kind: "address" };
}

export const nameOf = (p: Identity | null | undefined, fallback = "Someone") => shown(p, fallback).primary;
