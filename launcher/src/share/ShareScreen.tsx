// Share: screenshots and clips on this device, and a link for any of them. A share is one Walrus
// quilt (share.json, the media, a poster for clips) that rare.market/s/<id> shows. The player's
// wallet pays, after a price from live Walrus prices and two approvals in Rare Wallet.

import { useEffect, useRef, useState } from "react";
import { useApp } from "../lib/store";
import { focusEl, focusFirst } from "../nav/focus";
import { feedback } from "../nav/feedback";
import { useHints } from "../nav/hints";
import { usePadHandler } from "../nav/input";
import { Button, Dot, Empty, LoadBar, Panel, SectionHead, Toggle } from "../ui";
import { Qr } from "../marketplace/BuySheet";
import { useProfile } from "../sui/profile";
import { ago, fmtBytes, fmtSuiMist, fmtWal } from "../sui/ui/CloudSaves";
import { DURATIONS, fmtDuration } from "./format";
import type { Capture, ShareStatus } from "./native";
import { telegramPoster } from "./post";
import { copyText, useShare, type ShareStep } from "./store";
import { failed } from "../lib/fail";

const STEPS: Array<{ step: ShareStep; label: string; doing: string }> = [
  { step: "encode", label: "Prepare", doing: "Preparing the files" },
  { step: "register", label: "Pay for storage", doing: "Approve in Rare Wallet" },
  { step: "upload", label: "Upload", doing: "Uploading to Walrus" },
  { step: "certify", label: "Confirm", doing: "Approve to confirm" },
];

const kindLabel = (c: Capture) => (c.kind === "clip" ? "Clip" : "Screenshot");

/** Why capturing isn't possible here, in one line, or null. */
export function clipsNote(st: ShareStatus | null): string | null {
  if (!st) return null;
  if (!st.clips.available) return "Clips need gpu-screen-recorder or GStreamer with VA-API";
  if (!st.clips.enabled) return "Clips are off. Turn on Keep the last 30 seconds in Share.";
  return null;
}

export function ShareScreen() {
  const s = useShare();
  const root = useRef<HTMLDivElement>(null);
  // Markers show for what was new when the screen opened, and stay until it closes.
  const [seenAt] = useState(() => useShare.getState().seenAt);
  const lastTile = useRef<string | null>(null);

  useEffect(() => {
    void useProfile.getState().refresh();
    void useShare.getState().refresh({ thumbs: true });
    return () => {
      useShare.getState().markSeen();
      useShare.getState().select(null);
    };
  }, []);

  // Lands focus on the screen's default (Share once the price is in, the last tile when coming
  // back), unless the player has already moved it.
  const view = s.selected ? `detail-${s.phase.at}-${!!s.quote}` : `grid-${s.captures === null}`;
  const auto = useRef<Element | null>(null);
  useEffect(() => {
    const t = setTimeout(() => {
      const el = root.current;
      if (!el) return;
      const a = document.activeElement;
      const moved = a && a !== document.body && el.contains(a) && a !== auto.current;
      if (moved && view.startsWith("detail-idle-true")) return;
      const back = !s.selected && lastTile.current ? el.querySelector<HTMLElement>(`[data-capture="${CSS.escape(lastTile.current)}"]`) : null;
      if (back) focusEl(back, { smooth: false });
      else focusFirst(el);
      auto.current = document.activeElement;
    }, 40);
    return () => clearTimeout(t);
  }, [view]); // eslint-disable-line react-hooks/exhaustive-deps

  usePadHandler((b) => {
    const st = useShare.getState();
    if (b === "b" && st.selected) {
      if (st.phase.at === "uploading") return true;
      st.select(null);
      feedback("back");
      return true;
    }
    if (b === "x" && st.selected && (st.phase.at === "idle" || st.phase.at === "error")) {
      const i = DURATIONS.findIndex((d) => d.id === st.duration);
      st.setDuration(DURATIONS[(i + 1) % DURATIONS.length].id);
      feedback("toggle");
      return true;
    }
    return false;
  });

  const selected = s.captures?.find((c) => c.id === s.selected) ?? null;

  return (
    <div ref={root} data-nav-scope className="rise fixed inset-0 z-30 bg-bg">
      <LoadBar on={s.captures === null || !!s.taking || (!!selected && !s.quote && s.phase.at === "idle" && !s.error)} />
      <div data-scroll className="h-full overflow-y-auto px-[2.5rem] pb-[3rem] pt-[2rem]">
        {selected ? (
          <Detail c={selected} />
        ) : (
          <Grid
            seenAt={seenAt}
            onOpen={(c) => {
              lastTile.current = c.id;
              s.select(c.id);
            }}
          />
        )}
      </div>
    </div>
  );
}

