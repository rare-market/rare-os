import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DECODE_WAIT_MS, artDecoding, isReady, readyRef, setRenderMode, whenDecoded } from "./img";

// A loaded <img> as far as whenDecoded and readyRef look at it.
function fakeImg(src: string, decode: () => Promise<void>, complete = true) {
  return { src, complete, naturalWidth: complete ? 300 : 0, getAttribute: (k: string) => (k === "src" ? src : null), decode } as unknown as HTMLImageElement;
}

describe("art decoding", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    setRenderMode("cpu");
  });

  it("shows an image whose decode() never settles (WebKitGTK), after DECODE_WAIT_MS", () => {
    const done = vi.fn();
    whenDecoded(fakeImg("asset://localhost/never.jpg", () => new Promise<void>(() => {})), done);
    expect(done).not.toHaveBeenCalled();
    vi.advanceTimersByTime(DECODE_WAIT_MS);
    expect(done).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(10_000);
    expect(done).toHaveBeenCalledTimes(1);
    expect(isReady("asset://localhost/never.jpg")).toBe(true);
  });

  it("shows a decoded image at once, and only once", async () => {
    const done = vi.fn();
    whenDecoded(fakeImg("a.jpg", () => Promise.resolve()), done);
    await vi.advanceTimersByTimeAsync(0);
    expect(done).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(DECODE_WAIT_MS * 2);
    expect(done).toHaveBeenCalledTimes(1);
  });

  it("shows an image whose decode() rejects or throws", async () => {
    const rejected = vi.fn();
    whenDecoded(fakeImg("b.jpg", () => Promise.reject(new Error("EncodingError"))), rejected);
    await vi.advanceTimersByTimeAsync(0);
    expect(rejected).toHaveBeenCalledTimes(1);
    const thrown = vi.fn();
    whenDecoded(
      fakeImg("c.jpg", () => {
        throw new Error("no");
      }),
      thrown,
    );
    expect(thrown).toHaveBeenCalledTimes(1);
  });

  it("catches a cached image through the ref even when onLoad never fires", () => {
    const done = vi.fn();
    readyRef(done)(fakeImg("cached.jpg", () => new Promise<void>(() => {})));
    vi.advanceTimersByTime(DECODE_WAIT_MS);
    expect(done).toHaveBeenCalledTimes(1);
    const notYet = vi.fn();
    readyRef(notYet)(fakeImg("loading.jpg", () => Promise.resolve(), false));
    vi.advanceTimersByTime(DECODE_WAIT_MS);
    expect(notYet).not.toHaveBeenCalled();
  });

  it("decodes async on the CPU renderer and sync only on the GPU ones", () => {
    setRenderMode("cpu");
    expect(artDecoding()).toBe("async");
    setRenderMode("desktop");
    expect(artDecoding()).toBe("async");
    setRenderMode(undefined);
    expect(artDecoding()).toBe("async");
    for (const m of ["gpu", "gpuatlas", "wayland", "dmabuf"]) {
      setRenderMode(m);
      expect(artDecoding()).toBe("sync");
    }
  });
});
