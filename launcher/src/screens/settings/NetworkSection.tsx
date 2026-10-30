// Network: Wi-Fi (the same scan and join as first run) and Bluetooth. NetworkManager keeps the
// networks you joined and joins them again at boot; BlueZ keeps its own adapter state.

import { useCallback, useEffect, useRef, useState } from "react";
import { askText } from "../../components/Osk";
import { IconWifi } from "../../components/icons";
import { native } from "../../lib/bridge";
import { useApp } from "../../lib/store";
import type { BluetoothAdapter, NetStatus, WifiNet } from "../../lib/types";
import { Bars } from "../../onboarding/steps/Wifi";
import { Button, Dot, Panel } from "../../ui";
import { SettingRow } from "./rows";
import { failed } from "../../lib/fail";

function BluetoothRow() {
  const toast = useApp((s) => s.toast);
  const [bt, setBt] = useState<BluetoothAdapter | null | undefined>(undefined);
  useEffect(() => {
    void native.bluetoothState().then(setBt).catch(() => setBt(null));
  }, []);
  if (bt === undefined) return null;
  const body = !bt || !bt.present ? "No Bluetooth adapter found on this device." : bt.powered ? "On. Pair controllers and headphones in desktop mode." : bt.blocked ? "Off, and blocked by a switch or airplane mode." : "Off.";
  return (
    <SettingRow id="bluetooth" title="Bluetooth" body={body}>
      {bt?.present && !bt.powered && (
        <Button
          onClick={() =>
            native
              .bluetoothPowerOn()
              .then(setBt)
              .catch((e) => toast(failed("Couldn't turn Bluetooth on.", e), "error"))
          }
        >
          Turn on
        </Button>
      )}
      {bt?.present && bt.powered && (
        <span className="flex shrink-0 items-center gap-[0.5rem] text-small text-muted">
          <Dot tone="muted" />
          On
        </span>
      )}
    </SettingRow>
  );
}

export function NetworkSection() {
  const [net, setNet] = useState<NetStatus | null>(null);
  const [nets, setNets] = useState<WifiNet[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [joining, setJoining] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);

  const status = useCallback(async () => {
    const s = await native.netStatus().catch((): NetStatus => ({ online: navigator.onLine, wifi: null, managed: false }));
    if (alive.current) setNet(s);
    return s;
  }, []);
  const scan = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      const l = await native.wifiScan();
      if (alive.current) setNets(l);
    } catch (e) {
      if (alive.current) {
        setNets([]);
        setError(failed("Couldn't look for networks.", e));
      }
    } finally {
      if (alive.current) setScanning(false);
    }
  }, []);
  useEffect(() => {
    alive.current = true;
    void status().then((s) => s.managed && void scan());
    const id = setInterval(() => void status(), 4000);
    return () => {
      alive.current = false;
      clearInterval(id);
    };
  }, [status, scan]);

  const join = async (n: WifiNet) => {
    if (n.active) return;
    let password: string | undefined;
    if (n.secure) {
      const p = await askText(`Password for ${n.ssid}`, "", { secret: true });
      if (p == null) return;
      password = p;
    }
    setJoining(n.ssid);
    setError(null);
    try {
      await native.wifiConnect(n.ssid, password);
      await status();
      void scan();
    } catch (e) {
      const m = String((e as Error).message ?? e);
      setError(/secret|password|802-11-wireless-security/i.test(m) ? `Couldn't join ${n.ssid}. Check the password and try again.` : failed(`Couldn't join ${n.ssid}.`, e));
    } finally {
      setJoining(null);
    }
  };
  const forget = async (ssid: string) => {
    setError(null);
    try {
      await native.wifiForget(ssid);
      await status();
      void scan();
    } catch (e) {
      setError(failed("Couldn't forget that network.", e));
    }
  };

  const active = nets?.find((n) => n.active) ?? null;
  return (
    <div className="fade flex flex-col gap-[1.25rem]">
      <Panel>
        <SettingRow
          id="wifi"
          title="Wi-Fi"
          body={
            net == null
              ? "Checking"
              : !net.managed
                ? "Managed outside Rare OS. Change networks in desktop mode."
                : net.wifi
                  ? `Connected to ${net.wifi}${net.online ? "" : ", no internet"}.`
                  : net.online
                    ? "Connected to the internet."
                    : "Not connected."
          }
        >
          {net?.managed && (
            <Button onClick={() => void scan()} disabled={scanning || !!joining}>
              {scanning ? "Scanning" : "Scan again"}
            </Button>
          )}
        </SettingRow>
        {error && <SettingRow title={<span className="text-neg">{error}</span>} />}
        {net?.managed && active && (
          <SettingRow id="wifi-forget" title="Forget this network" body={`${active.ssid} is not joined automatically after this.`}>
            <Button onClick={() => void forget(active.ssid)}>Forget</Button>
          </SettingRow>
        )}
        <BluetoothRow />
      </Panel>
      {net?.managed && (
        <Panel>
          <div data-nav-group="settings-wifi-list">
            {nets && nets.length === 0 && !scanning && <SettingRow title="No networks found" body="Move closer to your router, then scan again." />}
            {(nets ?? []).map((n) => (
              <button
                key={n.ssid}
                data-nav
                data-hint-a={n.active ? "Connected" : joining ? "Joining" : "Join"}
                onClick={() => void join(n)}
                className="flex min-h-[3.75rem] w-full items-center gap-[1rem] border-t border-hair px-[1.4rem] py-[0.7rem] text-left first:border-t-0"
              >
                <IconWifi size="1.15rem" className="shrink-0 text-icon" />
                <span className="min-w-0 flex-1 truncate text-body font-medium">{n.ssid}</span>
                {joining === n.ssid && <span className="text-small text-muted">Joining</span>}
                {n.active && joining !== n.ssid && <span className="text-small text-muted">Connected</span>}
                {!n.active && n.secure && joining !== n.ssid && <span className="text-small text-muted">Password</span>}
                <Bars signal={n.signal} />
              </button>
            ))}
          </div>
        </Panel>
      )}
    </div>
  );
}
