// Add Keystone 3 Pro accounts over USB: list devices, read accounts (address and SUI balance),
// pick one or several, name them, add.

import { useCallback, useEffect, useRef, useState } from "react";
import { MarketLoadBar } from "../../../marketplace/parts";
import type { KeystoneDevice } from "../../native";
import { Btn, Empty, ListButton, Page, Panel } from "../kit";
import { useWallet, type WalletView } from "../store";
import { H, finishAdd, useHints, useStepBack } from "../onboard/flow";
import { HwAccounts, HwError, type HwRow } from "../onboard/HwParts";
import { keystoneApi, type KeystoneAccountInfo } from "../onboard/keystoneApi";

const PAGE = 5;

export function KeystonePage({ active }: { view: Extract<WalletView, { kind: "keystone" }>; active: boolean }) {
  const accounts = useWallet((s) => s.accounts);
  const [phase, setPhase] = useState<"find" | "reading" | "accounts">("find");
  const [devices, setDevices] = useState<KeystoneDevice[] | null>(null);
  const [device, setDevice] = useState<KeystoneDevice | null>(null);
  const [searching, setSearching] = useState(false);
  const [infos, setInfos] = useState<KeystoneAccountInfo[]>([]);
  const [err, setErr] = useState<{ message: string; fix: string | null } | null>(null);
  const [more, setMore] = useState(false);
  const [adding, setAdding] = useState(false);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const fail = async (e: unknown) => {
    const k = (await keystoneApi()).toError(e);
    if (alive.current) setErr({ message: k.message, fix: k.fix ?? null });
  };

  const search = useCallback(async () => {
    setSearching(true);
    setErr(null);
    setDevices(null);
    try {
      const l = await (await keystoneApi()).list();
      if (alive.current) setDevices(l);
    } catch (e) {
      void fail(e);
    } finally {
      if (alive.current) setSearching(false);
    }
  }, []);

  useEffect(() => {
    void search();
  }, [search]);

  const read = async (d: KeystoneDevice) => {
    setErr(null);
    setDevice(d);
    setPhase("reading");
    try {
      const list = await (await keystoneApi()).accounts(0, PAGE);
      if (!alive.current) return;
      setInfos(list);
      setPhase("accounts");
    } catch (e) {
      void fail(e);
      if (alive.current) setPhase("find");
    }
  };

  const showMore = async () => {
    if (!device) return;
    setMore(true);
    setErr(null);
    try {
      const list = await (await keystoneApi()).accounts(infos.length, PAGE);
      if (alive.current) setInfos((l) => [...l, ...list]);
    } catch (e) {
      void fail(e);
    } finally {
      if (alive.current) setMore(false);
    }
  };

  const add = async (picked: Array<{ row: HwRow; label: string }>) => {
    setAdding(true);
    setErr(null);
    try {
      const api = await keystoneApi();
      let first: string | undefined;
      for (const p of picked) {
        const a = await api.add(p.row.index, p.label);
        first ??= a.address;
      }
      await finishAdd(first);
    } catch (e) {
      void fail(e);
      if (alive.current) setAdding(false);
    }
  };

  useStepBack(active, phase === "accounts", () => setPhase("find"));
  useHints(active, [H.select, H.back]);

  const count = (accounts ?? []).filter((a) => a.kind === "keystone").length;
  return (
    <Page
      active={active}
      narrow
      title={phase === "accounts" ? "Pick Keystone accounts" : "Connect your Keystone"}
      subtitle={phase === "accounts" ? device?.name : "Connect your Keystone 3 Pro with a USB-C cable. On the Keystone: Settings > Connection > USB."}
      ready={phase + String(devices?.length ?? "x")}
    >
      {(searching || phase === "reading") && <MarketLoadBar />}
      {phase !== "accounts" && (
        <div className="flex flex-col gap-5">
          {phase === "reading" ? (
            <Empty title="Reading your Keystone" compact>
              Keep it unlocked and on the USB screen.
            </Empty>
          ) : err ? (
            <HwError message={err.message} fix={err.fix} onRetry={() => void search()} />
          ) : devices && devices.length > 0 ? (
            <Panel className="divide-y divide-border-primary p-1">
              {devices.map((d, i) => (
                <ListButton key={d.id} def={i === 0} title={d.name} body="USB" right="Connect" onClick={() => void read(d)} />
              ))}
            </Panel>
          ) : devices && !searching ? (
            <Empty title="No Keystone found" compact action={<Btn def onClick={() => void search()}>Search again</Btn>}>
              Plug it in, unlock it and turn on USB in its connection settings.
            </Empty>
          ) : null}
          <p className="text-sm text-secondary">The Keystone signs a transaction hash over USB. Rare shows you the decoded transaction first, so check it before you approve on the device.</p>
        </div>
      )}
      {phase === "accounts" && (
        <HwAccounts
          rows={infos.map((i) => ({ index: i.index, address: i.address }))}
          existing={(accounts ?? []).map((a) => a.address)}
          defaultName={(n) => `Keystone ${count + n + 1}`}
          onMore={() => void showMore()}
          more={more}
          onAdd={(p) => void add(p)}
          adding={adding}
          verified={{}}
          error={err?.message}
        />
      )}
    </Page>
  );
}
