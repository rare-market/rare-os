import { useCallback, useEffect, useRef, useState } from "react";
import { askText } from "../../components/Osk";
import { IconWifi } from "../../components/icons";
import { native } from "../../lib/bridge";
import type { NetStatus, WifiNet } from "../../lib/types";
import type { FirstRunStepProps } from "../../screens/FirstRun";
import { Button, Empty } from "../../ui";
import { DoneLine, Step, Working, useStepB } from "../kit";
import { failed } from "../../lib/fail";

export function Bars({ signal }: { signal: number }) {
  const n = signal >= 75 ? 4 : signal >= 50 ? 3 : signal >= 25 ? 2 : 1;
  return (
    <span className="flex h-[0.95rem] items-end gap-[2px]" aria-label={`Signal ${signal}%`}>
      {[1, 2, 3, 4].map((i) => (
        <span key={i} className={`w-[3px] rounded-[1px] ${i <= n ? "bg-fg/85" : "bg-fg/20"}`} style={{ height: `${i * 25}%` }} />
      ))}
    </span>
  );
}

export function Wifi({ next, back }: FirstRunStepProps) {
  useStepB(back);
  const [net, setNet] = useState<NetStatus | null>(null);
  const [nets, setNets] = useState<WifiNet[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [joining, setJoining] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const wasOffline = useRef(false);

  const status = useCallback(async () => {
    const s = await native.netStatus().catch((): NetStatus => ({ online: navigator.onLine, wifi: null, managed: false }));
    setNet(s);
    return s;
  }, []);

  const scan = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      setNets(await native.wifiScan());
    } catch (e) {
      setNets([]);
      setError(failed("Couldn't look for networks.", e));
    } finally {
      setScanning(false);
    }
  }, []);

  useEffect(() => {
    void status().then((s) => {
      if (!s.online) wasOffline.current = true;
      if (s.managed) void scan();
    });
    const id = setInterval(() => void status(), 3000);
    return () => clearInterval(id);
  }, [status, scan]);

  // Coming online here moves on by itself (not when B brought you back to a connected screen).
  useEffect(() => {
    if (!net?.online || !wasOffline.current) return;
    wasOffline.current = false;
    const t = setTimeout(() => next(), 1400);
    return () => clearTimeout(t);
  }, [net?.online]); // eslint-disable-line react-hooks/exhaustive-deps

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
    wasOffline.current = true;
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

  const online = !!net?.online;
  return (
    <Step
      title="Connect to Wi-Fi"
      body="Games, updates, Rare Marketplace and your wallet need the internet. Pick your network."
      actions={
        <>
          <Button variant={online ? "primary" : "secondary"} size="lg" def={online} onClick={() => next()}>
            {online ? "Continue" : "Continue offline"}
          </Button>
          {net?.managed && (
            <Button variant="ghost" size="lg" onClick={() => void scan()} disabled={scanning}>
              Scan again
            </Button>
          )}
        </>
      }
    >
      <div className="flex flex-col gap-[1rem]">
        {online ? (
          <DoneLine>{net?.wifi ? <>Connected to <span className="font-semibold">{net.wifi}</span></> : "Connected to the internet"}</DoneLine>
        ) : net ? (
          <div className="text-body text-muted">Not connected yet.</div>
        ) : null}
        {(scanning || joining) && <Working on label={joining ? `Joining ${joining}` : "Looking for networks"} />}
        {error && <div className="text-small text-neg">{error}</div>}
        {net && !net.managed ? (
          <Empty title="Wi-Fi is managed outside Rare" body="This system doesn't let the launcher change networks. Connect in desktop mode, then come back. If the device is online already, just continue." />
        ) : nets && nets.length === 0 && !scanning ? (
          <Empty title="No networks found" body="Move closer to your router, or check that Wi-Fi is turned on, then scan again." />
        ) : (
          <div data-nav-group="wifi-list" className="flex flex-col gap-[0.4rem]">
            {(nets ?? []).map((n, i) => (
              <button
                key={n.ssid}
                data-nav
                data-nav-default={!online && i === 0 ? "" : undefined}
                data-hint-a={n.active ? "Connected" : "Join"}
                onClick={() => void join(n)}
                className="flex items-center gap-[1rem] rounded-card border border-hair bg-l1 px-[1.2rem] py-[0.85rem] text-left"
              >
                <IconWifi size="1.15rem" className="shrink-0 text-icon" />
                <span className="min-w-0 flex-1 truncate text-body font-medium">{n.ssid}</span>
                {n.active && <span className="text-small text-pos">Connected</span>}
                {!n.active && n.secure && <span className="text-small text-muted">Password</span>}
                {!n.active && !n.secure && <span className="text-small text-muted">Open</span>}
                <Bars signal={n.signal} />
              </button>
            ))}
          </div>
        )}
      </div>
    </Step>
  );
}
