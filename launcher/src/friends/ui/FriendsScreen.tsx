// Friends: the list on the left (Settings' section list: add, party, requests, playing, online,
// offline), the focused entry on the right. Rows are the kit's 3.25rem rows on one panel; the
// right side is one panel with the player's picture, presence and actions. A picker (invite
// friends, pick a game, pick an item to send) takes the right side over until B.

import { Check, Shield, UserPlus, Users } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { askText } from "../../components/Osk";
import { rare, type ItemCard } from "../../lib/rare";
import { useApp } from "../../lib/store";
import { focusEl, focusFirst } from "../../nav/focus";
import { feedback } from "../../nav/feedback";
import { usePadHandler } from "../../nav/input";
import { SwitchRow } from "../../screens/Settings";
import { Button, Empty, LoadBar, Panel, StatLine } from "../../ui";
import { MImg } from "../../marketplace/MImg";
import { useWalletNav } from "../../wallet/ui/store";
import { gameRef, joinGame } from "../join";
import { useFriendsNav } from "../nav";
import { shown } from "../display";
import { resolveTarget } from "../resolve";
import * as F from "../store";
import { activeParty, partyInvites, useFriends } from "../store";
import type { Friend, GameRef, Party, Pending, Person } from "../types";
import { AddPane } from "./AddPane";
import { FriendAvatar, GameArt, PresenceLine, presenceText, shortAddress } from "./bits";
import { PartyPane } from "./PartyPane";

export type Sel =
  | { kind: "add" }
  | { kind: "privacy" }
  | { kind: "blocked" }
  | { kind: "new-party" }
  | { kind: "party"; id: string }
  | { kind: "friend"; address: string }
  | { kind: "request"; address: string }
  | { kind: "sent"; address: string };

export type Picker = { kind: "invite"; partyId: string | null } | { kind: "game"; partyId: string } | { kind: "item"; friend: Person } | null;

const selKey = (s: Sel) => ("address" in s ? `${s.kind}:${s.address}` : "id" in s ? `${s.kind}:${s.id}` : s.kind);

/** Wallet send flows, prefilled with the friend's SuiNS name (or address). */
export function sendCoins(p: Person) {
  useApp.getState().setTab("wallet");
  useWalletNav.getState().reset([{ kind: "send", to: p.suins ?? p.address }]);
}
export function sendItem(p: Person, id: string) {
  useApp.getState().setTab("wallet");
  useWalletNav.getState().reset([{ kind: "send-nfts", ids: [id], to: p.suins ?? p.address }]);
}

export const toastError = (e: unknown) => useApp.getState().toast(F.messageOf(e), "error");

