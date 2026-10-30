// Rareling launcher themes: accent variants of the shell, each built only from rare.market's own
// colour tokens (market/web/app/globals.css). A theme swaps the brand family (buttons, the current
// tab, the focus glow, the loading bar); surfaces, text, green and red never change.

export interface Theme {
  id: string;
  label: string;
  /** what it is made of, in rare.market's token names */
  note: string;
  /** token -> bare RGB triplet; null keeps app.css as it is */
  vars: Record<string, string> | null;
  swatch: string;
}

export const THEMES: Theme[] = [
  { id: "rare", label: "Rare blue", note: "brand", vars: null, swatch: "61 90 254" },
  {
    id: "royal",
    label: "Royal",
    note: "text-royalties",
    // light theme's royalties violet as the brand, the dark theme's as the label tint
    vars: { brand: "122 53 255", "brand-darker": "122 53 255", "labels-brand": "212 189 255", "hero-glow": "122 53 255" },
    swatch: "122 53 255",
  },
  {
    id: "midnight",
    label: "Midnight",
    note: "brand-darker",
    vars: { brand: "47 72 217", "brand-darker": "47 72 217", "labels-brand": "92 118 255", "hero-glow": "47 72 217" },
    swatch: "47 72 217",
  },
  {
    id: "periwinkle",
    label: "Periwinkle",
    note: "labels-brand",
    vars: { brand: "92 118 255", "brand-darker": "61 90 254", "labels-brand": "138 155 255", "hero-glow": "138 155 255" },
    swatch: "92 118 255",
  },
];

const VAR_NAMES = [...new Set(THEMES.flatMap((t) => Object.keys(t.vars ?? {})))];

export const themeById = (id: string | null | undefined) => THEMES.find((t) => t.id === id) ?? THEMES[0];

/** Puts a theme on the page (or back to app.css's tokens). */
export function applyTheme(id: string | null, root: HTMLElement | null = typeof document === "undefined" ? null : document.documentElement) {
  if (!root) return;
  const t = themeById(id);
  for (const k of VAR_NAMES) root.style.removeProperty(`--${k}`);
  for (const [k, v] of Object.entries(t.vars ?? {})) root.style.setProperty(`--${k}`, v);
  root.dataset.accent = t.id;
}

const key = (account: string) => `rare.perks.theme.${account.toLowerCase()}`;

export function readTheme(account: string | null): string {
  if (!account) return "rare";
  try {
    return themeById(localStorage.getItem(key(account))).id;
  } catch {
    return "rare";
  }
}

export function saveTheme(account: string, id: string) {
  try {
    localStorage.setItem(key(account), themeById(id).id);
  } catch {
    /* this session only */
  }
}
