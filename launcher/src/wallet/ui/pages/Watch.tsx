// Watch an address: view-only account from a 0x address or a SuiNS name.

import { useState } from "react";
import { watch } from "../../index";
import { Btn, Field, Page } from "../kit";
import { H, finishAdd, plainError, useHints } from "../onboard/flow";
import type { WalletView } from "../store";

export function WatchPage({ active }: { view: Extract<WalletView, { kind: "watch" }>; active: boolean }) {
  const [input, setInput] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useHints(active, [H.select, H.back]);

  const add = async () => {
    setBusy(true);
    setError("");
    try {
      const a = await watch(input.trim(), name.trim() || undefined);
      await finishAdd(a.address);
    } catch (e) {
      setError(plainError(e, "Could not add that address"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Page active={active} narrow title="Watch an address" subtitle="Follow any wallet's coins and collectibles. You cannot send from it.">
      <div className="flex flex-col gap-5">
        <Field def label="Address or SuiNS name" value={input} onChange={setInput} placeholder="0x... or name.sui" error={error} />
        <Field label="Name (optional)" value={name} onChange={setName} placeholder="Watch 1" />
        <Btn variant="primary" className={`w-full ${!input.trim() || busy ? "pointer-events-none opacity-40" : ""}`} onClick={() => void add()}>
          {busy ? "Adding" : "Start watching"}
        </Btn>
      </div>
    </Page>
  );
}
