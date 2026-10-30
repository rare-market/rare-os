// A clip's poster: one frame grabbed in the webview. The clip is loaded from its bytes through a
// Blob URL (same origin, so the canvas is not tainted), seeked about 1 s in, drawn at no more than
// 1280 px on the long edge and saved as JPEG.

import { fitWithin, posterTime } from "./format";

export async function grabPoster(bytes: Uint8Array, mime = "video/mp4", maxEdge = 1280): Promise<Blob> {
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: mime }));
  const v = document.createElement("video");
  v.muted = true;
  v.playsInline = true;
  v.preload = "auto";
  try {
    await new Promise<void>((res, rej) => {
      v.onloadedmetadata = () => res();
      v.onerror = () => rej(new Error("This clip can't be read"));
      v.src = url;
    });
    // Recorders that stream (MediaRecorder WebM) report an infinite duration until seeked far.
    let duration = v.duration;
    if (!Number.isFinite(duration)) {
      v.currentTime = 1e7;
      await new Promise((r) => (v.onseeked = r));
      duration = v.duration;
    }
    v.currentTime = posterTime(duration);
    await new Promise<void>((res, rej) => {
      const t = setTimeout(() => rej(new Error("Timed out reading the clip")), 10_000);
      v.onseeked = () => {
        clearTimeout(t);
        res();
      };
    });
    // A decoded frame is there once seeked; some engines need a tick more.
    if (v.readyState < 2) await new Promise((r) => (v.oncanplay = r));
    const { w, h } = fitWithin(v.videoWidth, v.videoHeight, maxEdge);
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    c.getContext("2d")!.drawImage(v, 0, 0, w, h);
    return await new Promise<Blob>((res, rej) => c.toBlob((b) => (b ? res(b) : rej(new Error("Couldn't make a poster"))), "image/jpeg", 0.8));
  } finally {
    v.removeAttribute("src");
    v.load();
    URL.revokeObjectURL(url);
  }
}
