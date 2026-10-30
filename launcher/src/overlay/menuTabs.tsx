// The Rare menu's tabs that are not about one game's market: Rare (over the launcher with nothing
// in front), Friends and Notifications. Rows are the kit's 3.25rem focusable rows; statuses are
// muted text or a Dot, never a pill. Acting on something that lives in the launcher (a friend's
// page, a notice's item) leaves the game for Home first, the way a Switcher pick of Home does.

import { useEffect, useState } from "react";
import { native } from "../lib/bridge";
import { useApp } from "../lib/store";
import { feedback } from "../nav/feedback";
import { actionLabel, runAction } from "../notify/actions";
import { noticeIsRead, useNotify, useUnread, useVisibleNotices } from "../notify/store";
import { NoticeArt } from "../notify/Toast";
import { joinGame, runningRef } from "../friends/join";
import { openFriends, type FriendsTarget } from "../friends/nav";
import * as F from "../friends/store";
import { activeParty, partyInvites, useFriends } from "../friends/store";
import type { Friend } from "../friends/types";
import { FriendAvatar, PresenceLine } from "../friends/ui/bits";
import { Button, Dot, Row, timeAgo } from "../ui";
import { leaveForHome } from "./session";
import { Calm } from "./panels";

const ROW = "grid-cols-[minmax(0,1fr)_auto]";
const PERSON = "grid-cols-[2.25rem_minmax(0,1fr)_auto]";

/** Where the menu is: over a game (leave it for Home before acting in the launcher) or the launcher. */
export interface Where {
  overGame: boolean;
  /** Closes the menu (the launcher's own, or back to the game). */
  close: () => void;
}

/** Runs `then` in the launcher: over a game the game is left for Home first. */
async function inLauncher(w: Where, then: () => void) {
  if (w.overGame) await leaveForHome();
  else w.close();
  then();
}

function Section({ title, children, note }: { title: string; children: React.ReactNode; note?: string }) {
  return (
    <section className="flex flex-col">
      <div className="flex items-baseline gap-[0.5rem] px-[0.75rem] pb-[0.25rem] pt-[0.7rem] text-label font-medium text-secondary">
        {title}
        {note && <span className="tabular-nums">{note}</span>}
      </div>
      {children}
    </section>
  );
}

function Line({ title, sub }: { title: React.ReactNode; sub?: React.ReactNode }) {
  return (
    <span className="min-w-0">
      <span className="block truncate font-medium">{title}</span>
      {sub && <span className="block truncate text-label text-secondary">{sub}</span>}
    </span>
  );
}

// ---------- Rare (over the launcher) ----------

/** Home, your profile, Settings, Power, and Desktop mode last under a hairline. */
export function RareTab({ w }: { w: Where }) {
  const s = useApp.getState();
  const power = (a: "suspend" | "reboot" | "poweroff" | "desktop") => {
    feedback("select");
    w.close();
    void native.power(a).catch((e) => s.toast(String(e), "error"));
  };
  return (
    <div data-nav-group="menu-rare" className="flex flex-col">
      <Row cols={ROW} hint="Home" onClick={() => (w.close(), s.setTab("home"))}>
        <Line title="Home" />
      </Row>
      <Row cols={ROW} hint="Open" onClick={() => (w.close(), s.open({ kind: "profile" }))}>
        <Line title="Your profile" />
      </Row>
      <Row cols={ROW} hint="Open" onClick={() => (w.close(), s.setTab("settings"))}>
        <Line title="Settings" />
      </Row>
      <Section title="Power">
        <Row cols={ROW} hint="Sleep" onClick={() => power("suspend")}>
          <Line title="Sleep" />
        </Row>
        <Row cols={ROW} hint="Restart" onClick={() => power("reboot")}>
          <Line title="Restart" />
        </Row>
        <Row cols={ROW} hint="Turn off" onClick={() => power("poweroff")}>
          <Line title="Turn off" />
        </Row>
      </Section>
      <div className="mx-[0.75rem] my-[0.5rem] border-t border-hair" />
      <Row cols={ROW} hint="Desktop mode" onClick={() => power("desktop")}>
        <Line title="Desktop mode" />
      </Row>
    </div>
  );
}

// ---------- Friends ----------

