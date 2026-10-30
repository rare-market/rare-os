// Settings, Emulators: what is installed, the newest official release, and a one-press update.
// Nothing downloads until the user presses Update.

import { useEffect, useState } from "react";
import { native } from "../lib/bridge";
import { useApp } from "../lib/store";
import type { EmuInfo, EmuUpdateInfo } from "../lib/types";
import { Button, Panel } from "../ui";
import { SettingRow } from "./settings/rows";
import { failed } from "../lib/fail";

const POWER = { quiet: "Quiet", balanced: "Balanced", performance: "Performance", custom: "Custom" } as const;

function date(secs: number | null) {
  return secs ? new Date(secs * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" }) : "";
}

export function EmulatorsPanel() {
  const toast = useApp((s) => s.toast);
  const [list, setList] = useState<EmuInfo[] | null>(null);
  const [found, setFound] = useState<Record<string, EmuUpdateInfo>>({});
  const [busy, setBusy] = useState<string | null>(null);

  const reload = () => void native.emus().then(setList).catch(() => setList([]));
  useEffect(reload, []);

  const check = async (e: EmuInfo) => {
    setBusy(e.id);
    try {
      const u = await native.emuCheck(e.id);
      setFound((f) => ({ ...f, [e.id]: u }));
      if (!u.available) toast(`${e.name} is up to date`);
    } catch (x) {
      toast(failed("Couldn't check for updates.", x, "Check your Wi-Fi and try again."), "error");
    }
    setBusy(null);
  };
  const update = async (e: EmuInfo) => {
    setBusy(e.id);
    try {
      const d = await native.emuUpdate(e.id);
      toast(`${e.name} ${d.tag} installed. The old version is kept as .bak`);
      setFound((f) => ({ ...f, [e.id]: { ...f[e.id], available: false } }));
      reload();
    } catch (x) {
      toast(failed("Couldn't update the emulator.", x), "error");
    }
    setBusy(null);
  };

  const installed = (list ?? []).filter((e) => e.path);
  if (list && installed.length === 0) {
    return (
      <Panel className="fade">
        <SettingRow title="No emulators found" body="Put emulator AppImages in the Applications folder in your home folder and they show up here." />
      </Panel>
    );
  }
  return (
    <Panel className="fade">
      {installed.map((e) => {
        const u = found[e.id];
        return (
          <SettingRow
            key={e.id}
            title={e.name}
            body={
              u?.available
                ? `${u.tag} is out${u.verify === "checksum" ? ", checksum published" : ", no checksum published"}. Installed ${date(e.modified)}.`
                : `Installed ${date(e.modified)}${e.tag ? `, ${e.tag}` : ""}. Starts on ${POWER[e.power]} power.`
            }
          >
            {u?.available ? (
              <Button variant="primary" disabled={busy !== null} onClick={() => void update(e)}>
                {busy === e.id ? "Updating" : "Update"}
              </Button>
            ) : (
              <Button disabled={busy !== null} onClick={() => void check(e)}>
                {busy === e.id ? "Checking" : "Check for update"}
              </Button>
            )}
          </SettingRow>
        );
      })}
    </Panel>
  );
}
