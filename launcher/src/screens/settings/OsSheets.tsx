// The two confirmations for OS changes, controller only: A confirms (the focused button), B
// cancels. Restart to update, and Restore previous system.

import { useState } from "react";
import { native } from "../../lib/bridge";
import { useOsUpdate } from "../../lib/osupdate";
import { useApp } from "../../lib/store";
import { usePadHandler } from "../../nav/input";
import { Sheet } from "../../trade/parts";
import { Button } from "../../ui";
import { failed } from "../../lib/fail";

function useBack(onClose: () => void) {
  usePadHandler((b) => {
    if (b === "b") {
      onClose();
      return true;
    }
    return false;
  });
}

/** "Restart to update": a clean restart installs the staged version. Later keeps it staged. */
export function RestartSheet() {
  const open = useOsUpdate((s) => s.restartOpen);
  const update = useOsUpdate((s) => s.update);
  const running = useApp((s) => s.running.length > 0);
  const toast = useApp((s) => s.toast);
  const [busy, setBusy] = useState(false);
  const close = () => useOsUpdate.setState({ restartOpen: false });
  useBack(close);
  if (!open) return null;
  return (
    <Sheet
      title={update.staged ? "Restart to update" : "Nothing to install"}
      sub={update.staged ? (update.stagedVersion ? `Version ${update.stagedVersion}` : undefined) : "Rare OS is up to date."}
      footer={
        update.staged ? (
          <>
            <Button
              variant="primary"
              def
              hint="Restart now"
              disabled={busy}
              onClick={() => {
                setBusy(true);
                void native.power("reboot").catch((e) => {
                  setBusy(false);
                  toast(failed("Couldn't restart.", e), "error");
                });
              }}
            >
              {busy ? "Restarting" : "Restart now"}
            </Button>
            <Button hint="Later" onClick={close}>
              Later
            </Button>
          </>
        ) : (
          <Button def hint="Close" onClick={close}>
            Close
          </Button>
        )
      }
    >
      {update.staged ? (
        <div className="flex flex-col gap-[0.6rem] text-body text-fg/80">
          <p>The update is downloaded and installs while the device restarts. It takes a minute or two. Your games and files stay as they are.</p>
          <p>{running ? "A game is running. Close it first so nothing is lost." : "Later keeps the update waiting. It installs the next time the device restarts, and the launcher reminds you again next time it opens."}</p>
          <p className="text-small text-muted">Hold the power button to switch off only if you have to. A forced power-off can lose a waiting update, though it downloads again by itself.</p>
        </div>
      ) : null}
    </Sheet>
  );
}

/** "Restore previous system": the next start is the system from before Rare OS. */
export function RestoreSheet() {
  const open = useOsUpdate((s) => s.restoreOpen);
  const toast = useApp((s) => s.toast);
  const [busy, setBusy] = useState(false);
  const close = () => useOsUpdate.setState({ restoreOpen: false });
  useBack(close);
  if (!open) return null;
  const go = (reboot: boolean) => {
    setBusy(true);
    native
      .osRestore(reboot)
      .then((msg) => {
        toast(msg);
        if (!reboot) close();
      })
      .catch((e) => toast(failed("Couldn't restore the previous system.", e), "error"))
      .finally(() => setBusy(false));
  };
  return (
    <Sheet
      title="Restore previous system"
      sub="Go back to the system this device ran before Rare OS."
      footer={
        <>
          <Button variant="primary" def hint="Restore and restart" disabled={busy} onClick={() => go(true)}>
            Restore and restart
          </Button>
          <Button hint="Restore, restart later" disabled={busy} onClick={() => go(false)}>
            Restore, restart later
          </Button>
          <Button hint="Cancel" onClick={close}>
            Cancel
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-[0.6rem] text-body text-fg/80">
        <p>The next start is the previous system. Rare OS stays installed, and your files and games are shared by both.</p>
        <p>To come back to Rare OS, pick it in the boot menu.</p>
      </div>
    </Sheet>
  );
}