/** Party (invite, join, leave), Playing, Online, requests. A on a friend's game joins it. */
export function FriendsTab({ w }: { w: Where }) {
  const s = useFriends();
  const [busy, setBusy] = useState(false);
  const fail = (e: unknown) => useApp.getState().toast(F.messageOf(e), "error");
  const go = (t: FriendsTarget) => void inLauncher(w, () => openFriends(t));
  useEffect(() => {
    if (s.status === "none") void F.syncAccount();
  }, [s.status]);

  if (s.status === "none") return <Calm text="Sign in with a passkey to see your friends." />;
  if (s.status === "watch") return <Calm text="Add a passkey to see your friends." />;
  if (s.status === "error") return <Calm text={s.error ?? "Friends didn't load. Try again in a moment."} />;
  if (s.status === "unlinked" || s.status === "linking")
    return (
      <div className="flex flex-col items-start gap-[0.8rem] px-[0.75rem] py-[1.2rem]">
        <p className="text-small text-secondary">Turn on friends to see who is playing and join their games.</p>
        <Button variant="secondary" size="md" disabled={s.status === "linking" || busy} hint="Turn on" onClick={() => (setBusy(true), void F.turnOn().catch(fail).finally(() => setBusy(false)))}>
          {s.status === "linking" ? "Turning on..." : "Turn on friends"}
        </Button>
      </div>
    );
  if (s.status !== "ready") return null;

  const party = activeParty(s);
  const invite = partyInvites(s)[0] ?? null;
  const playing = s.friends.filter((f) => f.presence.state !== "offline" && f.presence.game);
  const online = s.friends.filter((f) => f.presence.state !== "offline" && !f.presence.game);
  const requests = s.received.length;
  const host = (id: string) => F.label(s.parties.find((p) => p.id === id)?.members.find((m) => m.address === s.parties.find((p) => p.id === id)?.host));

  return (
    <div className="flex flex-col">
      <Section title="Party">
        <div data-nav-group="menu-party" className="flex flex-col">
          {invite && !party && (
            <div data-invite-join>
              <Row cols={ROW} hint="Join party" onClick={() => void F.joinParty(invite.id).catch(fail)}>
                <Line title={`${host(invite.id)} invited you`} sub={invite.game ? invite.game.title : `${invite.members.filter((m) => m.state === "joined").length} in the party`} />
                <span className="text-label text-secondary">Join</span>
              </Row>
            </div>
          )}
          {party ? (
            <>
              <Row cols={ROW} hint="Invite friends" onClick={() => go({ kind: "party", id: party.id })}>
                <Line title={party.host === s.address ? "Your party" : `${host(party.id)}'s party`} sub={party.members.filter((m) => m.state === "joined").map((m) => (m.address === s.address ? "you" : F.label(m))).join(", ")} />
                <span className="text-label text-secondary">Invite</span>
              </Row>
              <Row cols={ROW} hint="Leave party" onClick={() => void F.leaveParty(party.id).catch(fail)}>
                <Line title="Leave party" />
              </Row>
            </>
          ) : (
            <Row cols={ROW} hint="Start a party" onClick={() => void F.createParty(runningRef(null), []).then(() => go({ kind: "party", id: activeParty(useFriends.getState())?.id ?? "" })).catch(fail)}>
              <Line title="Start a party" sub="Then invite friends to this game" />
            </Row>
          )}
        </div>
      </Section>
      {playing.length > 0 && (
        <Section title="Playing" note={String(playing.length)}>
          <div data-nav-group="menu-playing" className="flex flex-col">
            {playing.map((f) => (
              <FriendRow key={f.address} f={f} hint="Join game" onClick={() => void inLauncher(w, () => void joinGame(f.presence.game!).catch(fail))} />
            ))}
          </div>
        </Section>
      )}
      {online.length > 0 && (
        <Section title="Online" note={String(online.length)}>
          <div data-nav-group="menu-online" className="flex flex-col">
            {online.map((f) => (
              <FriendRow key={f.address} f={f} hint="Open" onClick={() => go({ kind: "friend", address: f.address })} />
            ))}
          </div>
        </Section>
      )}
      {requests > 0 && (
        <Section title="Requests">
          <Row cols={ROW} hint="Open" onClick={() => go(null)}>
            <Line title={requests === 1 ? "1 friend request" : `${requests} friend requests`} />
            <Dot tone="brand" />
          </Row>
        </Section>
      )}
      {!playing.length && !online.length && !requests && <Calm text="None of your friends are online right now." />}
    </div>
  );
}

function FriendRow({ f, hint, onClick }: { f: Friend; hint: string; onClick: () => void }) {
  return (
    <Row cols={PERSON} hint={hint} onClick={onClick} className="h-[3.5rem]">
      <FriendAvatar p={f} size="2.25rem" />
      <span className="min-w-0">
        <span className="block truncate font-medium">{F.label(f)}</span>
        <PresenceLine p={f.presence} className="block truncate text-label text-secondary" />
      </span>
      <span />
    </Row>
  );
}

// ---------- Notifications ----------

/** Newest first, unread dot. A acts on one: the item, the offer or the invite. */
export function NoticesTab({ w }: { w: Where }) {
  const list = useVisibleNotices();
  const unread = useUnread();
  const read = useNotify((s) => s.read);
  const readAt = useNotify((s) => s.readAt);
  const markRead = useNotify((s) => s.markRead);
  const markAllRead = useNotify((s) => s.markAllRead);
  const now = Date.now();
  if (!list.length) return <Calm text="Nothing new. Sales, offers, invites and trophies show up here." />;
  return (
    <div className="flex flex-col">
      {unread > 0 && (
        <div data-nav-group="menu-news-head" className="flex items-center px-[0.75rem] pb-[0.3rem] pt-[0.4rem]">
          <span className="text-label tabular-nums text-secondary">{unread} new</span>
          <button data-nav data-hint-a="Mark all read" onClick={markAllRead} className="btn ml-auto h-[1.9rem] rounded-chip bg-l2 px-[0.7rem] text-meta font-medium text-fg">
            Mark all read
          </button>
        </div>
      )}
      <div data-nav-group="menu-news" className="flex flex-col">
        {list.map((n, i) => {
          const isRead = noticeIsRead({ read, readAt }, n);
          return (
            <Row
              key={n.id}
              idx={i}
              cols="grid-cols-[2.5rem_minmax(0,1fr)_auto]"
              className="h-[3.5rem]"
              hint={actionLabel(n.action, n)}
              onClick={() => {
                markRead(n.id);
                void inLauncher(w, () => runAction(n.action, n.digest, n));
              }}
            >
              <NoticeArt n={n} className="h-[2.5rem] w-[2.5rem]" />
              <span className="min-w-0">
                <span className={`block truncate text-small ${isRead ? "text-fg/80" : "font-semibold text-fg"}`}>{n.title}</span>
                <span className="block truncate text-label text-secondary">{[timeAgo(n.createdAt, now), n.body].filter(Boolean).join(" · ")}</span>
              </span>
              {isRead ? <span /> : <Dot tone="brand" />}
            </Row>
          );
        })}
      </div>
    </div>
  );
}
