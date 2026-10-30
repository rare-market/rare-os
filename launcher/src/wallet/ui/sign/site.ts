// Plain-words checks on the site a request came from (web games).
// "Not connected" is skipped because a game must be allowed before it can ask.

import type { Risk } from "../../review";

export function siteRisks(url: string | undefined): Risk[] {
  if (!url) return [];
  const out: Risk[] = [];
  let u: URL | null = null;
  try {
    u = new URL(url);
  } catch {
    out.push({ id: "site-invalid", text: "Could not verify this site address.", danger: true });
  }
  if (u) {
    if (u.protocol === "http:" && !/^(localhost|127\.0\.0\.1)$/.test(u.hostname)) out.push({ id: "site-insecure", text: "This site is not using HTTPS.", danger: true });
    if (u.hostname.split(".").some((p) => p.startsWith("xn--"))) out.push({ id: "site-lookalike", text: "This address uses special characters and may imitate another site.", danger: true });
  }
  return out;
}

/** UTF-8 text when the bytes are printable, else null; plus the hex. */
export function decodeMessage(bytes: Uint8Array): { text: string | null; hex: string } {
  const hex = [...bytes].map((x) => x.toString(16).padStart(2, "0")).join("");
  try {
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    // eslint-disable-next-line no-control-regex
    if (/[\x00-\x08\x0e-\x1f]/.test(text)) return { text: null, hex };
    return { text, hex };
  } catch {
    return { text: null, hex };
  }
}

export const hostOf = (url?: string) => {
  try {
    return url ? new URL(url).host : "";
  } catch {
    return url ?? "";
  }
};
