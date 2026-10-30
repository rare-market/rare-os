// Accounts (username, Rare account, wallet), Storage (games, folders, Steam, cover art, clips)
// and About.

import { useEffect } from "react";
import { BrandMark } from "../../components/Chrome";
import { askText } from "../../components/Osk";
import { feedback } from "../../nav/feedback";
import { useApp } from "../../lib/store";
import { prettyPath } from "../../lib/types";
import { pickUsername } from "../../friends/ui/pickUsername";
import { useProfile } from "../../sui/profile";
import { Button, Panel, StatLine } from "../../ui";
import { SettingRow } from "./rows";
import type { SectionProps } from "./config";

/** The Rare username: free, unique, picked here. SuiNS is a second line when the account owns one. */
function UsernameRow() {
  const prof = useProfile();
  useEffect(() => {
    void prof.refresh();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps
  if (!prof.address) return null;
  return (
    <SettingRow
      id="username"
      title="Username"
      body={prof.username ? `${prof.username}${prof.name ? `, SuiNS ${prof.name}` : ""}. You can change it once every 30 days.` : "Friends find you by it. Free, and nobody needs a SuiNS name."}
    >
      <Button variant={prof.username ? "secondary" : "primary"} onClick={() => void pickUsername()}>
        {prof.username ? "Change" : "Pick a username"}
      </Button>
    </SettingRow>
  );
}

export function AccountsSection() {
  const open = useApp((s) => s.open);
  return (
    <Panel className="fade">
      <UsernameRow />
      <SettingRow id="rare-account" title="Rare account" body="One sign-in for Rare Wallet and Rare Marketplace. Your games work without an account.">
        <Button variant="primary" onClick={() => open({ kind: "signin" })}>
          Sign in
        </Button>
      </SettingRow>
      <SettingRow id="wallet" title="Rare Wallet" body="Balances, collectibles and moving assets from your old SuiPlay wallet.">
        <Button onClick={() => useApp.getState().setTab("wallet")}>Open</Button>
      </SettingRow>
      <SettingRow id="setup" title="Setup" body="Go through the first-run steps again: passkey, moving assets, Steam and favourites.">
        <Button onClick={() => open({ kind: "firstrun" })}>Run setup</Button>
      </SettingRow>
    </Panel>
  );
}

const STEAM_START = {
  focus: { label: "When I pick a game", body: "Steam starts quietly when you open a Steam game, so Play starts fast. Uses nothing until then." },
  boot: { label: "After startup", body: "Steam starts quietly a little after the launcher. Steam games start fastest. Keeps Steam running, which costs some battery." },
  demand: { label: "On Play", body: "Steam starts only when you press Play. Uses nothing, and the first game takes a few seconds longer." },
} as const;

export function StorageSection({ cfg, patch }: SectionProps) {
  const library = useApp((s) => s.library);
  const scan = useApp((s) => s.scan);
  const toast = useApp((s) => s.toast);
  const open = useApp((s) => s.open);
  return (
    <div className="fade flex flex-col gap-[1.25rem]">
      <Panel className="px-[1.4rem] py-[1.2rem]">
        <div className="mb-[0.9rem] text-body font-semibold">Sources</div>
        <StatLine items={(library?.sources ?? []).map((s) => ({ key: s.source, label: s.source === "gog" ? "GOG" : s.source[0].toUpperCase() + s.source.slice(1), value: String(s.count) }))} size="lg" />
      </Panel>
      <Panel>
        <SettingRow id="scan" title="Scan for games" body="Steam, Epic, GOG, itch.io and your Games folders.">
          <Button onClick={() => void scan().then(() => toast("Library updated"))}>Scan again</Button>
        </SettingRow>
        <SettingRow id="games-folder" title="Games folder" body="A folder with one game per subfolder, on this device or an SD card.">
          <Button
            onClick={async () => {
              const f = await askText("Folder with one game per subfolder", "", { placeholder: "/run/media/sd/Games" });
              if (f) await patch({ gameFolders: [...cfg.gameFolders.filter((x) => x !== f), f] }).then(() => scan());
            }}
          >
            Add folder
          </Button>
        </SettingRow>
        {cfg.gameFolders.map((f) => (
          <SettingRow key={f} title={prettyPath(f)} body="Scanned for games.">
            <Button
              onClick={async () => {
                feedback("toggle");
                await patch({ gameFolders: cfg.gameFolders.filter((x) => x !== f) }).then(() => scan());
              }}
            >
              Remove
            </Button>
          </SettingRow>
        ))}
        <SettingRow id="steam-start" title="When Steam starts" body={STEAM_START[cfg.steamStart ?? "focus"].body}>
          <Button
            variant="secondary"
            onClick={() => {
              feedback("toggle");
              const order = ["focus", "boot", "demand"] as const;
              const cur = cfg.steamStart ?? "focus";
              void patch({ steamStart: order[(order.indexOf(cur) + 1) % order.length] });
            }}
          >
            {STEAM_START[cfg.steamStart ?? "focus"].label}
          </Button>
        </SettingRow>
        <SettingRow id="steamgriddb" title="SteamGridDB cover art" body={cfg.steamgriddbKey ? "API key added" : "Optional. A free API key adds covers for games outside Steam."}>
          <Button
            onClick={async () => {
              const k = await askText("SteamGridDB API key", cfg.steamgriddbKey ?? "", { secret: true });
              if (k !== null) await patch({ steamgriddbKey: k || null });
            }}
          >
            {cfg.steamgriddbKey ? "Change key" : "Add key"}
          </Button>
        </SettingRow>
        <SettingRow id="share" title="Screenshots and clips" body="Take a screenshot or keep the last 30 seconds, and share them as a link on rare.market.">
          <Button onClick={() => open({ kind: "share" })}>Open Share</Button>
        </SettingRow>
      </Panel>
    </div>
  );
}

export function AboutSection({ goLicences }: { goLicences: () => void }) {
  const device = useApp((s) => s.device);
  return (
    <div className="fade flex flex-col gap-[1.25rem]">
      <Panel className="px-[1.4rem] py-[1.2rem]">
        <StatLine
          size="lg"
          items={[
            { label: "Device", value: device?.name ?? "" },
            { label: "System", value: `${device?.osName ?? "Rare OS"} ${device?.osVersion ?? ""}`.trim() },
            { label: "Hostname", value: device?.hostname ?? "" },
            { label: "Display", value: device?.underGamescope ? "Game mode" : "Window" },
          ]}
        />
      </Panel>
      <Panel className="flex flex-col items-start gap-[1rem] px-[1.6rem] py-[1.5rem]">
        <div className="flex items-center gap-[0.9rem]">
          <BrandMark size="3rem" />
          <div>
            <div className="text-title font-semibold">Rare OS</div>
            <div className="num text-meta text-muted">{device?.osVersion || "0.1"}</div>
          </div>
        </div>
        <p className="max-w-[40rem] text-body leading-relaxed text-fg/80">
          A community gaming OS for PC handhelds, made for the SuiPlay0X1 first. Your Steam, Epic, GOG and local games in one place, with Rare Wallet and Rare Marketplace built in.
        </p>
        <Button onClick={goLicences}>Credits and licences</Button>
      </Panel>
    </div>
  );
}
