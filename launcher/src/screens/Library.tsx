import { useEffect, useState } from "react";
import { IconFolder, IconPlus } from "../components/icons";
import { askText } from "../components/Osk";
import { addWebGame, normaliseUrl } from "../lib/webgames";
import { Button, Empty, GameTile } from "../components/ui";
import { Glyph, Tabs } from "../ui";
import { native } from "../lib/bridge";
import { useApp } from "../lib/store";
import { SOURCE_LABEL, prettyPath, type Game, type Source } from "../lib/types";
import { usePadHandler } from "../nav/input";
import { useHints } from "../nav/hints";
import { openOptionsSheet } from "../components/Options";
import { feedback } from "../nav/feedback";
import { APPS, AppsGrid } from "../apps";
import { StreamGrid } from "../stream/tiles";
import { useStream } from "../stream/state";
import { pcGames } from "../stream/logic";
import { useOwnedStoreGames } from "../store";
import { failed } from "../lib/fail";

// "media" is the Apps row (Telegram, Netflix, Chrome...); "apps" are emulators; "stream" is the
// games on a paired PC (src/stream).
type Filter = Source | "all" | "media" | "stream";
type Sort = "recent" | "name" | "played";
const SORTS: Array<{ value: Sort; label: string }> = [
  { value: "recent", label: "Recently played" },
  { value: "name", label: "Name" },
  { value: "played", label: "Most played" },
];
const sortGames = (games: Game[], sort: Sort): Game[] =>
  sort === "name"
    ? [...games].sort((a, b) => a.title.localeCompare(b.title))
    : sort === "played"
      ? [...games].sort((a, b) => (b.playtimeMinutes ?? 0) - (a.playtimeMinutes ?? 0))
      : [...games].sort((a, b) => (b.lastPlayed ?? 0) - (a.lastPlayed ?? 0) || a.title.localeCompare(b.title));
const FILTERS: Filter[] = ["all", "stream", "media", "rare", "steam", "epic", "gog", "itch", "local", "web", "apps"];
const label = (f: Filter) => (f === "all" ? "All" : f === "media" ? "Apps" : f === "stream" ? "Streaming" : SOURCE_LABEL[f]);

// Another screen asks for a filter before it switches to Library (PC streaming setup: "Show games").
let requested: Filter | null = null;
export function requestLibraryFilter(f: Filter) {
  requested = f;
}
function initialFilter(): Filter {
  const r = requested;
  requested = null;
  if (r) return r;
  const q = new URLSearchParams(location.search).get("filter");
  return q === "apps" ? "media" : q === "stream" ? "stream" : "all";
}