function Grid({ seenAt, onOpen }: { seenAt: number; onOpen: (c: Capture) => void }) {
  const s = useShare();
  const st = s.status;
  const note = clipsNote(st);
  return (
    <>
      <div className="flex items-end gap-[1rem]">
        <div className="min-w-0 flex-1">
          <h1 className="text-title font-semibold tracking-[-0.02em]">Share</h1>
          <p className="mt-[0.25rem] max-w-[46rem] text-small text-muted">
            Screenshots and clips from this device. Share one as a link anyone can open on rare.market. It's stored on Walrus and your wallet pays for the storage.
          </p>
        </div>
      </div>

      <Panel className="mt-[1.5rem]">
        <div data-nav-group="share-actions" data-nav-remember className="flex min-h-[4.25rem] items-center gap-[1.25rem] px-[1.4rem] py-[0.85rem]">
          <div className="min-w-0 flex-1">
            <div className="text-body font-semibold">Capture</div>
            <div className="text-small text-muted">In a game: Guide and Y for a screenshot, Guide and View for the last 30 seconds.</div>
          </div>
          <Button def hint="Take a screenshot" disabled={!!s.taking || st?.screenshot.available === false} onClick={() => void s.takeScreenshot()}>
            {s.taking === "screenshot" ? "Saving" : "Take screenshot"}
          </Button>
          <Button hint="Save the last 30 seconds" disabled={!!s.taking || !st?.clips.recording} onClick={() => void s.saveClip()}>
            {s.taking === "clip" ? "Saving" : "Save last 30 seconds"}
          </Button>
        </div>
        <button
          data-nav
          data-hint-a={st?.clips.enabled ? "Turn off" : "Turn on"}
          disabled={!st?.clips.available}
          onClick={() => {
            feedback("toggle");
            void s.setClips(!st?.clips.enabled);
          }}
          className="flex min-h-[4.25rem] w-full items-center gap-[1.25rem] border-t border-hair px-[1.4rem] py-[0.85rem] text-left disabled:opacity-60"
        >
          <span className="min-w-0 flex-1">
            <span className="block text-body font-semibold">Keep the last 30 seconds while playing</span>
            <span className="block text-small text-muted">
              {!st
                ? ""
                : !st.clips.available
                  ? note
                  : st.clips.enabled
                    ? st.clips.recording
                      ? `Recording in the background${st.clips.method ? ` with ${st.clips.method}` : ""}. Uses a little battery.`
                      : "On. Recording starts with the next game."
                    : "Off. Turn it on to save clips."}
            </span>
          </span>
          {st?.clips.recording && (
            <span className="flex items-center gap-[0.45rem] text-small text-muted">
              <Dot tone="brand" live />
              Recording
            </span>
          )}
          <Toggle on={!!st?.clips.enabled} />
        </button>
        {st && !st.screenshot.available && (
          <div className="border-t border-hair px-[1.4rem] py-[0.85rem] text-small text-muted">Screenshots aren't available in this session.</div>
        )}
      </Panel>

      {s.error && (
        <div className="mt-[0.9rem] flex items-center gap-[0.5rem] text-small text-fg">
          <Dot tone="negative" />
          {s.error}
        </div>
      )}

      <SectionHead className="mb-[0.9rem] mt-[2rem]" title="Recent" note={s.captures ? `${s.captures.length} on this device` : undefined} />
      {s.captures && s.captures.length === 0 && <Empty title="No screenshots or clips yet" body="Press Guide and Y in a game, or take one here." />}
      {s.captures && s.captures.length > 0 && (
        <div data-nav-group="share-grid" className="grid grid-cols-4 gap-[1rem]">
          {s.captures.map((c) => (
            <Tile key={c.id} c={c} thumb={s.thumbs[c.id]} fresh={c.createdMs > seenAt} onOpen={() => onOpen(c)} />
          ))}
        </div>
      )}
    </>
  );
}

