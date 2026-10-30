import { useEffect, useMemo, useState } from "react";
import { imageChain, type ImgOpts } from "./media";
import { artDecoding, isReady, readyRef, whenDecoded } from "../lib/img";

/**
 * Marketplace art: walks the image chain (rare's image front, wsrv, originals, passthrough) and ends
 * on a quiet tile with the name's initial. Never the broken-image icon.
 */
export function MImg({ src, label, width = 400, className = "", opts, contain, onLoaded }: { src: string | null | undefined; label: string; width?: number; className?: string; opts?: ImgOpts; contain?: boolean; onLoaded?: () => void }) {
  const chain = useMemo(() => imageChain(src, width, opts), [src, width, opts?.fit]); // eslint-disable-line react-hooks/exhaustive-deps
  const key = chain.join("|");
  const [st, setSt] = useState(() => ({ key, i: 0, loaded: isReady(chain[0]) }));
  // A new source resets during render, so a cached image's onLoad is never undone by a later effect.
  const cur = st.key === key ? st : { key, i: 0, loaded: isReady(chain[0]) };
  if (cur !== st) setSt(cur);
  const at = cur.i;
  const show = () => setSt((s) => (s.key === key && s.i === at && !s.loaded ? { ...s, loaded: true } : s));
  // Also for art that was already loaded (isReady), which may never fire onLoad again.
  useEffect(() => {
    if (cur.loaded) onLoaded?.();
  }, [cur.loaded, key]); // eslint-disable-line react-hooks/exhaustive-deps
  if (cur.i >= chain.length) {
    const m = label.trim().match(/[\p{L}\p{N}]/u);
    return <div className={`${className} flex items-center justify-center bg-layer-03 text-[1.6rem] font-semibold text-secondary/60`}>{m ? m[0].toUpperCase() : ""}</div>;
  }
  return (
    <img
      key={chain[cur.i]}
      src={chain[cur.i]}
      alt=""
      draggable={false}
      // WebKitGTK may not fire onLoad for a cached image; the ref catches that so it never stays hidden.
      ref={cur.loaded ? undefined : readyRef(show)}
      decoding={artDecoding()}
      onLoad={(e) => whenDecoded(e.currentTarget, show)}
      onError={() => setSt((s) => (s.key === key && s.i === at ? { ...s, i: s.i + 1, loaded: isReady(chain[s.i + 1]) } : s))}
      className={`${className} ${contain ? "object-contain" : "object-cover"} transition-opacity duration-300 ${cur.loaded ? "opacity-100" : "opacity-0"}`}
    />
  );
}