export function Library() {
  const [focused, setFocused] = useState<string | null>(null);
  const library = useApp((s) => s.library);
  const scanning = useApp((s) => s.scanning);
  const scan = useApp((s) => s.scan);
  const toast = useApp((s) => s.toast);
  const [filter, setFilter] = useState<Filter>(initialFilter);
  const streamCount = useStream((s) => pcGames(s.hosts, s.apps).length);
  // Rare Store licences that aren't installed here sit next to the scanned games.
  const owned = useOwnedStoreGames(library?.games ?? []);
  const all = [...(library?.games ?? []), ...owned];
  // null once Steam has answered that nobody is signed in; undefined while it has not answered.
  const [steamUser, setSteamUser] = useState<string | null | undefined>(undefined);
  useEffect(() => {
    void native.steamAccount().then(setSteamUser).catch(() => setSteamUser(undefined));
  }, [scanning]);
  const [sort, setSort] = useState<Sort>("recent");
  const games = sortGames(all.filter((g) => filter === "all" || g.source === filter), sort);
  const count = (s: Filter) => (s === "media" ? APPS.length : s === "stream" ? streamCount : all.filter((g) => s === "all" || g.source === s).length);
  const foreign = library?.foreignSteamLibraries ?? [];

  // LT/RT cycle the source filter (the screen's sections; LB/RB always stay the launcher's tabs).
  // Only filters that have games, so X never lands on an empty page.
  // Streaming always shows: it is where a PC gets set up.
  const shown = FILTERS.filter((f) => f === "all" || f === "stream" || f === filter || count(f) > 0);
  const overlay = useApp((s) => s.overlay);
  usePadHandler((b) => {
    // View: sort and filter, in one list.
    if (b === "select" && !useApp.getState().overlay) {
      openOptionsSheet({
        title: "Library view",
        sub: "Sort and filter",
        items: [
          ...SORTS.map((o) => ({ label: `Sort: ${o.label}`, on: sort === o.value, onSelect: () => setSort(o.value) })),
          ...shown.map((f) => ({ label: `Show: ${label(f)}`, on: filter === f, onSelect: () => setFilter(f) })),
        ],
      });
      return true;
    }
    // X belongs to an open page or sheet, not the filter under it.
    if ((b !== "lt" && b !== "rt") || useApp.getState().overlay) return false;
    const i = shown.indexOf(filter);
    setFilter(shown[(i + (b === "rt" ? 1 : shown.length - 1)) % shown.length]);
    feedback("move");
    return true;
  });
  useHints([{ b: "ltrt", label: "Sections" }, { b: "select", label: "View" }], !overlay);

  return (
    <div data-scroll className="h-full overflow-y-auto px-[2.5rem] pb-[2.5rem]">
      <div data-sticky className="sticky top-0 z-10 -mx-[2.5rem] mb-[1rem] flex h-[3.5rem] items-stretch gap-[1.1rem] border-b border-hair bg-bg px-[2.5rem]">
        <span className="flex items-center">
          <Glyph b="lt" size="sm" />
        </span>
        <Tabs items={shown.map((f) => ({ value: f, label: label(f), count: count(f) }))} value={filter} className="h-full" />
        <span className="flex items-center">
          <Glyph b="rt" size="sm" />
        </span>
        <span className="ml-auto flex items-center">
          {filter === "stream" ? (
            <button
              data-nav
              onClick={() => useApp.getState().open({ kind: "stream" })}
              data-hint-a="PC streaming"
              className="btn flex h-[2.25rem] items-center gap-[0.4rem] rounded-chip bg-btn-2 px-[0.9rem] text-small font-medium"
            >
              <IconPlus size="1rem" /> PC
            </button>
          ) : (
          <button
            data-nav
            onClick={async () => {
              const url = await askText("Web game address", "", { placeholder: "https://game.example" });
              if (!url) return;
              let host: string;
              try {
                host = new URL(normaliseUrl(url)).hostname.replace(/^www\./, "");
              } catch (e) {
                toast(failed("That isn't a web address. Check it and try again.", e, ""), "error");
                return;
              }
              const title = await askText("Name for this game", host);
              if (!title) return;
              try {
                await addWebGame({ title, url });
                toast(`${title} added`);
                setFilter("web");
              } catch (e) {
                toast(failed("Couldn't add the web game.", e), "error");
              }
            }}
            data-hint-a="Add web game"
            className="btn flex h-[2.25rem] items-center gap-[0.4rem] whitespace-nowrap rounded-chip bg-btn-2 px-[0.9rem] text-small font-medium"
          >
            <IconPlus size="1rem" /> Web game
          </button>
          )}
        </span>
      </div>
      {/* The focused game's name over the grid, which is art only, and the count. */}
      <div className="mb-[0.75rem] flex h-[1.75rem] items-center gap-[0.75rem]">
        <span className="min-w-0 truncate text-row font-semibold">{filter !== "media" && filter !== "stream" ? focused ?? "" : ""}</span>
        <span className="ml-auto shrink-0 text-small text-muted">
          {filter === "media" ? `${APPS.length} apps` : filter === "stream" ? (streamCount ? `${streamCount} on your PC` : "") : scanning ? "Scanning" : `${games.length} ${games.length === 1 ? "game" : "games"}`}
        </span>
      </div>

      {foreign.length > 0 && filter !== "media" && filter !== "stream" && (
        <div className="mb-[1.5rem] flex items-center gap-[1rem] rounded-panel bg-l1 px-[1.25rem] py-[1rem]">
          <IconFolder size="1.4rem" className="shrink-0 text-icon-2" />
          <div className="min-w-0 flex-1">
            <div className="text-body font-semibold">Games found from your previous system</div>
            <div className="truncate text-small text-muted">
              {library?.games.filter((g) => g.foreign).length} games in {foreign.map(prettyPath).join(", ")}. They play from here; add the folder to Steam so Steam keeps them updated.
            </div>
          </div>
          <Button
            onClick={async () => {
              try {
                for (const f of foreign) await native.steamAddLibrary(f);
                toast("Added to Steam. Steam picks them up next time it starts.");
                await scan();
              } catch (e) {
                toast(failed("Couldn't add the folder to Steam.", e), "error");
              }
            }}
          >
            Add to Steam
          </Button>
        </div>
      )}

      {filter === "media" ? (
        <AppsGrid />
      ) : filter === "stream" ? (
        <StreamGrid />
      ) : games.length === 0 && !scanning ? (
        filter === "steam" && steamUser === null ? (
          <Empty title="Sign in to Steam to see your games.">
            <Button variant="primary" def onClick={() => void native.steamBigPicture()}>Sign in</Button>
          </Empty>
        ) : (
          <Empty title="No games here yet" body="Install a game in Steam, or put a game folder in Games on this device or your SD card.">
            <Button variant="primary" def onClick={() => void scan()}>Scan again</Button>
          </Empty>
        )
      ) : (
        <div data-nav-group={`library-${filter}`} onFocus={(e) => setFocused((e.target as HTMLElement).dataset.title ?? null)} className="grid grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-[1rem]">
          {games.map((g, i) => (
            <GameTile key={g.id} game={g} def={i === 0} bare onSelect={g.source === "rare" ? () => useApp.getState().open({ kind: "storegame", slug: g.id.slice(5) }) : undefined} />
          ))}
        </div>
      )}
    </div>
  );
}
