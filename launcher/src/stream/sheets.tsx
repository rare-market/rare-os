// PC streaming sheets, on the launcher's standard sheet (trade/parts Sheet: rare.market's modal),
// like an app's page (apps/sheets):
//   StreamSetup    get Moonlight, find PCs (mDNS) or add one by address, pair with a big PIN
//   StreamAppSheet a PC game: Play, Resume and Quit game, wake the PC, the stream quality

import { useEffect, useRef } from "react";
import { Monitor, Plus } from "lucide-react";
import { Art } from "../components/Art";
import { askText } from "../components/Osk";
import { useApp } from "../lib/store";
import { feedback } from "../nav/feedback";
import { focusFirst } from "../nav/focus";
import { exitHint, useHints } from "../nav/hints";
import { rareWay } from "../lib/guest";
import { Sheet } from "../trade/parts";
import { Button, Dot, LoadBar, Segmented } from "../ui";
import { InstallBar } from "../apps/tiles";
import { requestLibraryFilter } from "../screens/Library";
import { BITRATES, CODECS, CODEC_LABEL, RESOLUTIONS, autoCodec, cycle, playState, validAddress } from "./logic";
import { useStream } from "./state";
import type { Host, StreamApp } from "./types";

function useAutoFocus(dep?: unknown) {
  const ref = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const t = setTimeout(() => ref.current && focusFirst(ref.current), 20);
    return () => clearTimeout(t);
  }, [dep]);
  return (el: HTMLDivElement | null) => {
    ref.current = el;
  };
}

/** Moonlight from Flathub: why, progress, and the command for Desktop mode. */
function GetMoonlight() {
  const offer = useStream((s) => s.offer);
  const installing = useStream((s) => s.installing);
  if (!offer) return null;
  const busy = !!installing && !installing.error;
  return (
    <div className="mb-[1.4rem] flex flex-col gap-[0.6rem]">
      <div className="text-body font-semibold">Moonlight</div>
      <p className="text-small text-secondary">Rare streams with Moonlight, a free open-source app. It installs from Flathub for you only, no password needed.</p>
      {busy && (
        <div className="flex items-center gap-[0.9rem]">
          <InstallBar progress={installing!.progress} className="flex-1" />
          <span className="w-[3rem] text-right text-small tabular-nums text-fg">{installing!.progress == null ? "" : `${Math.round(installing!.progress * 100)}%`}</span>
        </div>
      )}
      {installing?.error && <p className="text-small text-negative">{installing.error}</p>}
      <div className="text-label text-secondary">Or in Desktop mode, in a terminal:</div>
      <code className="num block select-all break-all rounded-card bg-l2 px-[0.9rem] py-[0.6rem] text-meta text-fg">{offer.command}</code>
    </div>
  );
}

function HostRow({ host, def }: { host: Host; def?: boolean }) {
  const { pair, listApps } = useStream.getState();
  const ready = useStream((s) => !!s.moonlight);
  const sub = [host.address, host.software].filter(Boolean).join(" · ");
  const state = host.paired ? "Paired" : host.online ? "Not paired" : "Not answering";
  return (
    <button
      data-nav
      data-nav-default={def ? "" : undefined}
      data-hint-a={host.paired ? "Show games" : "Pair"}
      onClick={() => {
        if (!ready) return useApp.getState().toast("Get Moonlight first", "error");
        if (!host.paired) return void pair(host);
        void listApps(host.uuid);
        requestLibraryFilter("stream");
        useApp.getState().close();
        useApp.getState().setTab("library");
      }}
      className="flex h-[3.75rem] w-full items-center gap-[0.9rem] rounded-card px-[0.75rem] text-left"
    >
      <span className="flex h-[2.25rem] w-[2.25rem] shrink-0 items-center justify-center rounded-full bg-l3 text-icon-2">
        <Monitor size="1.1rem" />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-body font-medium">{host.name}</span>
        <span className="num block truncate text-meta text-muted">{sub}</span>
      </span>
      <span className="flex shrink-0 items-center gap-[0.45rem] text-small text-secondary">
        <Dot tone={host.online ? "primary" : "muted"} />
        {state}
      </span>
    </button>
  );
}

