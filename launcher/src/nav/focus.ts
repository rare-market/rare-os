// Spatial focus for controller navigation.
//
// Anything with [data-nav] can take focus. A direction moves to the nearest element on
// that side, weighting distance along the axis far less than drift off it, so rails,
// grids and lists all behave as expected. [data-nav-scope] (modals, overlays) traps
// focus; the topmost scope wins. [data-nav-group] remembers its last focused child (by
// position, so it survives a screen being rebuilt), so coming back to a rail lands where
// you left it. Something always has focus: when the focused element goes away, focus is
// restored to a saved spot or to the screen's default.

import { feedback } from "./feedback";
import { preloadArt } from "../lib/state";

const nativeArt: boolean = __RARE_NATIVE__;

export type Dir = "up" | "down" | "left" | "right";

/** A place to come back to: a group and an index in it, plus the page's scroll offset. */
export interface Spot {
  group: string | null;
  index: number;
  scrollTop: number | null;
}

const memory = new Map<string, number>();
// True while focus sits where the engine put it on its own (not where the user moved it).
// Then a screen's [data-nav-default] that shows up late (after data loads) still wins.
let auto = false;
let pending: { spot: Spot; until: number } | null = null;

function scopeRoot(): ParentNode {
  const scopes = Array.from(document.querySelectorAll<HTMLElement>("[data-nav-scope]"));
  return scopes.length ? scopes[scopes.length - 1] : document;
}

function visible(el: HTMLElement): boolean {
  if (el.closest("[inert]")) return false;
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0;
}

export function focusables(root: ParentNode = scopeRoot()): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>("[data-nav]")).filter((el) => !el.hasAttribute("disabled") && visible(el));
}

function groupOf(el: Element): HTMLElement | null {
  return el.closest<HTMLElement>("[data-nav-group]");
}

/** Rails and single rows of buttons go back to their last item; grids pick by position. */
function remembers(g: HTMLElement) {
  return g.hasAttribute("data-rail") || g.hasAttribute("data-nav-remember");
}

function groupItems(g: HTMLElement): HTMLElement[] {
  return Array.from(g.querySelectorAll<HTMLElement>("[data-nav]")).filter((el) => !el.hasAttribute("disabled") && groupOf(el) === g);
}

export function current(): HTMLElement | null {
  const a = document.activeElement as HTMLElement | null;
  if (a && a.hasAttribute("data-nav") && scopeRoot().contains(a) && !a.closest("[inert]")) return a;
  return null;
}

/** Where focus is now, to restore later (after an overlay closes or a tab comes back). */
export function spot(): Spot | null {
  const cur = current();
  if (!cur) return null;
  const g = groupOf(cur);
  const page = cur.closest<HTMLElement>("[data-scroll]");
  return {
    group: g?.dataset.navGroup ?? null,
    index: g ? groupItems(g).indexOf(cur) : focusables().indexOf(cur),
    scrollTop: page ? page.scrollTop : null,
  };
}

/** Asks for focus to go back to `s` as soon as that element exists (content may still be loading). */
export function restoreLater(s: Spot | null | undefined) {
  pending = s ? { spot: s, until: performance.now() + 2500 } : null;
  queueEnsure();
}

function resolve(s: Spot): HTMLElement | null {
  const root = scopeRoot();
  if (s.group) {
    const g = root.querySelector<HTMLElement>(`[data-nav-group="${CSS.escape(s.group)}"]`);
    if (!g) return null;
    const items = groupItems(g);
    return items[Math.min(s.index, items.length - 1)] ?? null;
  }
  return focusables(root)[s.index] ?? null;
}

// The focused element also carries [data-focused], so the ring shows even when the
// window itself isn't the active one (WebKit drops :focus styling then).
let marked: HTMLElement | null = null;
function mark(el: HTMLElement | null) {
  if (marked === el) return;
  marked?.removeAttribute("data-focused");
  marked = el;
  el?.setAttribute("data-focused", "");
}

