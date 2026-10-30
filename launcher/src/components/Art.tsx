import { useCallback, useMemo, useState } from "react";
import { artSrc, type ArtSize } from "../lib/state";
import { artDecoding, isReady, whenDecoded } from "../lib/img";

/** Stable gradient for a title, used when a game has no artwork at all. */
export function titleHue(title: string) {
  let h = 0;
  for (const c of title) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

/** A game with no artwork at all: the layer colour with its name, like rare.market's media tiles. */
export function Generated({ title, variant }: { title: string; variant: "cover" | "hero" | "header" }) {
  return (
    <div className={`absolute inset-0 flex items-center justify-center p-[10%] ${variant === "hero" ? "bg-l2" : "bg-l3"}`}>
      {variant !== "hero" && (
        <span className={`text-center font-semibold leading-tight text-fg/80 ${variant === "cover" ? "text-[1.05rem]" : "text-[0.95rem]"}`}>{title}</span>
      )}
    </div>
  );
}

/** Tries each source in order and falls back to a generated tile. */
export function Art({
  sources,
  title,
  variant,
  className = "",
  onFail,
  priority,
}: {
  sources: string[];
  title: string;
  variant: "cover" | "hero" | "header" | "logo";
  className?: string;
  onFail?: () => void;
  /** First screen art (the Home shelf): fetched first. */
  priority?: boolean;
}) {
  // Through Rust's image cache at the size this variant is drawn at (Steam's librarycache files too).
  const size: ArtSize = variant === "hero" ? "hero" : variant === "header" ? "card" : variant === "logo" ? "w720" : "tile";
  const list = useMemo(() => sources.filter(Boolean).map((s) => artSrc(s, size)), [sources, size]);
  const key = list.join("|");
  // A new source list resets during render, never in an effect: an effect that reset `loaded`
  // after mount ran after a cached image had already reported in, which left every Home shelf
  // tile at opacity 0 when Home came back (WebKitGTK 2.54 on the SuiPlay).
  const [st, setSt] = useState(() => ({ key, i: 0, loaded: isReady(list[0]) }));
  const cur = st.key === key ? st : { key, i: 0, loaded: isReady(list[0]) };
  if (cur !== st) setSt(cur);
  const i = cur.i;
  const loaded = cur.loaded || isReady(list[i]);
  const show = useCallback(() => setSt((s) => (s.key === key && s.i === i ? { ...s, loaded: true } : s)), [key, i]);
  // WebKitGTK often does not fire onLoad for an already-cached image. The ref (new for every
  // source) checks `complete` on mount; onLoad covers the first, uncached load.
  const onRef = useCallback(
    (el: HTMLImageElement | null) => {
      if (el && el.complete && el.naturalWidth > 0) whenDecoded(el, show);
    },
    [show],
  );
  if (i >= list.length) {
    if (variant === "logo") return null;
    return <Generated title={title} variant={variant} />;
  }
  return (
    <img
      key={list[i]}
      ref={onRef}
      src={list[i]}
      alt=""
      draggable={false}
      decoding={artDecoding()}
      fetchPriority={priority ? "high" : undefined}
      onLoad={(e) => whenDecoded(e.currentTarget, show)}
      onError={() => {
        if (i + 1 >= list.length) onFail?.();
        setSt((s) => (s.key === key && s.i === i ? { key, i: i + 1, loaded: false } : s));
      }}
      className={`${className} transition-opacity duration-300 ${loaded ? "opacity-100" : "opacity-0"}`}
    />
  );
}
