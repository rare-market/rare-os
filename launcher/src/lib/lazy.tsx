import { Suspense, lazy, useRef, type ComponentType, type ReactNode } from "react";

export type LazyScreen = ComponentType<any> & { preload: () => Promise<void> };

/**
 * React.lazy for a named export: `lazyNamed(() => import("./x"), "Screen")`, with `preload()`.
 *
 * React.lazy suspends on its first render even when the chunk is already in the module cache, and
 * React then holds a Suspense fallback for at least 300 ms before it reveals the content
 * (FALLBACK_THROTTLE_MS): every first visit to a screen was an empty frame for ~300 ms. Once
 * preloaded, this renders the component directly and never suspends.
 */
export function lazyNamed<M, K extends keyof M>(load: () => Promise<M>, name: K): LazyScreen {
  let loaded: ComponentType<any> | null = null;
  let pending: Promise<void> | null = null;
  const preload = () =>
    (pending ??= load().then(
      (m) => {
        loaded = m[name] as unknown as ComponentType<any>;
      },
      (e) => {
        pending = null; // a failed fetch can be tried again
        throw e;
      },
    ));
  const Suspending = lazy(async () => {
    await preload();
    return { default: loaded! };
  });
  function Screen(props: Record<string, unknown>) {
    // An instance that first rendered through React.lazy stays on it, so it never remounts when the
    // chunk lands.
    const viaLazy = useRef(!loaded).current;
    const C = viaLazy ? Suspending : loaded!;
    return <C {...props} />;
  }
  return Object.assign(Screen, { preload });
}

/** Loads the screens' chunks one at a time while the launcher is idle, so opening one never waits on a chunk. */
export function preloadWhenIdle(screens: LazyScreen[]) {
  const idle = (window as { requestIdleCallback?: (f: () => void, o?: { timeout: number }) => number }).requestIdleCallback;
  const queue = [...screens];
  const next = () => {
    const s = queue.shift();
    if (!s) return;
    void s.preload().catch(() => {}).finally(() => (idle ? idle(next, { timeout: 2000 }) : setTimeout(next, 50)));
  };
  next();
}

/**
 * A Suspense boundary for lazy screens. It renders nothing only the very first time it mounts; place it
 * above anything keyed by the screen (App's tab view) so a screen change suspends inside an
 * already-shown boundary, and with a deferred tab React keeps the previous screen up instead.
 */
export function Lazy({ children }: { children: ReactNode }) {
  return <Suspense fallback={null}>{children}</Suspense>;
}