// Where focus last was, so a re-render that replaces the focused element (a library rescan
// reordering a row, data refreshing) puts focus back in the same place instead of the top.
let lastSpot: { group: string; index: number } | null = null;

export function focusEl(el: HTMLElement | null | undefined, opts: { scroll?: boolean; smooth?: boolean } = {}) {
  if (!el) return;
  // Anyone placing focus on purpose (a page restoring what opened it, a restore spot) wins
  // over a late-loading default; only ensureFocus's own fallback stays "auto".
  auto = false;
  el.focus({ preventScroll: true });
  mark(el);
  const g = groupOf(el);
  let items: HTMLElement[] | null = null;
  if (g?.dataset.navGroup) {
    items = groupItems(g);
    const index = items.indexOf(el);
    memory.set(g.dataset.navGroup, index);
    lastSpot = { group: g.dataset.navGroup, index };
  } else lastSpot = null;
  if (opts.scroll !== false) reveal(el, opts.smooth !== false);
  if (nativeArt) preloadNear(el, items);
}

// Where focus can go next is in Rust's image memory before it gets there: the art of the next items
// in the rail or grid, and what the focused item opens ([data-prefetch]: its page's hero art).
// Once focus rests (held directions repeat every ~80 ms).
let preloadTimer: ReturnType<typeof setTimeout> | null = null;
function preloadNear(el: HTMLElement, items: HTMLElement[] | null) {
  if (preloadTimer) clearTimeout(preloadTimer);
  preloadTimer = setTimeout(() => {
    preloadTimer = null;
    if (!el.isConnected) return;
    const urls = (el.dataset.prefetch ?? "").split(" ").filter(Boolean);
    if (items) {
      const i = items.indexOf(el);
      for (const n of [...items.slice(i + 1, i + 9), ...items.slice(Math.max(0, i - 4), i)]) {
        for (const im of n.querySelectorAll("img")) if (!im.complete) urls.push(im.src);
        if (n.dataset.prefetch) urls.push(...n.dataset.prefetch.split(" "));
      }
    }
    preloadArt(urls);
  }, 120);
}

// Smooth scrolls are aimed at absolute targets. While one is still running, the next
// move computes from where the last one is heading, so fast repeats never undershoot.
const aims = new WeakMap<HTMLElement, { left: number; top: number; at: number }>();

function scrollTarget(box: HTMLElement, left: number | null, top: number | null, smooth: boolean) {
  const prev = aims.get(box);
  const fresh = prev && performance.now() - prev.at < 450;
  const l = left ?? (fresh ? prev!.left : box.scrollLeft);
  const t = top ?? (fresh ? prev!.top : box.scrollTop);
  const maxL = box.scrollWidth - box.clientWidth;
  const maxT = box.scrollHeight - box.clientHeight;
  const cl = Math.max(0, Math.min(maxL, l));
  const ct = Math.max(0, Math.min(maxT, t));
  aims.set(box, { left: cl, top: ct, at: performance.now() });
  if (smooth) glide(box, cl, ct);
  else {
    anims.delete(box);
    box.scrollLeft = cl;
    box.scrollTop = ct;
  }
}