export function Friends() {
  const s = useFriends();
  const [sel, setSel] = useState<Sel>({ kind: "add" });
  const [picker, setPicker] = useState<Picker>(null);
  const right = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const target = useFriendsNav((x) => x.target);
  const seq = useFriendsNav((x) => x.seq);

  // Opened at a person or a party (a notice, Home): select it and put focus on its row.
  useEffect(() => {
    if (!target || s.status !== "ready") return;
    let next: Sel | null = null;
    if (target.kind === "add") next = { kind: "add" };
    else if (target.kind === "party") next = s.parties.some((p) => p.id === target.id) ? { kind: "party", id: target.id } : null;
    else {
      const a = target.address;
      next = s.friends.some((f) => f.address === a)
        ? { kind: "friend", address: a }
        : s.received.some((f) => f.address === a)
          ? { kind: "request", address: a }
          : null;
    }
    useFriendsNav.setState({ target: null });
    if (!next) return;
    setPicker(null);
    setSel(next);
    const key = selKey(next);
    setTimeout(() => focusEl(list.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(key)}"]`)), 60);
  }, [seq, s.status, target]); // eslint-disable-line react-hooks/exhaustive-deps

  usePadHandler((b) => {
    if (b === "b" && picker) {
      setPicker(null);
      feedback("back");
      return true;
    }
    return false;
  });

  // The picked entry went away (a request accepted elsewhere, a party ended): back to the top.
  useEffect(() => {
    const gone =
      (sel.kind === "friend" && !s.friends.some((f) => f.address === sel.address)) ||
      (sel.kind === "request" && !s.received.some((f) => f.address === sel.address)) ||
      (sel.kind === "sent" && !s.sent.some((f) => f.address === sel.address)) ||
      (sel.kind === "party" && !s.parties.some((p) => p.id === sel.id));
    if (gone && s.status === "ready") {
      const p = sel.kind === "request" ? s.friends.find((f) => f.address === sel.address) : null;
      setSel(p ? { kind: "friend", address: p.address } : { kind: "add" });
    }
  }, [s.friends, s.received, s.sent, s.parties, s.status, sel]);

  const toRight = () => setTimeout(() => right.current && focusFirst(right.current), 30);
  const openPicker = (p: Picker) => {
    setPicker(p);
    feedback("open");
    setTimeout(() => right.current && focusFirst(right.current), 60);
  };

  if (s.status !== "ready") return <Gate />;

  const playing = s.friends.filter((f) => f.presence.state !== "offline" && f.presence.game);
  const online = s.friends.filter((f) => f.presence.state !== "offline" && !f.presence.game);
  const offline = s.friends.filter((f) => f.presence.state === "offline");
  const party = activeParty(s);
  const invites = partyInvites(s);
  const row = (k: Sel, children: ReactNode, hint = "Open") => (
    <ListRow key={selKey(k)} k={k} sel={sel} onSel={(v) => (setPicker(null), setSel(v))} onA={toRight} hint={hint}>
      {children}
    </ListRow>
  );

  return (
    <div className="relative flex h-full gap-[2rem] px-[2.5rem] pb-[1.5rem] pt-[0.5rem]">
      <LoadBar on={s.busy} />
      <div ref={list} data-scroll data-nav-group="friends-list" data-nav-remember className="-ml-[1rem] flex w-[24rem] shrink-0 flex-col gap-[0.15rem] overflow-y-auto pb-[1rem] pl-[0.25rem] pr-[0.5rem]">
        <div className="mb-[0.6rem] flex items-baseline gap-[0.75rem] px-[1rem]">
          <h1 className="text-title font-semibold">Friends</h1>
          <span className="text-small text-muted">{s.friends.length ? `${playing.length + online.length} online` : ""}</span>
        </div>
        {row({ kind: "add" }, <IconLine icon={<UserPlus size="1.15rem" />} title="Add a friend" body="By username, SuiNS name or address" />)}
        {party
          ? row({ kind: "party", id: party.id }, <PartyLine p={party} me={s.address} />, "Open party")
          : row({ kind: "new-party" }, <IconLine icon={<Users size="1.15rem" />} title="Start a party" body="Play together and chat" />)}
        {invites.map((p) => row({ kind: "party", id: p.id }, <PartyLine p={p} me={s.address} invite />, "Open invite"))}

        <Group title="Requests" n={s.received.length} />
        {s.received.map((f) => row({ kind: "request", address: f.address }, <PersonLine p={f} line="Wants to be your friend" />))}
        <Group title="Playing" n={playing.length} />
        {playing.map((f) => row({ kind: "friend", address: f.address }, <PersonLine p={f} presence={f.presence} />))}
        <Group title="Online" n={online.length} />
        {online.map((f) => row({ kind: "friend", address: f.address }, <PersonLine p={f} presence={f.presence} />))}
        <Group title="Offline" n={offline.length} />
        {offline.map((f) => row({ kind: "friend", address: f.address }, <PersonLine p={f} presence={f.presence} />))}
        <Group title="Sent" n={s.sent.length} />
        {s.sent.map((f) => row({ kind: "sent", address: f.address }, <PersonLine p={f} line="Request sent" />))}
        {!s.friends.length && !s.received.length && !s.sent.length && (
          <p className="px-[1rem] pt-[0.75rem] text-small text-muted">No friends yet. Add one by username, SuiNS name or address.</p>
        )}
        <div className="mt-[1rem]" />
        {s.blocked.length > 0 && row({ kind: "blocked" }, <IconLine title="Blocked" body={`${s.blocked.length} ${s.blocked.length === 1 ? "player" : "players"}`} />)}
        {row({ kind: "privacy" }, <IconLine icon={<Shield size="1.15rem" />} title="Privacy" body={privacyLine(s.me?.settings)} />)}
      </div>

      <div ref={right} data-scroll data-nav-group="friends-detail" className="min-w-0 flex-1 overflow-y-auto pb-[1rem]">
        {picker ? (
          <Pickers picker={picker} done={() => (setPicker(null), toRight())} />
        ) : (
          <Detail key={selKey(sel)} sel={sel} openPicker={openPicker} select={(v) => (setSel(v), toRight())} />
        )}
      </div>
    </div>
  );
}

const privacyLine = (p: { showPlaying: boolean; appearOffline: boolean; visibility: string } | undefined) =>
  !p ? "" : p.appearOffline ? "Appearing offline" : p.visibility === "everyone" ? "Status shown to everyone" : p.showPlaying ? "Friends see what you play" : "Friends see you online";

function Group({ title, n }: { title: string; n: number }) {
  if (!n) return null;
  return (
    <div className="flex items-baseline gap-[0.5rem] px-[1rem] pb-[0.25rem] pt-[1rem] text-label font-medium text-secondary">
      {title}
      <span className="tabular-nums text-muted">{n}</span>
    </div>
  );
}

function ListRow({ k, sel, onSel, onA, hint, children }: { k: Sel; sel: Sel; onSel: (s: Sel) => void; onA: () => void; hint: string; children: ReactNode }) {
  const on = selKey(k) === selKey(sel);
  return (
    <button
      data-nav
      data-key={selKey(k)}
      data-hint-a={hint}
      onFocus={() => onSel(k)}
      onClick={() => {
        onSel(k);
        onA();
      }}
      className={`relative flex min-h-[3.25rem] w-full items-center gap-[0.8rem] rounded-card px-[1rem] py-[0.45rem] text-left ${on ? "bg-l2" : ""}`}
    >
      {on && <span className="absolute inset-y-[0.7rem] left-0 w-[3px] rounded-full bg-fg" />}
      {children}
    </button>
  );
}

function IconLine({ icon, title, body }: { icon?: ReactNode; title: string; body: string }) {
  return (
    <>
      {icon && <span className="flex size-[2.25rem] shrink-0 items-center justify-center text-fg/80">{icon}</span>}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-body font-medium">{title}</span>
        <span className="block truncate text-meta text-muted">{body}</span>
      </span>
    </>
  );
}

/** The SuiNS name next to a username: muted text, nothing else. */
function Secondary({ p }: { p: Person }) {
  const s = shown(p).secondary;
  return s ? <span className="truncate text-meta text-muted">{s}</span> : null;
}

function PersonLine({ p, presence, line }: { p: Person; presence?: Friend["presence"]; line?: string }) {
  return (
    <>
      <FriendAvatar p={p} />
      <span className="min-w-0 flex-1">
        <span className="flex min-w-0 items-baseline gap-[0.6rem]">
          <span className="truncate text-body font-medium">{F.label(p)}</span>
          <Secondary p={p} />
        </span>
        {presence ? <PresenceLine p={presence} className="text-meta" /> : <span className="block truncate text-meta text-muted">{line}</span>}
      </span>
    </>
  );
}

function PartyLine({ p, me, invite }: { p: Party; me: string | null; invite?: boolean }) {
  const host = p.members.find((m) => m.address === p.host);
  const joined = p.members.filter((m) => m.state === "joined");
  return (
    <>
      <span className="flex shrink-0 -space-x-[0.6rem]">
        {joined.slice(0, 3).map((m) => (
          <span key={m.address} className="rounded-full ring-2 ring-[rgb(var(--layer-01))]">
            <FriendAvatar p={m} size="1.75rem" />
          </span>
        ))}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-body font-medium">{invite ? `Party invite from ${F.label(host)}` : host?.address === me ? "Your party" : `${F.label(host)}'s party`}</span>
        <span className="block truncate text-meta text-muted">
          {p.game ? `${p.game.title}, ` : ""}
          {joined.length} {joined.length === 1 ? "player" : "players"}
        </span>
      </span>
    </>
  );
}

/** Before the list: no account, a watch account, friends not turned on yet, loading. */
function Gate() {
  const s = useFriends();
  const body: ReactNode =
    s.status === "none" ? (
      <Empty title="Add a Rare Wallet account to use friends" body="Your friends know you by your Sui address, shown with your SuiNS name and NFT picture.">
        <Button variant="primary" def onClick={() => useApp.getState().setTab("wallet")}>
          Open Rare Wallet
        </Button>
      </Empty>
    ) : s.status === "watch" ? (
      <Empty title="This account is watch only" body="Friends need an account that can sign. Pick or add one in Rare Wallet.">
        <Button variant="primary" def onClick={() => useApp.getState().setTab("wallet")}>
          Open Rare Wallet
        </Button>
      </Empty>
    ) : s.status === "unlinked" || s.status === "linking" ? (
      <Empty
        title="Turn on friends"
        body={
          <>
            Rare Wallet signs one message that lets this device handle your friends, parties and presence. It does not move funds or cost gas. Your friend list stays private on
            rare.market, never on chain.
            {s.error && <span className="mt-[0.5rem] block text-negative">{s.error}</span>}
          </>
        }
      >
        <Button variant="primary" def disabled={s.status === "linking"} onClick={() => void F.turnOn()}>
          {s.status === "linking" ? "Approve in Rare Wallet" : "Turn on friends"}
        </Button>
      </Empty>
    ) : s.status === "error" ? (
      <Empty title="Friends could not load" body={s.error ?? "Check your connection."}>
        <Button def onClick={() => void F.refresh()}>
          Try again
        </Button>
      </Empty>
    ) : null;
  return (
    <div className="relative h-full px-[2.5rem] pt-[0.5rem]">
      <LoadBar on={s.status === "loading" || s.status === "linking"} />
      <h1 className="mb-[1.25rem] text-title font-semibold">Friends</h1>
      {body && <div className="max-w-[44rem]">{body}</div>}
    </div>
  );
}

// ---------- right side ----------

function Detail({ sel, openPicker, select }: { sel: Sel; openPicker: (p: Picker) => void; select: (s: Sel) => void }) {
  const s = useFriends();
  switch (sel.kind) {
    case "add":
      return <AddPane />;
    case "privacy":
      return <Privacy />;
    case "blocked":
      return <Blocked list={s.blocked} />;
    case "new-party":
      return <NewParty openPicker={openPicker} select={select} />;
    case "party": {
      const p = s.parties.find((x) => x.id === sel.id);
      return p ? <PartyPane party={p} openPicker={openPicker} /> : null;
    }
    case "friend": {
      const f = s.friends.find((x) => x.address === sel.address);
      return f ? <FriendPane f={f} openPicker={openPicker} select={select} /> : null;
    }
    case "request": {
      const f = s.received.find((x) => x.address === sel.address);
      return f ? <RequestPane f={f} /> : null;
    }
    case "sent": {
      const f = s.sent.find((x) => x.address === sel.address);
      return f ? <SentPane f={f} /> : null;
    }
  }
}

export function PersonHead({ p, children }: { p: Person; children?: ReactNode }) {
  return (
    <div className="flex items-center gap-[1.25rem]">
      <FriendAvatar p={p} size="5rem" />
      <div className="min-w-0">
        <div className="truncate text-title font-semibold">{F.label(p)}</div>
        {shown(p).secondary && <div className="truncate text-body text-muted">{shown(p).secondary}</div>}
        <div className="num truncate text-meta text-muted">{p.address}</div>
        {children && <div className="mt-[0.35rem] text-small">{children}</div>}
      </div>
    </div>
  );
}

const act = (fn: () => Promise<unknown>, done?: string) => () =>
  void fn()
    .then(() => done && useApp.getState().toast(done))
    .catch(toastError);

function FriendPane({ f, openPicker, select }: { f: Friend; openPicker: (p: Picker) => void; select: (s: Sel) => void }) {
  const s = useFriends();
  const party = activeParty(s);
  const g = f.presence.state !== "offline" ? f.presence.game : null;
  const inParty = party?.members.some((m) => m.address === f.address);
  const [sure, setSure] = useState<"remove" | "block" | null>(null);
  const invite = async () => {
    if (party) {
      await F.inviteToParty(party.id, [f.address]);
      useApp.getState().toast(`Invited ${F.label(f)} to your party`);
    } else {
      const p = await F.createParty(currentGame(), [f.address]);
      useApp.getState().toast(`Party started. ${F.label(f)} is invited`);
      select({ kind: "party", id: p.id });
    }
  };
  return (
    <Panel className="fade flex flex-col gap-[1.4rem] px-[1.6rem] py-[1.5rem]">
      <PersonHead p={f}>
        <PresenceLine p={f.presence} long />
      </PersonHead>

      {g && (
        <div className="flex items-center gap-[1.1rem] border-t border-hair pt-[1.2rem]">
          <div className="relative aspect-[2/3] w-[5.5rem] shrink-0 overflow-hidden rounded-card bg-l3">
            <GameArt game={g} className="absolute inset-0 h-full w-full object-cover" />
          </div>
          <div className="min-w-0 flex-1">
            <div className="text-label text-muted">Playing</div>
            <div className="truncate text-row font-semibold">{g.title}</div>
            <div className="text-meta text-muted">{joinNote(g)}</div>
          </div>
          <Button variant="primary" def hint="Join" onClick={() => void joinGame(g).catch(toastError)}>
            Join game
          </Button>
        </div>
      )}

      <div data-nav-group="friend-actions" data-nav-remember className="flex flex-wrap gap-[0.75rem]">
        {!inParty && (
          <Button def={!g} onClick={act(invite)}>
            {party ? "Invite to party" : "Start a party"}
          </Button>
        )}
        <Button onClick={() => sendCoins(f)}>Send coins</Button>
        <Button onClick={() => openPicker({ kind: "item", friend: f })}>Send an item</Button>
        {sure ? (
          <>
            <Button variant="outline" onClick={act(() => F.graph(sure, f.address), sure === "remove" ? `Removed ${F.label(f)}` : `Blocked ${F.label(f)}`)}>
              {sure === "remove" ? "Yes, remove" : "Yes, block"}
            </Button>
            <Button variant="ghost" onClick={() => setSure(null)}>
              Keep
            </Button>
          </>
        ) : (
          <>
            <Button variant="ghost" onClick={() => setSure("remove")}>
              Remove friend
            </Button>
            <Button variant="ghost" onClick={() => setSure("block")}>
              Block
            </Button>
          </>
        )}
      </div>

      <div className="border-t border-hair pt-[1.2rem]">
        <StatLine
          items={[
            { label: "Friends since", value: new Date(f.since).toLocaleDateString([], { month: "short", day: "numeric", year: "numeric" }) },
            ...(f.presence.state === "offline" && f.presence.lastSeen ? [{ label: "Last seen", value: presenceText(f.presence).replace(/^Last seen /, "") }] : []),
          ]}
        />
      </div>
    </Panel>
  );
}

/** How Join works for this game, in one line. */
function joinNote(g: GameRef): string {
  const own = useApp.getState().library?.games.some((x) => x.id === g.id || (g.appid && x.id === `steam:${g.appid}`));
  if (g.kind === "steam") return own ? "Starts it through Steam, where their invite lands" : "Not on this device: opens it in Steam";
  if (g.kind === "web") return g.join ? "Opens their game with the party invite" : "Opens the same game; group up in its party screen";
  return own ? "Starts the same game here" : "Not on this device";
}

/** The game this device is running, for a new party. */
export function currentGame(): GameRef | null {
  const app = useApp.getState();
  const g = app.library?.games.find((x) => x.id === app.running[0]);
  return g ? gameRef(g) : null;
}

function RequestPane({ f }: { f: Pending }) {
  return (
    <Panel className="fade flex flex-col gap-[1.4rem] px-[1.6rem] py-[1.5rem]">
      <PersonHead p={f}>
        <span className="text-muted">Wants to be your friend</span>
      </PersonHead>
      <div data-nav-group="request-actions" className="flex gap-[0.75rem]">
        <Button variant="primary" def onClick={act(() => F.graph("accept", f.address), `${F.label(f)} is now your friend`)}>
          Accept
        </Button>
        <Button onClick={act(() => F.graph("decline", f.address))}>Decline</Button>
        <Button variant="ghost" onClick={act(() => F.graph("block", f.address), `Blocked ${F.label(f)}`)}>
          Block
        </Button>
      </div>
      <p className="text-small text-muted">They will not be told if you decline or block.</p>
    </Panel>
  );
}

function SentPane({ f }: { f: Pending }) {
  return (
    <Panel className="fade flex flex-col gap-[1.4rem] px-[1.6rem] py-[1.5rem]">
      <PersonHead p={f}>
        <span className="text-muted">Request sent {new Date(f.at).toLocaleDateString([], { month: "short", day: "numeric" })}</span>
      </PersonHead>
      <div className="flex gap-[0.75rem]">
        <Button def onClick={act(() => F.graph("cancel", f.address), "Request cancelled")}>
          Cancel request
        </Button>
      </div>
    </Panel>
  );
}

function Blocked({ list }: { list: Pending[] }) {
  return (
    <Panel className="fade">
      <div className="px-[1.4rem] pb-[0.4rem] pt-[1.2rem] text-small text-muted">Blocked players cannot send you requests or invites, and never see your status.</div>
      {list.map((f) => (
        <div key={f.address} className="flex min-h-[4.25rem] items-center gap-[1rem] border-t border-hair px-[1.4rem] py-[0.6rem]">
          <FriendAvatar p={f} />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-body font-medium">{F.label(f)}</span>
            <span className="num block truncate text-meta text-muted">{shortAddress(f.address)}</span>
          </span>
          <Button size="sm" onClick={act(() => F.graph("unblock", f.address), `Unblocked ${F.label(f)}`)}>
            Unblock
          </Button>
        </div>
      ))}
    </Panel>
  );
}

function Privacy() {
  const me = useFriends((s) => s.me);
  if (!me) return null;
  const p = me.settings;
  const save = (v: Partial<typeof p>) => void F.savePrivacy(v).catch(toastError);
  return (
    <div className="fade flex flex-col gap-[1.25rem]">
      <Panel>
        <SwitchRow title="Show what I'm playing" body="Friends see the game you are in. Off: they only see that you are online." on={p.showPlaying} onChange={(v) => save({ showPlaying: v })} />
        <SwitchRow title="Appear offline" body="Friends see you as offline. You still see them, and get their invites." on={p.appearOffline} onChange={(v) => save({ appearOffline: v })} />
        <SwitchRow
          title="Show my status to everyone"
          body="Off: only friends see your status. On: anyone can look up your name, picture and status on rare.market."
          on={p.visibility === "everyone"}
          onChange={(v) => save({ visibility: v ? "everyone" : "friends" })}
        />
      </Panel>
      <Panel className="flex items-center gap-[1.25rem] px-[1.4rem] py-[1rem]">
        <span className="min-w-0 flex-1">
          <span className="block text-body font-semibold">Friends on this device</span>
          <span className="block text-small text-muted">Turning off signs this device out. Your friends and requests stay on rare.market.</span>
        </span>
        <Button onClick={() => void F.turnOff()}>Turn off</Button>
      </Panel>
    </div>
  );
}

function NewParty({ openPicker, select }: { openPicker: (p: Picker) => void; select: (s: Sel) => void }) {
  const friends = useFriends((s) => s.friends);
  const g = currentGame();
  return (
    <Panel className="fade flex flex-col gap-[1rem] px-[1.6rem] py-[1.5rem]">
      <div className="text-row font-semibold">Start a party</div>
      <p className="max-w-[38rem] text-small text-muted">
        Invite friends, pick a game and chat. The party shows on everyone's Home, and Join opens the same game for each of them. Chat is kept for 24 hours.
      </p>
      <div className="flex gap-[0.75rem]">
        <Button variant="primary" def disabled={!friends.length} onClick={() => openPicker({ kind: "invite", partyId: null })}>
          Invite friends
        </Button>
        <Button
          onClick={act(async () => {
            const p = await F.createParty(g, []);
            select({ kind: "party", id: p.id });
          })}
        >
          Start without invites
        </Button>
      </div>
      {g && <p className="text-meta text-muted">The party starts with {g.title}, the game you are playing.</p>}
    </Panel>
  );
}

// ---------- pickers ----------

function Pickers({ picker, done }: { picker: NonNullable<Picker>; done: () => void }) {
  if (picker.kind === "invite") return <InvitePicker partyId={picker.partyId} done={done} />;
  if (picker.kind === "game") return <GamePicker partyId={picker.partyId} done={done} />;
  return <ItemPicker friend={picker.friend} />;
}

function PickerHead({ title, note }: { title: string; note?: string }) {
  return (
    <div className="mb-[0.9rem] flex items-baseline gap-[0.75rem]">
      <h2 className="text-row font-semibold">{title}</h2>
      {note && <span className="text-small text-muted">{note}</span>}
    </div>
  );
}

function InvitePicker({ partyId, done }: { partyId: string | null; done: () => void }) {
  const s = useFriends();
  const party = partyId ? s.parties.find((p) => p.id === partyId) : null;
  const inParty = new Set(party?.members.map((m) => m.address) ?? []);
  const list = s.friends.filter((f) => !inParty.has(f.address));
  const [picked, setPicked] = useState<string[]>([]);
  const room = 7 - (party ? party.members.length - 1 : 0);
  const go = async () => {
    if (partyId) await F.inviteToParty(partyId, picked);
    else await F.createParty(currentGame(), picked);
    useApp.getState().toast(picked.length === 1 ? "Invite sent" : `${picked.length} invites sent`);
    done();
  };
  return (
    <div className="fade">
      <PickerHead title="Invite friends" note={`Up to ${room} more`} />
      <Panel>
        {list.map((f) => {
          const on = picked.includes(f.address);
          return (
            <button
              key={f.address}
              data-nav
              data-hint-a={on ? "Remove" : "Add"}
              onClick={() => {
                feedback("toggle");
                setPicked(on ? picked.filter((a) => a !== f.address) : picked.length < room ? [...picked, f.address] : picked);
              }}
              className="flex min-h-[4rem] w-full items-center gap-[1rem] border-t border-hair px-[1.4rem] py-[0.5rem] text-left first:border-t-0"
            >
              <FriendAvatar p={f} />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-body font-medium">{F.label(f)}</span>
                <PresenceLine p={f.presence} className="text-meta" />
              </span>
              <span className={`flex size-[1.5rem] items-center justify-center rounded-chip ${on ? "bg-fg text-bg" : "border border-border-light"}`}>{on && <Check size="1rem" />}</span>
            </button>
          );
        })}
        {!list.length && <div className="px-[1.4rem] py-[1.2rem] text-small text-muted">Every friend is already in this party.</div>}
      </Panel>
      <div className="mt-[1rem] flex gap-[0.75rem]">
        <Button variant="primary" disabled={!picked.length} onClick={act(go)}>
          {picked.length > 1 ? `Invite ${picked.length} friends` : "Invite"}
        </Button>
        <Button variant="ghost" onClick={done}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

function GamePicker({ partyId, done }: { partyId: string; done: () => void }) {
  const library = useApp((s) => s.library);
  const games = useMemo(
    () =>
      (library?.games ?? [])
        .filter((g) => g.source !== "apps" && g.installed)
        .sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0))
        .slice(0, 18),
    [library],
  );
  return (
    <div className="fade">
      <PickerHead title="Pick the party's game" note="Join opens it for everyone" />
      <div data-nav-group="party-games" className="grid grid-cols-6 gap-[1rem]">
        {games.map((g) => (
          <div key={g.id} className="min-w-0">
            <button
              data-nav
              data-hint-a="Pick"
              onClick={act(async () => {
                await F.setPartyGame(partyId, gameRef(g, partyId));
                done();
              })}
              className="tile relative block aspect-[2/3] w-full overflow-hidden rounded-card bg-l3"
            >
              <GameArt game={gameRef(g)} className="absolute inset-0 h-full w-full object-cover" />
            </button>
            <div className="focus-text mt-[0.6rem] truncate text-meta font-medium text-fg/85">{g.title}</div>
          </div>
        ))}
      </div>
      {!games.length && <Empty title="No games installed yet" />}
    </div>
  );
}

function ItemPicker({ friend }: { friend: Person }) {
  const me = useFriends((s) => s.address);
  const [items, setItems] = useState<ItemCard[] | null>(null);
  useEffect(() => {
    if (!me) return;
    void rare
      .items(me, 48)
      .then((r) => setItems(r.data))
      .catch(() => setItems([]));
  }, [me]);
  return (
    <div className="fade relative">
      <LoadBar on={!items} />
      <PickerHead title={`Send an item to ${F.label(friend)}`} note="Rare Wallet shows the review before anything is sent" />
      {items && !items.length && <Empty title="No collectibles in this wallet" />}
      <div data-nav-group="send-items" className="grid grid-cols-6 gap-[1rem]">
        {(items ?? []).map((it) => (
          <div key={it.id} className="min-w-0">
            <button data-nav data-hint-a="Send" onClick={() => sendItem(friend, it.id)} className="tile relative block aspect-square w-full overflow-hidden rounded-card border border-hair bg-l3">
              <MImg src={it.imageUrl} label={it.name} width={256} className="absolute inset-0 h-full w-full" />
            </button>
            <div className="focus-text mt-[0.6rem] truncate text-meta font-medium text-fg/85">{it.name}</div>
            {it.collection && <div className="truncate text-label text-muted">{it.collection.name}</div>}
          </div>
        ))}
      </div>
    </div>
  );
}

/** Asks for a message with the on-screen keyboard and sends it to the party. */
export async function writeMessage(partyId: string) {
  const t = await askText("Message to your party", "", { placeholder: "Up to 280 characters" });
  if (!t?.trim()) return;
  await F.sendMessage(partyId, t.trim()).catch(toastError);
}

/** Add by username, SuiNS name or address: the keyboard shows who it is as you type, then the request goes out. */
export async function addByName(): Promise<void> {
  const raw = await askText("Add a friend", "", {
    placeholder: "Username, name.sui or 0x...",
    status: async (v) => {
      const r = await resolveTarget(v).catch(() => ({ ok: false as const, msg: "Could not look that up. Try again." }));
      if (!r) return null;
      if (!r.ok) return { text: r.msg, ok: false };
      return { text: r.secondary ? `${r.label}, ${r.secondary}` : r.label, ok: true };
    },
  });
  if (!raw?.trim()) return;
  const r = await resolveTarget(raw).catch(() => null);
  if (!r || !r.ok) {
    useApp.getState().toast(r && !r.ok ? r.msg : "Could not find that player", "error");
    return;
  }
  if (r.address === useFriends.getState().address) {
    useApp.getState().toast("That is your own account", "error");
    return;
  }
  try {
    await F.graph("request", r.address);
    const now = useFriends.getState();
    useApp.getState().toast(now.friends.some((f) => f.address === r.address) ? `${r.label} is now your friend` : `Request sent to ${r.label}`);
  } catch (e) {
    toastError(e);
  }
}
