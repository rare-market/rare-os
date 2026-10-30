// Settings search: pure matching over the typed index (index.ts). No React in here.

import { SECTIONS, sectionLabel, type SectionId } from "./sections";

/** One row of Settings. `id` is the row's data-setting-id; search jumps to it. */
export interface SettingEntry {
  id: string;
  section: SectionId;
  title: string;
  keywords: string[];
}

export interface SectionHits {
  section: SectionId;
  label: string;
  entries: SettingEntry[];
}

const words = (s: string) => s.toLowerCase().split(/[^a-z0-9]+/).filter(Boolean);

/** How well one query word fits a field's words: 3 exact start of a word, 2 inside a word, 0 none. */
function wordScore(q: string, fieldWords: string[]): number {
  let best = 0;
  for (const w of fieldWords) {
    if (w.startsWith(q)) return 3;
    if (q.length >= 3 && w.includes(q)) best = 2;
  }
  return best;
}

/** Every query word has to fit somewhere. Title counts most, then keywords, then the section name. */
export function scoreEntry(query: string, e: SettingEntry): number {
  const qs = words(query);
  if (!qs.length) return 0;
  const title = words(e.title);
  const keys = e.keywords.flatMap(words);
  const sect = words(sectionLabel(e.section));
  let total = 0;
  for (const q of qs) {
    const t = wordScore(q, title);
    const k = wordScore(q, keys);
    const s = wordScore(q, sect);
    const best = Math.max(t * 3, k * 2, s);
    if (best === 0) return 0;
    total += best;
  }
  return total;
}

/** Matches grouped by section (in the order of the left list), best first inside a section. */
export function searchSettings(query: string, entries: SettingEntry[]): SectionHits[] {
  const scored = entries.map((e) => ({ e, s: scoreEntry(query, e) })).filter((x) => x.s > 0);
  const out: SectionHits[] = [];
  for (const sec of SECTIONS) {
    const hits = scored.filter((x) => x.e.section === sec.id).sort((a, b) => b.s - a.s);
    if (hits.length) out.push({ section: sec.id, label: sec.label, entries: hits.map((x) => x.e) });
  }
  return out;
}