// Our own scroll animation instead of scrollTo({behavior: "smooth"}): WebKitGTK restarts a
// smooth scroll from zero speed on every call, which stutters under held-direction repeats.
// This one eases toward the latest target each frame, so retargeting mid-flight stays fluid,
// and it only ever writes scrollLeft/scrollTop (no layout reads in the loop).
const anims = new Map<HTMLElement, { l: number; t: number; x: number; y: number }>();
let animating = false;
function glide(box: HTMLElement, l: number, t: number) {
  const a = anims.get(box);
  if (a) {
    a.l = l;
    a.t = t;
  } else anims.set(box, { l, t, x: box.scrollLeft, y: box.scrollTop });
  if (!animating) {
    animating = true;
    let prev = performance.now();
    const step = (now: number) => {
      // Frame-rate independent ease: ~90% of the way in 150 ms (time constant 65 ms).
      const k = 1 - Math.exp(-Math.min(64, now - prev) / 65);
      prev = now;
      for (const [b, s] of anims) {
        s.x += (s.l - s.x) * k;
        s.y += (s.t - s.y) * k;
        const doneX = Math.abs(s.l - s.x) < 0.5;
        const doneY = Math.abs(s.t - s.y) < 0.5;
        if (doneX) s.x = s.l;
        if (doneY) s.y = s.t;
        b.scrollLeft = s.x;
        b.scrollTop = s.y;
        if (doneX && doneY) anims.delete(b);
      }
      if (anims.size) requestAnimationFrame(step);
      else animating = false;
    };
    requestAnimationFrame(step);
  }
}

function aimed(box: HTMLElement, axis: "left" | "top"): number {
  const prev = aims.get(box);
  if (prev && performance.now() - prev.at < 450) return axis === "left" ? prev.left : prev.top;
  return axis === "left" ? box.scrollLeft : box.scrollTop;
}

/** Offset of `el` inside the scroll box, independent of the box's current scroll. */
function offsetIn(el: HTMLElement, box: HTMLElement): { left: number; top: number; width: number; height: number } {
  const r = el.getBoundingClientRect();
  const b = box.getBoundingClientRect();
  return { left: r.left - b.left + box.scrollLeft, top: r.top - b.top + box.scrollTop, width: r.width, height: r.height };
}

/** Scrolls the element into view inside its scrollers, horizontally and vertically. */
function reveal(el: HTMLElement, smooth = true) {
  const rail = el.closest<HTMLElement>("[data-rail]");
  if (rail && rail.scrollWidth > rail.clientWidth) {
    const o = offsetIn(el, rail);
    const pad = parseFloat(getComputedStyle(rail).paddingLeft) || 0;
    const view = aimed(rail, "left");
    if (o.left - pad < view) scrollTarget(rail, o.left - pad, null, smooth);
    else if (o.left + o.width + pad > view + rail.clientWidth) scrollTarget(rail, o.left + o.width + pad - rail.clientWidth, null, smooth);
  }
  const page = el.closest<HTMLElement>("[data-scroll]");
  if (page && page.scrollHeight > page.clientHeight) {
    // Keep the focused row a little above the middle, like a console shelf.
    const row = groupOf(el)?.closest<HTMLElement>("section") ?? null;
    const box = row && page.contains(row) && row.getBoundingClientRect().height < page.clientHeight * 0.8 ? row : el;
    const o = offsetIn(box, page);
    const view = aimed(page, "top");
    const sticky = page.querySelector<HTMLElement>("[data-sticky]");
    const topPad = (sticky?.offsetHeight ?? 0) + 12;
    const outside = o.top - topPad < view || o.top + o.height + 12 > view + page.clientHeight;
    if (outside || el.closest("[data-center]")) {
      const target = o.top + o.height / 2 - page.clientHeight * 0.45;
      scrollTarget(page, null, o.top - topPad < target ? o.top - topPad : target, smooth);
    }
  }
}

export function focusFirst(root: ParentNode = scopeRoot()) {
  const def = root.querySelector<HTMLElement>("[data-nav-default]");
  focusEl(def && visible(def) && !def.hasAttribute("disabled") ? def : focusables(root)[0], { smooth: false });
}

function center(r: DOMRect) {
  return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
}

