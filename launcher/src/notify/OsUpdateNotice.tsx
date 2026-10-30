// Launcher-level watcher for OS updates: polls the update state cheaply, and once per launch per
// staged version puts "Restart to update" in the notification centre (and a toast through the
// usual notice path; class System). The notice's action is Restart now: it
// opens the restart sheet in Settings' OsSheets. A on it opens the restart sheet in Settings' OsSheets. Renders the sheet
// when the notice is opened from anywhere, not only Settings.

import { useEffect } from "react";
import { RestartSheet } from "../screens/settings/OsSheets";
import { toAnnounce, useOsUpdate } from "../lib/osupdate";
import { notifyLocal } from "./stream";

const POLL_MS = 120_000;

export function OsUpdateNotice() {
  const update = useOsUpdate((s) => s.update);
  const announced = useOsUpdate((s) => s.announced);
  useEffect(() => {
    void useOsUpdate.getState().refresh();
    const id = setInterval(() => {
      if (!document.hidden) void useOsUpdate.getState().refresh();
    }, POLL_MS);
    return () => clearInterval(id);
  }, []);
  useEffect(() => {
    const key = toAnnounce(update, announced);
    if (!key) return;
    useOsUpdate.setState({ announced: key });
    notifyLocal({
      kind: "os_update",
      title: update.stagedVersion ? `Rare OS ${update.stagedVersion} is ready` : "A Rare OS update is ready",
      body: "It installs when you restart.",
      action: { kind: "os_update" },
    });
  }, [update, announced]);
  return <RestartSheet />;
}
