// Key presses on the amount pad as a pure function, so the digits-and-one-dot rules are tested.

/** Applies one key ("0"-"9", ".", "back") to the amount text. `decimals` caps the fraction digits. */
export function padPress(value: string, key: string, decimals: number): string {
  if (key === "back") return value.slice(0, -1);
  if (key === ".") {
    if (decimals <= 0 || value.includes(".")) return value;
    return value === "" ? "0." : value + ".";
  }
  if (!/^\d$/.test(key)) return value;
  const [whole, frac] = value.split(".");
  if (value.includes(".")) return frac.length >= decimals ? value : value + key;
  if (whole.length >= 15) return value;
  // no leading zeros: "0" then "5" is "5", and "0" then "0" stays "0"
  if (whole === "0") return key;
  return value + key;
}

/** Text typed on a keyboard or pasted, cleaned to the same rules. */
export function cleanTyped(text: string, decimals: number): string {
  let out = "";
  for (const ch of text.replace(",", ".")) out = padPress(out, ch, decimals);
  return out;
}
