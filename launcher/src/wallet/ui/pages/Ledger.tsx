// Add Ledger accounts: choose USB or Bluetooth, find the device, read its accounts (address and
// SUI balance), verify an address on the device screen, pick one or several, name them, add.
// The Ledger libraries load on first use.

import { useCallback, useEffect, useRef, useState } from "react";
import { MarketLoadBar } from "../../../marketplace/parts";
import { isTauri, onLedgerPairing, type LedgerDevice } from "../../native";
import { toLedgerError, LedgerError } from "../../ledger/errors";
import { useLedgerPrompt } from "../../ledger/prompt";
import { Btn, Empty, ListButton, Page, Panel, Segmented } from "../kit";
import { useWallet, type WalletView } from "../store";
import { H, finishAdd, useHints, useStepBack } from "../onboard/flow";
import { HwAccounts, HwError, type HwRow } from "../onboard/HwParts";
import { BluetoothStep } from "../onboard/BluetoothStep";

const lib = () => import("../../ledger");
const PAGE = 5;
const devUdev = !isTauri && typeof location !== "undefined" && new URLSearchParams(location.search).has("mockudev");
const MODEL: Record<string, string> = { nanoS: "Nano S", nanoSP: "Nano S Plus", nanoX: "Nano X", stax: "Stax", flex: "Flex", unknown: "Ledger" };

