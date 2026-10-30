// PC games as covers: the "On your PC" row on Home and Library > Streaming. Same shape, focus
// and second line as the launcher's game covers (components/ui GameTile); box art is what the PC
// serves (Sunshine's app images), cached by Rare.

import { useEffect } from "react";
import { Monitor } from "lucide-react";
import { Art } from "../components/Art";
import { useApp } from "../lib/store";
import { Button, Dot, Empty, LoadBar, Rail, SectionHead } from "../ui";
import { homeHost, ordered, pcGames, playState, tileLine } from "./logic";
import { startStream, useStream } from "./state";
import type { Host, StreamApp } from "./types";

export function StreamTile({ host, app, def }: { host: Host; app: StreamApp; def?: boolean }) {
  const running = useApp((s) => s.running);
  const state = playState(host, app, running);
  return (
    <div className="w-[9rem] shrink-0">
      <button
        data-nav
        data-nav-default={def ? "" : undefined}
        data-hint-a="Details"
        data-stream={`${host.uuid}:${app.id}`}
        onClick={() => useApp.getState().open({ kind: "streamapp", host: host.uuid, app: app.id })}
        className="tile relative block aspect-[2/3] w-full overflow-hidden rounded-card bg-l3 text-left"
      >
        <Art sources={app.art ? [app.art] : []} title={app.name} variant="cover" className="absolute inset-0 h-full w-full object-cover" />
        {(state === "playing" || state === "on-pc") && <Dot tone="primary" live={state === "playing"} className="absolute right-[0.6rem] top-[0.6rem] ring-2 ring-black/40" />}
      </button>
      <div className="focus-text mt-[0.85rem] truncate px-[0.1rem] text-meta font-medium text-fg/85">{app.name}</div>
      <div className="truncate px-[0.1rem] text-label text-muted">{tileLine(host, state)}</div>
    </div>
  );
}

/** Home: the games on the paired PC that answers, as soon as it does. */
export function OnYourPcRail() {
  useEffect(startStream, []);
  const hosts = useStream((s) => s.hosts);
  const apps = useStream((s) => s.apps);
  const host = homeHost(hosts);
  if (!host || !apps[host.uuid]?.length) return null;
  return (
    <Rail id="stream-pc" title="On your PC" note={host.name}>
      {ordered(apps[host.uuid]).slice(0, 16).map((a) => (
        <StreamTile key={a.id} host={host} app={a} />
      ))}
    </Rail>
  );
}

function HostHead({ host }: { host: Host }) {
  const waking = useStream((s) => s.waking === host.uuid);
  const status = host.online ? (host.currentGame != null ? "Playing on the PC" : "Online") : waking ? "Waking up" : host.canWake ? "Asleep" : "Off";
  return (
    <SectionHead
      title={host.name}
      note={
        <span className="flex items-center gap-[0.45rem]">
          <Dot tone={host.online ? "primary" : "muted"} live={waking} />
          {status}
        </span>
      }
      right={host.software ?? undefined}
      className="mb-[0.9rem]"
    />
  );
}

/** Library > Streaming: every paired PC with its games, or how to set one up. */
export function StreamGrid() {
  useEffect(startStream, []);
  const s = useStream();
  const paired = s.hosts.filter((h) => h.paired);
  const open = () => useApp.getState().open({ kind: "stream" });
  if (!s.loaded) return <div className="h-[16rem]" />;
  if (!s.moonlight || paired.length === 0)
    return (
      <Empty title="Play the games on your PC" body="Stream from a gaming PC running Sunshine or GeForce Experience, over your home Wi-Fi.">
        <Button variant="primary" def hint="Set up streaming" icon={<Monitor size="1.1rem" />} onClick={open}>
          Set up streaming
        </Button>
      </Empty>
    );
  const games = pcGames(paired, s.apps);
  let first = true;
  return (
    <div className="relative flex flex-col gap-[2.25rem]">
      <LoadBar on={!!s.listing} className="-top-[1.25rem]" />
      {paired.map((host) => {
        const list = ordered(games.filter((g) => g.host.uuid === host.uuid).map((g) => g.app));
        return (
          <section key={host.uuid}>
            <HostHead host={host} />
            {list.length === 0 ? (
              s.listing === host.uuid ? (
                <div className="h-[13.5rem]" />
              ) : (
                <Empty title={host.online ? `No games listed on ${host.name}` : `${host.name} isn't answering`} body={host.online ? "Add games in Sunshine's Applications tab, then look again." : "Turn it on, or wake it from here if Wake-on-LAN is set up."}>
                  {host.online ? (
                    <Button def={first} onClick={() => void s.listApps(host.uuid)}>
                      Look again
                    </Button>
                  ) : host.canWake ? (
                    <Button variant="primary" def={first} onClick={() => void s.wake(host).then((ok) => ok && void s.listApps(host.uuid))}>
                      Wake {host.name}
                    </Button>
                  ) : null}
                </Empty>
              )
            ) : (
              <div data-nav-group={`library-stream-${host.uuid}`} className="grid grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-x-[1.1rem] gap-y-[1.4rem]">
                {list.map((a) => {
                  const def = first;
                  first = false;
                  return <StreamTile key={a.id} host={host} app={a} def={def} />;
                })}
              </div>
            )}
          </section>
        );
      })}
    </div>
  );
}
