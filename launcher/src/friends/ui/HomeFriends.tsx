// Home: the party (yours, or an invite) as one panel with Join, and "Friends playing" as a shelf:
// each card is the friend's game art with their picture, name and game under it (rare.market's
// games card). A opens the friend in Friends, X joins their game.

import { useApp } from "../../lib/store";
import { usePadHandler } from "../../nav/input";
import { Button, Panel, Rail } from "../../ui";
import { joinGame } from "../join";
import { openFriends } from "../nav";
import * as F from "../store";
import { activeParty, partyInvites, useFriends } from "../store";
import type { Friend, Party } from "../types";
import { FriendAvatar, GameArt } from "./bits";

const toastError = (e: unknown) => useApp.getState().toast(F.messageOf(e), "error");

export function HomeFriends() {
  const s = useFriends();
  const running = useApp((x) => x.running);
  // X on a friend card joins their game.
  usePadHandler((b) => {
    if (b !== "x") return false;
    const el = document.activeElement as HTMLElement | null;
    const a = el?.dataset?.friendJoin;
    const f = a ? useFriends.getState().friends.find((x) => x.address === a) : null;
    if (!f?.presence.game) return false;
    void joinGame(f.presence.game).catch(toastError);
    return true;
  }, s.status === "ready");
  if (s.status !== "ready") return null;
  const party = activeParty(s);
  const invite = partyInvites(s)[0] ?? null;
  const playing = s.friends.filter((f) => f.presence.state !== "offline" && f.presence.game);
  if (!party && !invite && !playing.length) return null;
  return (
    <>
      {(party ?? invite) && <PartyStrip p={(party ?? invite)!} me={s.address} invite={!party} running={running} />}
      {playing.length > 0 && (
        <Rail id="friends-playing" title="Friends playing" note={`${playing.length}`}>
          {playing.slice(0, 12).map((f) => (
            <FriendCard key={f.address} f={f} />
          ))}
        </Rail>
      )}
    </>
  );
}

function FriendCard({ f }: { f: Friend }) {
  const g = f.presence.game!;
  return (
    <div className="w-[15rem] shrink-0">
      <button
        data-nav
        data-hint-a="Open"
        data-hint-x="Join game"
        data-friend-join={f.address}
        onClick={() => openFriends({ kind: "friend", address: f.address })}
        className="tile relative block aspect-[460/215] w-full overflow-hidden rounded-card bg-l3"
      >
        <GameArt game={g} kind="header" className="absolute inset-0 h-full w-full object-cover" />
      </button>
      <div className="mt-[0.75rem] flex items-center gap-[0.6rem] px-[0.1rem]">
        <FriendAvatar p={f} size="1.75rem" />
        <span className="min-w-0">
          <span className="focus-text block truncate text-meta font-medium text-fg/85">{F.label(f)}</span>
          <span className="block truncate text-label text-muted">{g.title}</span>
        </span>
      </div>
    </div>
  );
}

function PartyStrip({ p, me, invite, running }: { p: Party; me: string | null; invite: boolean; running: string[] }) {
  const host = p.members.find((m) => m.address === p.host);
  const joined = p.members.filter((m) => m.state === "joined");
  const g = p.game;
  const playingIt = !!g && running.includes(g.id);
  return (
    <section className="px-[2.5rem]">
      <Panel className="flex items-center gap-[1.25rem] px-[1.4rem] py-[1rem]">
        <span className="flex shrink-0 -space-x-[0.7rem]">
          {joined.slice(0, 5).map((m) => (
            <span key={m.address} className="rounded-full ring-2 ring-[rgb(var(--layer-01))]">
              <FriendAvatar p={m} size="2.25rem" />
            </span>
          ))}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-body font-semibold">{invite ? `${F.label(host)} invited you to a party` : p.host === me ? "Your party" : `${F.label(host)}'s party`}</span>
          <span className="block truncate text-small text-muted">
            {g ? `${g.title}, ` : ""}
            {joined.map((m) => (m.address === me ? "you" : F.label(m))).join(", ")}
          </span>
        </span>
        <span data-nav-group="home-party" className="flex shrink-0 gap-[0.75rem]">
          {invite ? (
            <>
              <Button variant="primary" onClick={() => void F.joinParty(p.id).catch(toastError)}>
                Join party
              </Button>
              <Button onClick={() => void F.leaveParty(p.id).catch(toastError)}>Decline</Button>
            </>
          ) : (
            <>
              {g && !playingIt && (
                <Button variant="primary" onClick={() => void joinGame(g).catch(toastError)}>
                  Join game
                </Button>
              )}
              <Button onClick={() => openFriends({ kind: "party", id: p.id })}>Open party</Button>
            </>
          )}
        </span>
      </Panel>
    </section>
  );
}
