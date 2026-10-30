// Settings: a section list on the left (with a search field on top), rows on the right. Rows are
// rare.market's settings rows on one panel (a card never holds a card): title and a muted line,
// the control on the right. Switches flip with A. LT / RT move between sections (docs/CONTROLS.md).
//
// The rows live in screens/settings/*; every one carries an id that is listed in
// settings/index.ts, which is what search reads. SettingRow and SwitchRow are re-exported here
// because other screens import them from "./Settings".

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { askText } from "../components/Osk";
import { IconSearch } from "../components/icons";
import { native } from "../lib/bridge";
import { useApp } from "../lib/store";
import type { Config } from "../lib/types";
import { focusEl, focusFirst } from "../nav/focus";
import { feedback } from "../nav/feedback";
import { inControllerMode, usePadHandler } from "../nav/input";
import { NotificationSettings } from "../notify";
import { Panel } from "../ui";
import { CREDITS } from "./credits";
import { EmulatorsPanel } from "./Emulators";
import { AboutSection, AccountsSection, StorageSection } from "./settings/AccountsStorage";
import { patchConfig } from "./settings/config";
import { SETTINGS_INDEX } from "./settings/index";
import { NetworkSection } from "./settings/NetworkSection";
import { PowerSection } from "./settings/PowerSection";
import { useSettingsJump } from "./settings/jump";
import { SECTIONS, type SectionId } from "./settings/sections";
import { searchSettings, type SettingEntry } from "./settings/search";
import { DeveloperSection } from "./settings/DeveloperSection";
import { ControlsSection, DisplaySection, SoundSection, SystemSection } from "./settings/SystemSections";
import { failed } from "../lib/fail";

export { SettingRow, SwitchRow } from "./settings/rows";

/** Moves focus to a row (search results land here) and marks it for a moment. */
function revealRow(id: string, root: HTMLElement | null): boolean {
  const row = root?.querySelector<HTMLElement>(`[data-setting-id="${id}"]`);
  if (!row) return false;
  const target = row.hasAttribute("data-nav") ? row : row.querySelector<HTMLElement>("[data-nav]");
  if (target) focusEl(target);
  else row.scrollIntoView({ block: "center" });
  row.classList.add("bg-l2");
  setTimeout(() => row.classList.remove("bg-l2"), 1800);
  return true;
}

