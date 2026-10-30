// Art images: how they decode, when they show, and what stays cached.
//
// GPU renderer (RARE_WEBKIT=gpu and the other GPU modes): the page is drawn as composited layers in
// tiles. An <img decoding="async"> whose decoded pixels are not ready for the size a tile wants is
// drawn as nothing and decoded later (RenderBoxModelObject::decodingModeForImageDraw, WebKit 2.54),
// so every image on the screen vanished for one frame. Art there uses decoding="sync".
//
// CPU renderer (the default since d6377be): sync decoding only moves big decodes onto the main
// thread (the Home shelf lagged on the SuiPlay), so art decodes async.
//
// Art fades in once loaded and decoded, but never waits on img.decode() for more than
// DECODE_WAIT_MS: a decode that is slow or never settles must not leave an image invisible.

let mode = "cpu";

/** The renderer the launcher runs with (render.rs `mode_name`), set once at start. */
export function setRenderMode(m: string | null | undefined) {
  mode = (m || "cpu").toLowerCase();
}

/** The decoding attribute for every art image: sync only on WebKit's GPU renderers. */
export function artDecoding(): "sync" | "async" {
  return mode === "cpu" || mode === "desktop" ? "async" : "sync";
}

/** The longest an image waits on img.decode() before it is shown anyway. */
export const DECODE_WAIT_MS = 120;

// Art that has loaded once shows at once when it mounts again (tab switches, a remounted shelf),
// with no fade and no decode wait. The elements are kept (newest last, a bounded few) so WebKit's
// memory cache keeps their decoded pixels while another screen is up.
const READY_MAX = 96;
const ready = new Map<string, HTMLImageElement>();

/** True when this source loaded before in this session. */
export function isReady(src: string | null | undefined): boolean {
  return !!src && ready.has(src);
}

function markReady(src: string, el: HTMLImageElement) {
  if (!src) return;
  ready.delete(src);
  ready.set(src, el);
  while (ready.size > READY_MAX) ready.delete(ready.keys().next().value as string);
}

/**
 * Calls `done` once, as soon as the image is decoded, could not be decoded, or DECODE_WAIT_MS has
 * passed, whichever comes first. It always calls `done`.
 */
export function whenDecoded(el: HTMLImageElement, done: () => void) {
  let fired = false;
  const go = () => {
    if (fired) return;
    fired = true;
    clearTimeout(timer);
    if (el.naturalWidth > 0) markReady(el.getAttribute("src") ?? el.src, el);
    done();
  };
  const timer = setTimeout(go, DECODE_WAIT_MS);
  if (typeof el.decode !== "function") return go();
  try {
    el.decode().then(go, go);
  } catch {
    go();
  }
}

/** A ref for an art <img>: an image that is already complete when it mounts (cached; WebKitGTK may never fire onLoad for it) becomes ready too. */
export function readyRef(done: () => void) {
  return (el: HTMLImageElement | null) => {
    if (el && el.complete && el.naturalWidth > 0) whenDecoded(el, done);
  };
}