function Tile({ c, thumb, fresh, onOpen }: { c: Capture; thumb: string | undefined; fresh: boolean; onOpen: () => void }) {
  return (
    <button data-nav data-capture={c.id} data-hint-a="Open" onClick={onOpen} className="tile relative aspect-video w-full overflow-hidden rounded-card border border-hair bg-l1 text-left">
      {thumb ? (
        <img src={thumb} alt="" draggable={false} decoding="async" className="fade-in absolute inset-0 h-full w-full object-cover" />
      ) : thumb === "" ? (
        <span className="absolute inset-0 flex items-center justify-center text-small text-muted">Can't read this file</span>
      ) : null}
      <span className="absolute inset-x-0 bottom-0 h-3/5 bg-gradient-to-t from-black/90 via-black/45 to-transparent" />
      {fresh && <span className="absolute inset-x-0 top-0 h-2/5 bg-gradient-to-b from-black/70 to-transparent" />}
      {fresh && (
        <span className="absolute right-[0.85rem] top-[0.65rem] flex items-center gap-[0.4rem] text-label font-medium text-white/85">
          <Dot tone="brand" />
          New
        </span>
      )}
      <span className="absolute inset-x-[0.85rem] bottom-[0.7rem] flex items-end gap-[0.5rem]">
        <span className="min-w-0 flex-1">
          <span className="block truncate text-body font-bold text-white">{c.game?.name ?? kindLabel(c)}</span>
          <span className="block truncate text-meta text-white/60">
            {kindLabel(c)} · {ago(c.createdMs)}
          </span>
        </span>
        {c.kind === "clip" && <span className="shrink-0 text-meta tabular-nums text-white/80">{fmtDuration(c.durationMs)}</span>}
      </span>
    </button>
  );
}