export function LedgerPage({ active }: { view: Extract<WalletView, { kind: "ledger" }>; active: boolean }) {
  const accounts = useWallet((s) => s.accounts);
  const prompt = useLedgerPrompt();
  const [transport, setTransport] = useState<"usb" | "ble">("usb");
  const [phase, setPhase] = useState<"find" | "connecting" | "accounts">("find");
  const [devices, setDevices] = useState<LedgerDevice[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [rows, setRows] = useState<HwRow[]>([]);
  const [model, setModel] = useState("");
  const [err, setErr] = useState<LedgerError | null>(null);
  const [more, setMore] = useState(false);
  const [adding, setAdding] = useState(false);
  const [verified, setVerified] = useState<Record<string, "busy" | "ok" | "bad">>({});
  const [code, setCode] = useState<string | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  // Bluetooth pairing: the Rust side sends the 6 digit code to compare with the Ledger's screen.
  useEffect(() => {
    let off: (() => void) | undefined;
    let dead = false;
    void onLedgerPairing(setCode).then((u) => (dead ? u() : (off = u)));
    return () => {
      dead = true;
      off?.();
    };
  }, []);

  const search = useCallback(async (t: "usb" | "ble") => {
    setSearching(true);
    setErr(null);
    setDevices(null);
    try {
      if (devUdev) throw new LedgerError("udev", undefined, undefined, { fix: "sudo mkdir -p /etc/udev/rules.d && printf '%s\\n' 'SUBSYSTEM==\"usb\", ATTRS{idVendor}==\"2c97\", TAG+=\"uaccess\"' 'KERNEL==\"hidraw*\", SUBSYSTEM==\"hidraw\", ATTRS{idVendor}==\"2c97\", TAG+=\"uaccess\"' | sudo tee /etc/udev/rules.d/70-rare-ledger.rules >/dev/null && printf '%s\\n' 'SUBSYSTEM==\"usb\", ATTRS{idVendor}==\"1209\", ATTRS{idProduct}==\"3001\", TAG+=\"uaccess\"' | sudo tee /etc/udev/rules.d/70-rare-keystone.rules >/dev/null && sudo udevadm control --reload-rules && sudo udevadm trigger" });
      const { listLedgers } = await lib();
      const all = await listLedgers({ ble: t === "ble", scanMs: 4000 });
      if (alive.current) setDevices(all.filter((d) => d.transport === t));
    } catch (e) {
      if (alive.current) setErr(toLedgerError(e));
    } finally {
      if (alive.current) setSearching(false);
    }
  }, []);

  useEffect(() => {
    void search(transport);
  }, [transport, search]);

  const connect = async (d: LedgerDevice) => {
    setErr(null);
    setPhase("connecting");
    try {
      const { connectLedger, ledgerAccounts } = await lib();
      const c = await connectLedger(d.id);
      setModel(c.name || MODEL[c.model]);
      const list = await ledgerAccounts(0, PAGE);
      if (!alive.current) return;
      setRows(list.map((a) => ({ index: a.index, address: a.address })));
      setPhase("accounts");
    } catch (e) {
      if (!alive.current) return;
      setErr(toLedgerError(e));
      setPhase("find");
    }
  };

  const showMore = async () => {
    setMore(true);
    setErr(null);
    try {
      const { ledgerAccounts } = await lib();
      const list = await ledgerAccounts(rows.length, PAGE);
      if (alive.current) setRows((r) => [...r, ...list.map((a) => ({ index: a.index, address: a.address }))]);
    } catch (e) {
      if (alive.current) setErr(toLedgerError(e));
    } finally {
      if (alive.current) setMore(false);
    }
  };

  const verify = async (r: HwRow) => {
    setVerified((v) => ({ ...v, [r.address]: "busy" }));
    setErr(null);
    try {
      const { verifyLedgerAddress } = await lib();
      const shown = await verifyLedgerAddress(r.index);
      setVerified((v) => ({ ...v, [r.address]: shown.address === r.address ? "ok" : "bad" }));
    } catch (e) {
      setVerified((v) => {
        const { [r.address]: _gone, ...rest } = v;
        return rest;
      });
      setErr(toLedgerError(e));
    }
  };

  const add = async (picked: Array<{ row: HwRow; label: string }>) => {
    setAdding(true);
    setErr(null);
    try {
      const { addLedgerAccount } = await lib();
      let first: string | undefined;
      for (const p of picked) {
        const a = await addLedgerAccount(p.row.index, p.label);
        first ??= a.address;
      }
      await finishAdd(first);
    } catch (e) {
      if (alive.current) {
        setErr(toLedgerError(e));
        setAdding(false);
      }
    }
  };

  useStepBack(active, phase === "accounts", () => setPhase("find"));
  useHints(active, [H.select, H.back]);

  const count = (accounts ?? []).filter((a) => a.kind === "ledger").length;
  const title = phase === "accounts" ? "Pick Ledger accounts" : "Connect your Ledger";
  const subtitle = phase === "accounts" ? model || undefined : "Unlock your Ledger and open the Sui app.";

  return (
    <Page active={active} narrow title={title} subtitle={subtitle} ready={phase + String(devices?.length ?? "x")}>
      {(searching || phase === "connecting") && <MarketLoadBar />}

      {phase !== "accounts" && (
        <div className="flex flex-col gap-5">
          <div>
            <div className="mb-1.5 text-sm text-secondary">How is it connected</div>
            <Segmented
              items={[
                { value: "usb", label: "USB cable" },
                { value: "ble", label: "Bluetooth" },
              ]}
              value={transport}
              onChange={(v) => setTransport(v === "ble" ? "ble" : "usb")}
            />
            <p className="mt-2 text-sm text-secondary">
              {transport === "usb" ? "Plug it in with a USB-C or USB cable." : "Nano X, Stax and Flex. Turn Bluetooth on from the Ledger's settings."}
            </p>
          </div>

          {code && (phase === "connecting" || searching) && (
            <Panel className="px-5 py-4 text-center">
              <div className="text-sm text-secondary">Pairing code</div>
              <div className="my-1 font-mono text-3xl tracking-widest">{code}</div>
              <div className="text-sm text-secondary">Check it matches your Ledger and confirm there.</div>
            </Panel>
          )}

          {phase === "connecting" ? (
            <Empty title={prompt.message || "Connecting to your Ledger"} compact>
              Keep it unlocked with the Sui app open.
            </Empty>
          ) : err && err.code === "ble-off" && transport === "ble" ? (
            <BluetoothStep note="Turn Bluetooth on to connect your Ledger over Bluetooth." onReady={() => void search("ble")} />
          ) : err ? (
            <HwError message={err.message} fix={err.code === "udev" ? err.fix : null} onRetry={() => void search(transport)} />
          ) : devices && devices.length > 0 ? (
            <Panel className="divide-y divide-border-primary p-1">
              {devices.map((d, i) => (
                <ListButton
                  key={d.id}
                  def={i === 0}
                  title={d.name || MODEL[d.model]}
                  body={d.transport === "ble" ? (d.paired ? "Bluetooth, paired" : "Bluetooth") : "USB"}
                  right="Connect"
                  onClick={() => void connect(d)}
                />
              ))}
            </Panel>
          ) : devices && !searching ? (
            <Empty title="No Ledger found" compact action={<Btn def onClick={() => void search(transport)}>Search again</Btn>}>
              {transport === "usb" ? "Plug it in, unlock it and open the Sui app." : "Turn on Bluetooth on your Ledger, unlock it and open the Sui app."}
            </Empty>
          ) : null}
        </div>
      )}

      {phase === "accounts" && (
        <HwAccounts
          rows={rows}
          existing={(accounts ?? []).map((a) => a.address)}
          defaultName={(n) => `Ledger ${count + n + 1}`}
          onVerify={(r) => void verify(r)}
          onMore={() => void showMore()}
          more={more}
          onAdd={(p) => void add(p)}
          adding={adding}
          verified={verified}
          error={err ? err.message : undefined}
        />
      )}
    </Page>
  );
}
