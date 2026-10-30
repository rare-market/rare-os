// A party: its game (Join opens it here), the members with their presence, and the text chat
// (short messages over the notify stream, kept 24 hours). An invite shows Join party and Decline.

import { useEffect, useRef } from "react";
import { useApp } from "../../lib/store";
import { Button, Panel } from "../../ui";
import { joinGame } from "../join";
import * as F from "../store";
import { useFriends } from "../store";
import type { Party } from "../types";
import { FriendAvatar, GameArt, PresenceLine } from "./bits";
import { toastError, writeMessage, type Picker } from "./FriendsScreen";

const time = (t: number) => new Date(t).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

export function PartyPane({ party, openPicker }: { party: Party; openPicker: (p: Picker) => void }) {
  const me = useFriends((s) => s.address);
  const messages = useFriends((s) => s.messages[party.id]);
  const running = useApp((s) => s.running);
  const mine = party.members.find((m) => m.address === me);
  const invited = mine?.state === "invited";
  const host = party.members.find((m) => m.address === party.host);
  const isHost = party.host === me;
  const g = party.game;
  const playingIt = !!g && running.includes(g.id);
  const chat = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (invited) return;
    void F.loadMessages(party.id);
    F.openChat(party.id);
    return () => F.openChat(null);
  }, [party.id, invited]);
  useEffect(() => {
    chat.current?.scrollTo({ top: chat.current.scrollHeight });
  }, [messages?.length]);

  const run = (fn: () => Promise<unknown>, done?: string) => () =>
    void fn()
      .then(() => done && useApp.getState().toast(done))
      .catch(toastError);

  return (
    <div className="fade flex flex-col gap-[1.25rem]">
      <Panel className="flex flex-col gap-[1.3rem] px-[1.6rem] py-[1.5rem]">
        <div className="flex items-center gap-[1.25rem]">
          {g ? (
            <div className="relative aspect-[2/3] w-[5.5rem] shrink-0 overflow-hidden rounded-card bg-l3">
              <GameArt game={g} className="absolute inset-0 h-full w-full object-cover" />
            </div>
          ) : null}
          <div className="min-w-0 flex-1">
            <div className="text-label text-muted">{invited ? `${F.label(host)} invited you` : isHost ? "Your party" : `${F.label(host)}'s party`}</div>
            <div className="truncate text-title font-semibold">{g ? g.title : "No game picked yet"}</div>
            <div className="text-meta text-muted">
              {party.members.filter((m) => m.state === "joined").length} in the party
              {party.members.some((m) => m.state === "invited") ? `, ${party.members.filter((m) => m.state === "invited").length} invited` : ""}
            </div>
          </div>
        </div>

        <div data-nav-group="party-actions" data-nav-remember className="flex flex-wrap gap-[0.75rem]">
          {invited ? (
            <>
              <Button variant="primary" def onClick={run(() => F.joinParty(party.id), "You joined the party")}>
                Join party
              </Button>
              <Button onClick={run(() => F.leaveParty(party.id))}>Decline</Button>
            </>
          ) : (
            <>
              {g && !playingIt && (
                <Button variant="primary" def onClick={() => void joinGame(g).catch(toastError)}>
                  Join game
                </Button>
              )}
              <Button def={!g || playingIt} onClick={() => void writeMessage(party.id)}>
                Write a message
              </Button>
              <Button onClick={() => openPicker({ kind: "invite", partyId: party.id })}>Invite friends</Button>
              {isHost && <Button onClick={() => openPicker({ kind: "game", partyId: party.id })}>{g ? "Change game" : "Pick a game"}</Button>}
              <Button variant="ghost" onClick={run(() => F.leaveParty(party.id), "You left the party")}>
                Leave party
              </Button>
            </>
          )}
        </div>

        <div className="flex flex-wrap gap-x-[1.75rem] gap-y-[0.9rem] border-t border-hair pt-[1.2rem]">
          {party.members.map((m) => (
            <div key={m.address} className="flex min-w-[13rem] items-center gap-[0.7rem]">
              <FriendAvatar p={m} size="2.25rem" />
              <span className="min-w-0">
                <span className="block truncate text-body font-medium">
                  {m.address === me ? "You" : F.label(m)}
                  {m.address === party.host && <span className="ml-[0.5rem] text-meta font-normal text-muted">Host</span>}
                </span>
                {m.state === "invited" ? <span className="block text-meta text-muted">Invited</span> : <PresenceLine p={m.presence} className="text-meta" />}
              </span>
            </div>
          ))}
        </div>
      </Panel>

      {!invited && (
        <Panel className="flex flex-col px-[1.6rem] py-[1.2rem]">
          <div className="mb-[0.6rem] flex items-baseline gap-[0.75rem]">
            <span className="text-body font-semibold">Party chat</span>
            <span className="text-meta text-muted">Kept for 24 hours</span>
          </div>
          <div ref={chat} className="flex max-h-[17rem] flex-col gap-[0.7rem] overflow-y-auto">
            {(messages ?? []).map((m) => {
              const who = party.members.find((x) => x.address === m.from) ?? F.personOf(m.from);
              return (
                <div key={m.id} className="flex items-start gap-[0.7rem]">
                  {who ? <FriendAvatar p={who} size="1.75rem" /> : <span className="size-[1.75rem] shrink-0" />}
                  <div className="min-w-0">
                    <div className="flex items-baseline gap-[0.5rem]">
                      <span className="text-small font-semibold">{m.from === me ? "You" : F.label(who)}</span>
                      <span className="text-label text-muted">{time(m.at)}</span>
                    </div>
                    <div className="break-words text-small text-fg/90">{m.text}</div>
                  </div>
                </div>
              );
            })}
            {messages && !messages.length && <div className="text-small text-muted">No messages yet.</div>}
          </div>
        </Panel>
      )}
    </div>
  );
}
