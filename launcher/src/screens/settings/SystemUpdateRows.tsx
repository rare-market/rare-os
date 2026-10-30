// Settings > System: version, updates, restore the previous system, boot menu next start.
// One panel; rows are the Settings rows. The sheets live in OsSheets.tsx.

import { useEffect, useState } from "react";
import { native } from "../../lib/bridge";
import { updateLine, useOsUpdate } from "../../lib/osupdate";
import { useApp } from "../../lib/store";
import { Button, Panel } from "../../ui";
import { SettingRow } from "./rows";
import { RestoreSheet } from "./OsSheets";
import { failed } from "../../lib/fail";

export function SystemUpdateRows() {
  const device = useApp((s) => s.device);
  const toast = useApp((s) => s.toast);
  const update = useOsUpdate((s) => s.update);
  const checking = useOsUpdate((s) => s.checking);
  const [canRestore, setCanRestore] = useState(false);
  const [menuSet, setMenuSet] = useState(false);
  useEffect(() => {
    void useOsUpdate.getState().refresh();
    void native.osRestoreAvailable().then(setCanRestore).catch(() => setCanRestore(false));
  }, []);
  const line = updateLine(update, checking, Date.now());
  const version = update.bootedVersion ?? device?.osVersion ?? "";
  return (
    <>
      <Panel>
        <SettingRow title={`${device?.osName ?? "Rare OS"} ${version}`.trim()} body="The version running now." />
        <SettingRow id="updates" title={line.title} body={line.body}>
          {update.staged ? (
            <Button variant="primary" hint="Restart to update" onClick={() => useOsUpdate.setState({ restartOpen: true })}>
              Restart now
            </Button>
          ) : (
            <Button
              hint="Check for updates"
              disabled={checking}
              onClick={() =>
                void useOsUpdate
                  .getState()
                  .check()
                  .catch((e) => toast(failed("Couldn't check for updates.", e, "Check your Wi-Fi and try again."), "error"))
              }
            >
              {checking ? "Checking" : "Update now"}
            </Button>
          )}
        </SettingRow>
        {canRestore && (
          <SettingRow id="restore-system" title="Restore previous system" body="Start the system this device ran before Rare OS. Rare OS stays installed and your files and games are shared.">
            <Button hint="Restore previous system" onClick={() => useOsUpdate.setState({ restoreOpen: true })}>
              Restore
            </Button>
          </SettingRow>
        )}
        <SettingRow id="boot-menu" title="Boot menu" body={menuSet ? "The boot menu shows on the next start, once. Pick an older version there if an update misbehaves." : "Show the boot menu on the next start, once, to pick an older version or another system."}>
          <Button
            hint="Boot menu next start"
            disabled={menuSet}
            onClick={() =>
              void native
                .osBootMenuNext()
                .then(() => {
                  setMenuSet(true);
                  toast("The boot menu shows on the next start");
                })
                .catch((e) => toast(failed("Couldn't set the boot menu.", e), "error"))
            }
          >
            {menuSet ? "Set for next start" : "Show next start"}
          </Button>
        </SettingRow>
      </Panel>
      <RestoreSheet />
    </>
  );
}
