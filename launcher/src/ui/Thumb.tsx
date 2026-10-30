import { useState } from "react";
import { artDecoding, isReady, whenDecoded } from "../lib/img";

/**
 * Remote art that fades in once loaded (opacity only) and falls back to a quiet tile with the
 * name's initial, never a broken-image icon. Marketplace art goes through MImg instead.
 */
export function Thumb({ src, label, className = "", contain = false, eager = false }: { src: string | null | undefined; label: string; className?: string; contain?: boolean; eager?: boolean }) {
  const [state, setState] = useState<{ src: string | null | undefined; ok: boolean; bad: boolean }>(() => ({ src, ok: isReady(src), bad: false }));
  const cur = state.src === src ? state : { src, ok: isReady(src), bad: false };
  if (cur !== state) setState(cur);
  if (!src || cur.bad) {
    const m = label.trim().match(/[\p{L}\p{N}]/u);
    return <div className={`${className} flex items-center justify-center bg-l3 text-[1.4rem] font-semibold text-fg/35`}>{m ? m[0].toUpperCase() : ""}</div>;
  }
  return (
    <img
      // WebKitGTK may not fire onLoad for a cached image; the ref catches that so it never sticks faded.
      ref={(el) => {
        if (el && !cur.ok && el.complete && el.naturalWidth > 0) whenDecoded(el, () => setState((s) => (s.src === src && !s.ok ? { ...s, ok: true } : s)));
      }}
      src={src}
      alt=""
      draggable={false}
      decoding={artDecoding()}
      loading={eager ? undefined : "lazy"}
      onLoad={(e) => whenDecoded(e.currentTarget, () => setState((s) => (s.src === src ? { ...s, ok: true } : s)))}
      onError={() => setState((s) => (s.src === src ? { ...s, bad: true } : s))}
      className={`${className} ${contain ? "object-contain" : "object-cover"} transition-opacity duration-300 ${cur.ok ? "opacity-100" : "opacity-0"}`}
    />
  );
}
