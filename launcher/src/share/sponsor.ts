// "Rare covers it": a hook for a sponsor that stores a share on Walrus so the player pays nothing.
// Sponsoring itself lives elsewhere (the gas sponsor service); this file only says what Share
// needs from it. getSponsor() is null unless one is set with setShareSponsor() or the build sets
// VITE_RARE_SHARE_SPONSOR to the sponsor's base URL, and the Share screen offers "Rare covers it"
// only when it is non-null and the quote says yes.
//
// HTTP shape the URL form expects (JSON unless noted):
//   GET  <base>/quote?bytes=<quilt bytes>&epochs=<n>  -> { covered: boolean, walFrost?: string, note?: string }
//   POST <base>/store?epochs=<n>  multipart/form-data, one part per quilt file named by its
//        identifier (share.json, shot.webp or clip.mp4, poster.jpg)  -> { quiltId: string, blobObjectId?: string }

export interface SponsorFile {
  identifier: string;
  mime: string;
  bytes: Uint8Array;
}

export interface SponsorQuote {
  /** false: the sponsor won't take this one (too big, daily limit); the player can still pay */
  covered: boolean;
  /** what it costs the sponsor, for display only */
  walFrost: bigint | null;
  note: string | null;
}

export interface ShareSponsor {
  quote(bytes: number, epochs: number): Promise<SponsorQuote>;
  /** Stores the quilt; the result is the same quilt id the player's own upload would give. */
  store(files: SponsorFile[], epochs: number): Promise<{ quiltId: string; blobObjectId: string | null }>;
}

let current: ShareSponsor | null = null;

/** Set by the sponsor integration; null turns it off. */
export function setShareSponsor(s: ShareSponsor | null) {
  current = s;
}

export function getSponsor(): ShareSponsor | null {
  if (current) return current;
  const base = (import.meta.env?.VITE_RARE_SHARE_SPONSOR as string | undefined)?.replace(/\/+$/, "");
  return base ? (current = httpSponsor(base)) : null;
}

function httpSponsor(base: string): ShareSponsor {
  return {
    async quote(bytes, epochs) {
      const r = await fetch(`${base}/quote?bytes=${bytes}&epochs=${epochs}`);
      if (!r.ok) return { covered: false, walFrost: null, note: null };
      const j = (await r.json()) as { covered?: boolean; walFrost?: string; note?: string };
      return { covered: !!j.covered, walFrost: j.walFrost ? BigInt(j.walFrost) : null, note: j.note ?? null };
    },
    async store(files, epochs) {
      const form = new FormData();
      for (const f of files) form.append(f.identifier, new Blob([f.bytes as BlobPart], { type: f.mime }), f.identifier);
      const r = await fetch(`${base}/store?epochs=${epochs}`, { method: "POST", body: form });
      if (!r.ok) throw new Error(`Rare couldn't store this share (${r.status})`);
      const j = (await r.json()) as { quiltId: string; blobObjectId?: string };
      return { quiltId: j.quiltId, blobObjectId: j.blobObjectId ?? null };
    },
  };
}