/** Find PCs and pair. */
export function StreamSetup() {
  const s = useStream();
  const onRef = useAutoFocus(s.pairing?.pin ?? s.moonlight ?? "find");
  useEffect(() => {
    if (!s.loaded) void s.load();
    // Finding PCs is Rare's own mDNS; it doesn't need Moonlight.
    void s.scan();
    return () => {
      // Leaving the sheet stops a pairing that hasn't finished.
      if (useStream.getState().pairing?.phase === "waiting") void useStream.getState().cancelPair();
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  if (s.pairing) return <PairSheet onRef={onRef} />;

  const addByAddress = async () => {
    const a = await askText("Your PC's address", "", { placeholder: "192.168.0.20" });
    if (!a) return;
    if (!validAddress(a)) return useApp.getState().toast("That doesn't look like an address", "error");
    const h = await s.add(a.trim());
    if (h && !h.paired && s.moonlight) void s.pair(h);
  };
  const unpaired = s.hosts.findIndex((h) => !h.paired && h.online);
  const busy = !!s.installing && !s.installing.error;
  return (
    <Sheet
      onRef={onRef}
      wide
      title="Stream from your PC"
      sub="Play the games on your gaming PC on this SuiPlay, over your home network."
      footer={
        <>
          {s.offer && (
            <Button variant="primary" def disabled={busy} hint="Get Moonlight" onClick={() => void s.install()}>
              {busy ? "Installing" : "Get Moonlight"}
            </Button>
          )}
          <Button hint="Add by address" icon={<Plus size="1rem" />} onClick={() => void addByAddress()}>
            Add by address
          </Button>
          <Button hint="Look again" disabled={s.scanning} onClick={() => void s.scan()}>
            Look again
          </Button>
        </>
      }
    >
      <GetMoonlight />
      <div className="relative">
        <div className="mb-[0.5rem] flex items-center gap-[0.75rem]">
          <span className="text-body font-semibold">PCs on your network</span>
          <span className="text-small text-muted">{s.scanning ? "Looking" : ""}</span>
        </div>
        <LoadBar on={s.scanning} className="top-[1.9rem]" />
        <div data-nav-group="stream-hosts" className="flex flex-col">
          {s.hosts.map((h, i) => (
            <HostRow key={h.uuid} host={h} def={!s.offer && (unpaired >= 0 ? i === unpaired : i === 0)} />
          ))}
        </div>
        {!s.scanning && s.hosts.length === 0 && <p className="py-[0.9rem] text-small text-secondary">No PCs found yet.</p>}
      </div>
      <p className="mt-[1.1rem] text-small text-secondary">
        Your PC needs Sunshine (free, for any graphics card) or GeForce Experience, on the same network. Install Sunshine from <span className="num text-fg">github.com/LizardByte/Sunshine</span>, start it, then look again.
      </p>
    </Sheet>
  );
}

/** The PIN, big, and where to type it on the PC. */
function PairSheet({ onRef }: { onRef: (el: HTMLDivElement | null) => void }) {
  const p = useStream((s) => s.pairing)!;
  const { cancelPair, pair } = useStream.getState();
  const host = useStream((s) => s.hosts.find((h) => h.uuid === p.uuid));
  const done = () => {
    void cancelPair();
    requestLibraryFilter("stream");
    useApp.getState().close();
    useApp.getState().setTab("library");
  };
  const gfe = p.software === "GeForce Experience";
  const steps = gfe
    ? [<>GeForce Experience shows a box on {p.name}.</>, <>Type this PIN in it.</>, <>Rare finishes on its own.</>]
    : [
        <>
          On {p.name}, open Sunshine at <span className="num text-fg">https://localhost:47990</span>
        </>,
        <>Go to PIN. Enter this PIN and a name for this SuiPlay.</>,
        <>Select Send. Rare finishes on its own.</>,
      ];
  return (
    <Sheet
      onRef={onRef}
      wide
      title={`Pair with ${p.name}`}
      sub="Pairing happens once. After that your PC's games show up in your Library."
      footer={
        p.phase === "paired" ? (
          <Button variant="primary" def hint="Show games" onClick={done}>
            Show games
          </Button>
        ) : p.phase === "failed" ? (
          <>
            <Button variant="primary" def hint="Try again" onClick={() => host && void pair(host)}>
              Try again
            </Button>
            <Button onClick={() => void cancelPair()}>Back</Button>
          </>
        ) : (
          <Button def hint="Cancel" onClick={() => void cancelPair()}>
            Cancel
          </Button>
        )
      }
    >
      <div className="flex items-center gap-[2.5rem]">
        <div className="flex shrink-0 gap-[0.6rem]" aria-label={`PIN ${p.pin}`}>
          {p.pin.split("").map((d, i) => (
            <span key={i} className="flex h-[5.5rem] w-[4.25rem] items-center justify-center rounded-card bg-l2 text-hero font-semibold tabular-nums">
              {d}
            </span>
          ))}
        </div>
        <ol className="flex min-w-0 flex-1 flex-col gap-[0.7rem]">
          {steps.map((s, i) => (
            <li key={i} className="flex gap-[0.75rem] text-body">
              <span className="w-[1rem] shrink-0 tabular-nums text-muted">{i + 1}</span>
              <span className="min-w-0">{s}</span>
            </li>
          ))}
        </ol>
      </div>
      <div className="relative mt-[1.6rem] border-t border-hair pt-[1rem]">
        <LoadBar on={p.phase === "waiting"} className="-top-px" />
        <div className="flex items-center gap-[0.5rem] text-small">
          <Dot tone={p.phase === "paired" ? "primary" : p.phase === "failed" ? "negative" : "muted"} live={p.phase === "waiting"} />
          <span className={p.phase === "failed" ? "text-negative" : "text-fg/80"}>{p.phase === "paired" ? `Paired with ${p.name}` : p.phase === "failed" ? p.error : `Waiting for ${p.name}`}</span>
        </div>
      </div>
    </Sheet>
  );
}

/** A focusable setting row: A moves to the next value. */
function Choice<T extends string>({ label, items, value, onPick, def }: { label: string; items: Array<{ value: T; label: string }>; value: T; onPick: (v: T) => void; def?: boolean }) {
  return (
    <button
      data-nav
      data-nav-default={def ? "" : undefined}
      data-hint-a="Change"
      onClick={() => {
        feedback("toggle");
        onPick(cycle(items.map((i) => i.value), value));
      }}
      className="flex h-[3.25rem] w-full items-center gap-[1rem] rounded-card px-[0.75rem] text-left"
    >
      <span className="min-w-0 flex-1 text-body">{label}</span>
      <Segmented items={items} value={value} />
    </button>
  );
}

/** A PC game's page. */
export function StreamAppSheet({ uuid, appId }: { uuid: string; appId: number }) {
  const s = useStream();
  const running = useApp((x) => x.running);
  const onRef = useAutoFocus();
  useHints([exitHint()]);
  useEffect(() => {
    if (!s.loaded) void s.load();
    else void s.refresh();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const host = s.hosts.find((h) => h.uuid === uuid);
  const app: StreamApp | undefined = s.apps[uuid]?.find((a) => a.id === appId);
  if (!host || !app) return null;
  const state = playState(host, app, running);
  const other = host.currentGame != null ? s.apps[uuid]?.find((a) => a.id === host.currentGame)?.name ?? "Another game" : null;
  const waking = s.waking === uuid;
  const close = () => useApp.getState().close();
  const set = s.settings;
  const auto = autoCodec(s.decode, host.codecs);
  const codecItems = CODECS.map((c) => (c.value === "auto" && auto ? { ...c, label: `Auto (${CODEC_LABEL[auto]})` } : c));

  const line =
    state === "playing"
      ? "Streaming to this SuiPlay"
      : state === "on-pc"
        ? `Running on ${host.name}. Resume picks it up where you left it.`
        : state === "busy"
          ? `${other} is running on ${host.name}. Play quits it first.`
          : state === "asleep"
            ? waking
              ? `Waking ${host.name}`
              : `${host.name} is asleep. Play wakes it first.`
            : state === "offline"
              ? `${host.name} isn't answering. Turn it on, then look again.`
              : `Ready on ${host.name}`;

  const footer = !s.moonlight ? (
    <Button variant="primary" def hint="Set up streaming" onClick={() => useApp.getState().open({ kind: "stream" })}>
      Set up streaming
    </Button>
  ) : state === "playing" || state === "on-pc" ? (
    <>
      <Button variant="primary" def hint="Resume" onClick={() => (close(), void s.play(host, app))}>
        Resume
      </Button>
      <Button hint="Quit game" onClick={() => (close(), void s.quit(host))}>
        Quit game
      </Button>
    </>
  ) : state === "busy" ? (
    <Button variant="primary" def hint={`Quit ${other} and play`} onClick={() => (close(), void s.play(host, app, true))}>
      Quit {other} and play
    </Button>
  ) : state === "asleep" ? (
    <Button variant="primary" def disabled={waking} hint="Wake and play" onClick={() => void s.play(host, app).then(() => useApp.getState().overlay?.kind === "streamapp" && close())}>
      {waking ? `Waking ${host.name}` : "Wake and play"}
    </Button>
  ) : state === "offline" ? (
    <Button def hint="Look again" onClick={() => void s.refresh()}>
      Look again
    </Button>
  ) : (
    <Button variant="primary" def hint="Play" onClick={() => (close(), void s.play(host, app))}>
      Play
    </Button>
  );

  return (
    <Sheet onRef={onRef} wide title={app.name} sub={`On ${host.name}${host.software ? ` · ${host.software}` : ""}`} footer={footer}>
      <div className="flex gap-[1.75rem]">
        <div className="relative aspect-[2/3] w-[9rem] shrink-0 overflow-hidden rounded-card bg-l3">
          <Art sources={app.art ? [app.art] : []} title={app.name} variant="cover" className="absolute inset-0 h-full w-full object-cover" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="relative flex items-center gap-[0.5rem] text-small text-fg/80">
            <Dot tone={state === "playing" || state === "on-pc" ? "primary" : "muted"} live={state === "playing" || waking} />
            {line}
          </div>
          <div className="mb-[0.35rem] mt-[1.2rem] text-body font-semibold">Stream quality</div>
          <div data-nav-group="stream-quality" className="flex flex-col">
            <Choice label="Resolution" items={RESOLUTIONS} value={set.resolution} onPick={(v) => void s.setSettings({ ...set, resolution: v })} />
            <Choice label="Bitrate" items={BITRATES} value={String(set.bitrateMbps)} onPick={(v) => void s.setSettings({ ...set, bitrateMbps: Number(v) })} />
            <Choice label="Video" items={codecItems} value={set.codec} onPick={(v) => void s.setSettings({ ...set, codec: v })} />
          </div>
          <p className="mt-[0.8rem] text-small text-secondary">
            For every PC game, at 60 fps. Lower the bitrate if the picture stutters on busy Wi-Fi. While you play, Moonlight reads your controller. {rareWay()} to come back to Rare and resume or quit.
          </p>
        </div>
      </div>
    </Sheet>
  );
}