export function Settings() {
  const [section, setSection] = useState<SectionId>(() => useSettingsJump.getState().section ?? "system");
  const jumpSeq = useSettingsJump((s) => s.seq);
  useEffect(() => {
    const t = useSettingsJump.getState().section;
    if (t) {
      setSection(t);
      useSettingsJump.setState({ section: null });
    }
  }, [jumpSeq]);
  const [cfg, setCfg] = useState<Config | null>(null);
  const [q, setQ] = useState("");
  const [jump, setJump] = useState<string | null>(null);
  const toast = useApp((s) => s.toast);
  const pane = useRef<HTMLDivElement>(null);
  const nav = useRef<HTMLElement>(null);
  const field = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void native.config().then(setCfg);
  }, []);
  const patch = useCallback(
    async (p: Partial<Config>) => {
      setCfg((c) => (c ? { ...c, ...p } : c));
      try {
        setCfg(await patchConfig(p));
      } catch (e) {
        toast(failed("Couldn't save that setting.", e), "error");
      }
    },
    [toast],
  );

  const hits = useMemo(() => searchSettings(q, SETTINGS_INDEX), [q]);
  const searching = q.trim().length > 0;

  const go = (e: SettingEntry) => {
    feedback("select");
    setQ("");
    setSection(e.section);
    setJump(e.id === e.section ? null : e.id);
    if (e.id === e.section) setTimeout(() => pane.current && focusFirst(pane.current), 80);
  };

  // After a jump the section has to render (some load their values first): look for the row for a moment.
  useEffect(() => {
    if (!jump || searching) return;
    let tries = 0;
    const id = setInterval(() => {
      if (revealRow(jump, pane.current) || ++tries > 30) {
        clearInterval(id);
        setJump(null);
      }
    }, 60);
    return () => clearInterval(id);
  }, [jump, section, searching, cfg]);

  const openKeyboard = async () => {
    const v = await askText("Search settings", q, { placeholder: "Brightness, Wi-Fi, volume" });
    if (v !== null) setQ(v);
    setTimeout(() => field.current?.focus(), 30);
  };

  usePadHandler((b) => {
    const el = document.activeElement as HTMLElement | null;
    if (b === "y") {
      void openKeyboard();
      return true;
    }
    if (b === "b" && searching) {
      setQ("");
      return true;
    }
    if (el?.dataset.slider !== undefined && (b === "left" || b === "right")) {
      el.dispatchEvent(new CustomEvent("nudge", { detail: b === "left" ? -1 : 1 }));
      return true;
    }
    if ((b === "lt" || b === "rt") && !searching) {
      const i = SECTIONS.findIndex((s) => s.id === section);
      const next = SECTIONS[(i + (b === "rt" ? 1 : SECTIONS.length - 1)) % SECTIONS.length].id;
      feedback("move");
      setSection(next);
      if (nav.current?.contains(el)) setTimeout(() => focusEl(nav.current?.querySelector<HTMLElement>(`[data-section="${next}"]`)), 0);
      else setTimeout(() => pane.current && focusFirst(pane.current), 120);
      return true;
    }
    return false;
  });

  // A letter typed with a hardware keyboard starts a search.
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || e.ctrlKey || e.metaKey || e.altKey) return;
      if (/^[a-z0-9]$/i.test(e.key) && !"xyqes".includes(e.key.toLowerCase())) field.current?.focus();
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  return (
    <div className="flex h-full gap-[2rem] px-[2.5rem] pb-[1.5rem] pt-[0.5rem]">
      <nav ref={nav} data-nav-group="settings-nav" data-nav-remember data-scroll className="-ml-[1.5rem] flex w-[17.5rem] shrink-0 flex-col gap-[0.15rem] overflow-y-auto px-[0.5rem] pb-[0.5rem]">
        <h1 className="mb-[0.5rem] px-[1rem] text-title font-semibold">Settings</h1>
        <label className="mb-[0.5rem] flex h-[2.75rem] items-center gap-[0.6rem] rounded-card border border-border-light bg-l1 px-[0.85rem]">
          <IconSearch size="1.1rem" className="shrink-0 text-icon-2" />
          <input
            ref={field}
            data-nav
            data-hint-a="Search"
            value={q}
            placeholder="Search settings"
            onChange={(e) => setQ(e.target.value)}
            onClick={() => inControllerMode() && void openKeyboard()}
            onKeyDown={(e) => {
              if (e.key === "Enter" && hits[0]) go(hits[0].entries[0]);
              if (e.key === "Escape" && q) {
                e.stopPropagation();
                setQ("");
              }
            }}
            className="min-w-0 flex-1 bg-transparent text-body outline-none placeholder:text-muted"
          />
        </label>
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            data-nav
            data-section={s.id}
            data-nav-default={s.id === "system" ? "" : undefined}
            onFocus={() => !searching && setSection(s.id)}
            onClick={() => {
              setQ("");
              setSection(s.id);
            }}
            className={`relative rounded-card px-[1rem] py-[0.6rem] text-left text-body font-medium ${!searching && section === s.id ? "bg-l2 text-fg" : "text-fg/70"}`}
          >
            {!searching && section === s.id && <span className="absolute inset-y-[0.6rem] left-0 w-[3px] rounded-full bg-fg" />}
            {s.label}
          </button>
        ))}
      </nav>
      <div ref={pane} data-scroll className="min-w-0 flex-1 overflow-y-auto pb-[1rem]">
        {searching ? (
          hits.length === 0 ? (
            <p className="fade px-[0.5rem] text-body text-muted">No settings match "{q.trim()}".</p>
          ) : (
            <div className="fade flex flex-col gap-[1.25rem]" data-nav-group="settings-results">
              {hits.map((g) => (
                <div key={g.section}>
                  <div className="mb-[0.5rem] px-[0.25rem] text-label font-medium text-secondary">{g.label}</div>
                  <Panel>
                    {g.entries.map((e) => (
                      <button
                        key={e.id}
                        data-nav
                        data-hint-a="Go to setting"
                        onClick={() => go(e)}
                        className="flex min-h-[3.5rem] w-full items-center gap-[1rem] border-t border-hair px-[1.4rem] py-[0.7rem] text-left first:border-t-0"
                      >
                        <span className="min-w-0 flex-1 truncate text-body font-semibold">{e.title}</span>
                      </button>
                    ))}
                  </Panel>
                </div>
              ))}
            </div>
          )
        ) : (
          <SectionView section={section} cfg={cfg} patch={patch} setSection={setSection} />
        )}
      </div>
    </div>
  );
}

function SectionView({ section, cfg, patch, setSection }: { section: SectionId; cfg: Config | null; patch: (p: Partial<Config>) => Promise<void>; setSection: (s: SectionId) => void }) {
  if (section === "notifications") return <NotificationSettings />;
  if (section === "emulators") return <EmulatorsPanel />;
  if (section === "network") return <NetworkSection />;
  if (section === "display") return <DisplaySection />;
  if (section === "about") return <AboutSection goLicences={() => setSection("licences")} />;
  if (section === "accounts") return <AccountsSection />;
  if (section === "licences") return <Licences />;
  if (section === "developer") return cfg ? <DeveloperSection cfg={cfg} patch={patch} /> : null;
  if (!cfg) return null;
  const p = { cfg, patch };
  switch (section) {
    case "system":
      return <SystemSection {...p} />;
    case "sound":
      return <SoundSection {...p} />;
    case "controls":
      return <ControlsSection {...p} />;
    case "power":
      return <PowerSection {...p} />;
    case "storage":
      return <StorageSection {...p} />;
  }
  return null;
}

function Licences() {
  return (
    <div className="fade flex flex-col gap-[0.75rem]">
      <p className="text-small text-muted">Rare OS is built from our own code and these open-source projects. Thank you to everyone who made them.</p>
      <Panel>
        {CREDITS.map((c) => (
          <div key={c.name} data-nav tabIndex={0} className="border-t border-hair px-[1.4rem] py-[0.85rem] first:border-t-0 first:rounded-t-panel last:rounded-b-panel">
            <div className="flex items-baseline gap-[0.6rem]">
              <span className="text-body font-semibold">{c.name}</span>
              <span className="text-meta text-muted">{c.licence}</span>
            </div>
            <div className="text-small text-muted">{c.use}</div>
          </div>
        ))}
      </Panel>
    </div>
  );
}
