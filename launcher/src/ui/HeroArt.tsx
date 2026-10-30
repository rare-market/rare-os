import { useState } from "react";
import { artDecoding, isReady, readyRef, whenDecoded } from "../lib/img";

/**
 * Key art behind a page header: the art fills the top of the page,
 * blends into the background (dark art never shows a seam), and plain linear scrims keep the
 * lockup on the left and the content below legible. No glow, no blur, nothing animated but the
 * one fade when the art loads. Never put a title over art that has its own title baked in.
 */
export function HeroArt({ src, height = "72%", position = "center 25%", side = "left" }: { src: string | null | undefined; height?: string; position?: string; side?: "left" | "none" }) {
  const [state, setState] = useState<{ src: typeof src; ok: boolean; bad: boolean }>(() => ({ src, ok: isReady(src), bad: false }));
  const cur = state.src === src ? state : { src, ok: isReady(src), bad: false };
  if (cur !== state) setState(cur);
  if (!src || cur.bad) return null;
  const show = () => setState((s) => (s.src === src && !s.ok ? { ...s, ok: true } : s));
  return (
    <div aria-hidden className="pointer-events-none absolute inset-x-0 top-0" style={{ height }}>
      <img
        src={src}
        alt=""
        draggable={false}
        // WebKitGTK may not fire onLoad for a cached image; the ref catches that so it never stays hidden.
        ref={cur.ok ? undefined : readyRef(show)}
        decoding={artDecoding()}
        onLoad={(e) => whenDecoded(e.currentTarget, show)}
        onError={() => setState((s) => (s.src === src ? { ...s, bad: true } : s))}
        className={`absolute inset-0 h-full w-full object-cover mix-blend-lighten transition-opacity duration-300 ${cur.ok ? "opacity-100" : "opacity-0"}`}
        style={{ objectPosition: position }}
      />
      <div className="absolute inset-x-0 bottom-0 h-[45%] bg-gradient-to-b from-transparent to-bg" />
      {side === "left" && <div className="absolute inset-y-0 left-0 w-[60%] bg-gradient-to-r from-bg/90 via-bg/50 to-transparent" />}
    </div>
  );
}