export function move(dir: Dir): boolean {
  pending = null;
  auto = false;
  const cur = current();
  if (!cur) {
    focusFirst();
    return true;
  }
  const all = focusables();
  const a = cur.getBoundingClientRect();
  const { x: ax, y: ay } = center(a);
  const curGroupEl = groupOf(cur);
  const curGroup = curGroupEl?.dataset.navGroup;
  const horizontal = dir === "left" || dir === "right";
  let best: HTMLElement | null = null;
  let bestScore = Infinity;
  for (const el of all) {
    if (el === cur) continue;
    const b = el.getBoundingClientRect();
    const { x: bx, y: by } = center(b);
    let main: number;
    let cross: number;
    const overlapX = Math.max(b.left, a.left) < Math.min(b.right, a.right);
    const overlapY = Math.max(b.top, a.top) < Math.min(b.bottom, a.bottom);
    switch (dir) {
      case "right":
        if (bx <= ax + 1 || b.left < a.left + 1) continue;
        main = b.left - a.right;
        cross = overlapY ? 0 : Math.abs(by - ay);
        break;
      case "left":
        if (bx >= ax - 1 || b.right > a.right - 1) continue;
        main = a.left - b.right;
        cross = overlapY ? 0 : Math.abs(by - ay);
        break;
      case "down":
        if (by <= ay + 1 || b.top < a.top + 1) continue;
        main = b.top - a.bottom;
        cross = overlapX ? Math.abs(bx - ax) * 0.1 : Math.abs(bx - ax);
        break;
      case "up":
        if (by >= ay - 1 || b.bottom > a.bottom - 1) continue;
        main = a.top - b.bottom;
        cross = overlapX ? Math.abs(bx - ax) * 0.1 : Math.abs(bx - ax);
        break;
    }
    const g = groupOf(el)?.dataset.navGroup;
    // Left and right stay in the same rail or grid row while they can.
    const groupPenalty = horizontal && curGroup && g !== curGroup ? 5000 : 0;
    // Up and down prefer what is on screen over things scrolled off sideways.
    const offscreen = !horizontal && (b.right < 0 || b.left > innerWidth) ? 3000 : 0;
    const score = Math.max(0, main) + cross * 2.5 + groupPenalty + offscreen;
    if (score < bestScore) {
      bestScore = score;
      best = el;
    }
  }
  if (!best) return false;
  // Entering another rail, button row or side menu: go back to its remembered item.
  {
    const g = groupOf(best);
    const id = g?.dataset.navGroup;
    if (g && id && id !== curGroup && remembers(g)) {
      const i = memory.get(id);
      const items = groupItems(g);
      if (i != null && items[i]) best = items[i];
    }
  }
  focusEl(best);
  feedback("move");
  return true;
}

/**
 * LT / RT: jump a section. Inside a tall grid or list that runs past the screen it pages
 * by a screen instead; at the end of it the next jump leaves for the next section.
 */
export function jump(forward: boolean): boolean {
  pending = null;
  auto = false;
  const cur = current();
  if (!cur) {
    focusFirst();
    return true;
  }
  const page = cur.closest<HTMLElement>("[data-scroll]");
  const all = focusables();
  const curGroup = groupOf(cur);
  const a = cur.getBoundingClientRect();
  const viewH = page ? page.clientHeight : innerHeight;

  if (curGroup && !curGroup.hasAttribute("data-rail")) {
    const gr = curGroup.getBoundingClientRect();
    const moreThere = forward ? gr.bottom > a.bottom + 4 : gr.top < a.top - 4;
    if (moreThere && gr.height > viewH * 0.7) {
      const targetY = center(a).y + (forward ? 1 : -1) * viewH * 0.8;
      const ax = center(a).x;
      let best: HTMLElement | null = null;
      let bestD = Infinity;
      for (const el of groupItems(curGroup)) {
        const r = el.getBoundingClientRect();
        const c = center(r);
        if (forward ? c.y <= center(a).y + 1 : c.y >= center(a).y - 1) continue;
        const d = Math.abs(c.y - targetY) * 2 + Math.abs(c.x - ax);
        if (d < bestD) {
          bestD = d;
          best = el;
        }
      }
      if (best) {
        focusEl(best);
        feedback("move");
        return true;
      }
    }
  }

  // Next or previous section: the nearest group (or ungrouped element) fully past this one.
  const edge = curGroup ? curGroup.getBoundingClientRect() : a;
  let target: HTMLElement | null = null;
  let bestY = forward ? Infinity : -Infinity;
  for (const el of all) {
    const g = groupOf(el);
    if (g && g === curGroup) continue;
    const r = (g ?? el).getBoundingClientRect();
    if (forward ? r.top >= edge.bottom - 2 && r.top < bestY : r.bottom <= edge.top + 2 && r.top > bestY) {
      bestY = r.top;
      target = el;
    }
  }
  if (!target) {
    // Nothing further: go to the very first or last element.
    target = forward ? all[all.length - 1] : all[0];
    if (!target || target === cur) return false;
  }
  const g = groupOf(target);
  if (g) {
    const items = groupItems(g);
    const i = g.dataset.navGroup ? memory.get(g.dataset.navGroup) : undefined;
    target = (remembers(g) && i != null && items[i]) || items[0] || target;
  }
  focusEl(target);
  feedback("move");
  return true;
}

