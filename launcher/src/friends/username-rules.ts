// Rare username rules, for instant feedback while typing (the server is the judge:
// market api/src/usernames/rules.ts). 3 to 20 of a-z 0-9 _ . ; no leading or trailing . or _ ;
// no two . or _ in a row. Names are lowercase.

export const MIN_LEN = 3;
export const MAX_LEN = 20;

/** What the player typed, as a candidate: trimmed, no @, lowercase. */
export const normalizeUsername = (s: string) => s.trim().replace(/^@/, "").toLowerCase();

/** A plain sentence when the name breaks a rule, else null. */
export function usernameProblem(raw: string): string | null {
  const u = normalizeUsername(raw);
  if (!u) return null;
  if (!/^[a-z0-9_.]+$/.test(u)) return "Use letters, numbers, _ and . only.";
  if (/^[._]|[._]$/.test(u)) return "Start and end with a letter or number.";
  if (/[._]{2}/.test(u)) return "No two . or _ in a row.";
  if (u.length < MIN_LEN) return `Use ${MIN_LEN} to ${MAX_LEN} characters.`;
  if (u.length > MAX_LEN) return `Use ${MIN_LEN} to ${MAX_LEN} characters.`;
  return null;
}

export type Target = { kind: "address"; address: string } | { kind: "suins"; name: string } | { kind: "username"; username: string };

/** What the add friend box holds: a 0x address, a .sui name, or a username (with or without @). */
export function parseTarget(raw: string): Target | null {
  const t = raw.trim();
  if (!t) return null;
  if (/^0x[0-9a-fA-F]{1,64}$/.test(t)) return { kind: "address", address: t.toLowerCase() };
  if (/^@/.test(t)) return { kind: "username", username: normalizeUsername(t) };
  // name.sui, or the wallet's older form pay@alice
  if (/\.sui$/i.test(t) || /^[^@]+@[^@]+$/.test(t)) return { kind: "suins", name: t.toLowerCase() };
  return { kind: "username", username: normalizeUsername(t) };
}