function Detail({ c }: { c: Capture }) {
  const s = useShare();
  const address = useProfile((p) => p.address);
  const [confirm, setConfirm] = useState(false);
  const toast = useApp((x) => x.toast);
  const ph = s.phase;
  const other = DURATIONS.find((d) => d.id !== s.duration)!;
  useHints(ph.at === "idle" || ph.at === "error" ? [{ b: "x", label: `Keep for ${other.label}` }] : []);

  const media = c.kind === "clip" ? s.preview : s.thumbs[c.id];
  const q = s.quote;
  const rare = s.payer === "rare" && !!q?.sponsor?.covered;
  const del = () => {
    if (!confirm) return setConfirm(true);
    setConfirm(false);
    void s.remove(c.id).then(() => toast(`${kindLabel(c)} deleted`));
  };

  return (
    <div className="grid grid-cols-[minmax(0,1fr)_23rem] items-start gap-[1.5rem]">
      <div className="relative aspect-video overflow-hidden rounded-panel bg-l1">
        {media && c.kind === "clip" ? (
          <video key={media} src={media} poster={s.thumbs[c.id] || undefined} muted loop autoPlay playsInline className="absolute inset-0 h-full w-full object-contain" />
        ) : media ? (
          <img src={media} alt="" draggable={false} className="fade-in absolute inset-0 h-full w-full object-contain" />
        ) : s.thumbs[c.id] ? (
          <img src={s.thumbs[c.id]} alt="" draggable={false} className="absolute inset-0 h-full w-full object-contain" />
        ) : null}
      </div>

      <Panel className="flex flex-col gap-[1rem] px-[1.25rem] py-[1.2rem]">
        <div className="min-w-0">
          <div className="truncate text-row font-semibold">{c.game?.name ?? kindLabel(c)}</div>
          <div className="text-small text-muted">
            {[kindLabel(c), c.kind === "clip" ? fmtDuration(c.durationMs) : null, `${c.width} x ${c.height}`, fmtBytes(c.bytes), ago(c.createdMs)].filter(Boolean).join(" · ")}
          </div>
        </div>

        {(ph.at === "idle" || ph.at === "error") && (
          <>
            <div className="flex flex-col gap-[0.5rem]">
              <div className="text-label font-medium text-secondary">Keep it for</div>
              <div className="grid grid-cols-2 gap-[0.5rem]" data-nav-group="share-duration">
                {DURATIONS.map((d) => (
                  <button
                    key={d.id}
                    data-nav
                    data-hint-a={`Keep for ${d.label}`}
                    onClick={() => s.setDuration(d.id)}
                    className={`btn h-[2.25rem] rounded-chip px-[0.8rem] text-small font-medium ${s.duration === d.id ? "bg-fg text-bg" : "bg-l2 text-fg"}`}
                  >
                    {d.label}
                  </button>
                ))}
              </div>
            </div>

            {q?.sponsor?.covered && (
              <div className="flex flex-col gap-[0.5rem]">
                <div className="text-label font-medium text-secondary">Who pays</div>
                <div className="grid grid-cols-2 gap-[0.5rem]" data-nav-group="share-payer">
                  {(["rare", "me"] as const).map((p) => (
                    <button
                      key={p}
                      data-nav
                      onClick={() => s.setPayer(p)}
                      className={`btn h-[2.25rem] rounded-chip px-[0.8rem] text-small font-medium ${s.payer === p ? "bg-fg text-bg" : "bg-l2 text-fg"}`}
                    >
                      {p === "rare" ? "Rare covers it" : "My wallet"}
                    </button>
                  ))}
                </div>
              </div>
            )}

            <div className="border-t border-hair pt-[0.9rem]">
              {rare ? (
                <>
                  <div className="text-body font-semibold">Free</div>
                  <div className="text-small text-muted">{q?.sponsor?.note ?? `Rare pays for ${q!.days} days on Walrus.`} Anyone with the link can see it.</div>
                </>
              ) : q?.cost ? (
                <>
                  <div className="text-body">
                    <span className="font-semibold tabular-nums">About {fmtWal(q.cost.walFrost)}</span>
                    <span className="text-muted"> plus {fmtSuiMist(q.cost.tipMist + q.cost.gasMist)} in fees</span>
                  </div>
                  <div className="text-small text-muted">
                    Stored on Walrus{s.network === "testnet" ? " testnet" : ""} for {q.days} days, {fmtBytes(q.size)}. Anyone with the link can see it.
                  </div>
                </>
              ) : q ? (
                <div className="text-small text-muted">Couldn't get the Walrus price. Try again in a moment.</div>
              ) : (
                <div className="text-small text-muted">Getting the price</div>
              )}
              {!address && !rare && <div className="mt-[0.4rem] text-small text-muted">Add a Rare Wallet account to share.</div>}
            </div>

            {ph.at === "error" && (
              <div className="flex items-start gap-[0.5rem] text-small text-fg">
                <Dot tone="negative" className="mt-[0.4rem]" />
                {ph.message}
              </div>
            )}
            {s.error && (
              <div className="flex items-start gap-[0.5rem] text-small text-fg">
                <Dot tone="negative" className="mt-[0.4rem]" />
                {s.error}
              </div>
            )}

            <div className="flex gap-[0.5rem]" data-nav-group="share-go" data-nav-remember>
              <Button variant="primary" def hint="Share" className="flex-1" disabled={!q || (!rare && (!address || !q.cost))} onClick={() => void s.share(address)}>
                {ph.at === "error" ? "Try again" : "Share"}
              </Button>
              <Button hint={confirm ? "Delete" : "Delete from this device"} onClick={del}>
                {confirm ? "Press again" : "Delete"}
              </Button>
            </div>
          </>
        )}

        {ph.at === "uploading" && (
          <div className="flex flex-col gap-[0.25rem] border-t border-hair pt-[0.9rem]">
            {STEPS.map((x, i) => {
              const cur = STEPS.findIndex((y) => y.step === ph.step);
              const state = i < cur ? "done" : i === cur ? "now" : "next";
              return (
                <div key={x.step} className="flex h-[2.25rem] items-center gap-[0.65rem] text-small">
                  <Dot tone={state === "now" ? "brand" : state === "done" ? "primary" : "muted"} live={state === "now"} />
                  <span className={state === "next" ? "text-muted" : "text-fg"}>{state === "now" ? x.doing : x.label}</span>
                </div>
              );
            })}
            <div className="mt-[0.4rem] text-small text-muted">{rare ? "Rare is storing it." : "Two approvals in Rare Wallet: one pays for the storage, one confirms it."}</div>
          </div>
        )}

        {ph.at === "done" && (
          <>
            <div className="flex flex-col items-center gap-[0.75rem] border-t border-hair pt-[1rem]">
              <Qr text={ph.result.link} className="size-[11rem]" />
              <div className="num w-full break-all text-center text-meta text-fg">{ph.result.link.replace(/^https:\/\//, "")}</div>
              <div className="flex items-center gap-[0.45rem] text-small text-muted">
                <Dot tone={ph.copied ? "brand" : "muted"} />
                {ph.copied ? "Copied" : "Scan the code or copy the link"}
              </div>
            </div>
            <div className="grid grid-cols-2 gap-[0.5rem]" data-nav-group="share-done">
              <Button variant="primary" def hint="Copy link" className="col-span-2" onClick={() => void copyText(ph.result.link).then((ok) => s.copied(ok))}>
                Copy link
              </Button>
              {telegramPoster && (
                <Button hint="Post to Telegram" className="col-span-2" onClick={() => void telegramPoster?.(ph.result.link).catch((e) => toast(failed("Couldn't open Telegram.", e), "error"))}>
                  Post to Telegram
                </Button>
              )}
              <Button hint="Back to your captures" onClick={() => s.select(null)}>
                Done
              </Button>
              <Button hint={confirm ? "Delete" : "Delete from this device"} onClick={del}>
                {confirm ? "Press again" : "Delete"}
              </Button>
            </div>
          </>
        )}
      </Panel>
    </div>
  );
}