export function activate() {
  auto = false;
  const cur = current();
  if (cur) {
    feedback("select");
    cur.click();
  } else focusFirst();
}

// ---- keeping focus alive ----

let queued = false;
function queueEnsure() {
  if (queued) return;
  queued = true;
  requestAnimationFrame(() => {
    queued = false;
    ensureFocus();
  });
}

/** Puts focus somewhere sensible when nothing in the active scope has it. */
export function ensureFocus() {
  const a = document.activeElement as HTMLElement | null;
  if (a && (a.tagName === "INPUT" || a.tagName === "TEXTAREA") && scopeRoot().contains(a)) return;
  if (pending) {
    if (performance.now() > pending.until) pending = null;
    else {
      const el = resolve(pending.spot);
      if (el) {
        const s = pending.spot;
        pending = null;
        const page = el.closest<HTMLElement>("[data-scroll]");
        if (page && s.scrollTop != null) page.scrollTop = s.scrollTop;
        focusEl(el, { smooth: false });
        auto = false;
        return;
      }
      // The saved spot isn't there yet; keep waiting, but don't leave focus nowhere.
      if (current()) return;
    }
  }
  const cur = current();
  if (cur) {
    if (marked !== cur) mark(cur);
    if (auto) {
      const def = scopeRoot().querySelector<HTMLElement>("[data-nav-default]");
      if (def && def !== cur && visible(def) && !def.hasAttribute("disabled")) {
        focusEl(def, { smooth: false });
        auto = true;
      }
    }
    return;
  }
  if (marked && !marked.isConnected) marked = null;
  if (lastSpot) {
    const el = resolve({ group: lastSpot.group, index: lastSpot.index, scrollTop: null });
    if (el) {
      focusEl(el, { smooth: false });
      return;
    }
  }
  // Last resort: the screen's default. Give screens that restore focus themselves (the
  // Marketplace page stack, Trade) a couple of frames to do it first.
  if (fallbackTimer) return;
  fallbackTimer = setTimeout(() => {
    fallbackTimer = null;
    if (current()) return;
    const a = document.activeElement as HTMLElement | null;
    if (a && (a.tagName === "INPUT" || a.tagName === "TEXTAREA")) return;
    const root = scopeRoot();
    if (focusables(root).length) {
      focusFirst(root);
      auto = true;
    }
  }, 60);
}
let fallbackTimer: ReturnType<typeof setTimeout> | null = null;

let watching = false;
export function watchFocus() {
  if (watching) return;
  watching = true;
  new MutationObserver(queueEnsure).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["inert", "disabled", "data-nav-scope"] });
  document.addEventListener("focusout", () => setTimeout(queueEnsure, 0));
  document.addEventListener("focusin", (e) => {
    const t = e.target as HTMLElement | null;
    mark(t && t.hasAttribute?.("data-nav") ? t : null);
  });
  queueEnsure();
}
