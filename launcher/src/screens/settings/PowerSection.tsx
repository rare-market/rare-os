// Power: profile, power limit, CPU boost, fan, sleep when idle, battery care.
// Profile, limit and boost live in launcher.json and system::keep puts them back at start,
// after a resume and when the charger changes. The fan preset and battery care are kept by
// rare-fand (/var/lib/rare). A frame limit is per game (Quick settings), so it is not here.

import { useEffect, useState } from "react";
import { native } from "../../lib/bridge";
import { useApp } from "../../lib/store";
import { DEFAULT_IDLE_BATTERY_MIN, idleLabel, nextIdleChoice } from "../../lib/sleep";
import type { BatteryCare, FanMode, Perf, Profile } from "../../lib/types";
import { Button, Panel } from "../../ui";
import { ChoiceRow, SettingRow, SliderRow, SwitchRow } from "./rows";
import type { SectionProps } from "./config";
import { failed } from "../../lib/fail";

const PROFILES: Array<{ value: Exclude<Profile, "custom">; label: string; watts: number }> = [
  { value: "quiet", label: "Quiet", watts: 10 },
  { value: "balanced", label: "Balanced", watts: 15 },
  { value: "performance", label: "Performance", watts: 25 },
];
const FANS: Array<{ value: FanMode; label: string }> = [
  { value: "default", label: "Default" },
  { value: "cool", label: "Cool" },
  { value: "quiet", label: "Quiet" },
];

const CARE_NOTE: Record<BatteryCare["state"], string> = {
  off: "Charging stops at 80% while plugged in and starts again at 75%, so the battery ages more slowly.",
  unavailable: "",
  unplugged: "Charging stops at 80% the next time you plug in.",
  low: "Charging normally until the battery is above 70%.",
  hot: "Charging normally while the battery is warm.",
  unknown: "Charging normally while the battery level can't be read.",
  charging: "Plugged in and charging to 80%.",
  holding: "Plugged in and holding at 80%.",
};

/** Battery care (rare-fand) and battery health. Nothing on devices without a battery. */
function BatteryRows() {
  const device = useApp((s) => s.device);
  const toast = useApp((s) => s.toast);
  const [care, setCare] = useState<BatteryCare | null>(null);
  useEffect(() => {
    void native.batteryCare().then(setCare).catch(() => setCare(null));
  }, []);
  const health = device?.battery?.health ?? null;
  const showCare = care?.available ?? false;
  return (
    <>
      {care && showCare && (
        <SwitchRow
          id="battery-care"
          title="Battery care (stop at 80%)"
          body={CARE_NOTE[care.on ? care.state : "off"] || CARE_NOTE.off}
          on={care.on}
          onChange={(v) => {
            setCare({ ...care, on: v, state: v ? care.state : "off" });
            native
              .setBatteryCare(v)
              // rare-fand applies it within 5 s; read back what it did.
              .then(() => setTimeout(() => void native.batteryCare().then(setCare), 6000))
              .catch((e) => {
                setCare(care);
                toast(failed("Couldn't change battery care.", e), "error");
              });
          }}
        />
      )}
      {health != null && <SettingRow id="battery-health" title={`Battery health ${health}%`} body="How much charge the battery holds now, compared with when it was new." />}
    </>
  );
}

export function PowerSection({ cfg, patch }: SectionProps) {
  const toast = useApp((s) => s.toast);
  const [perf, setPerf] = useState<Perf | null>(null);
  const [tdp, setTdp] = useState(15);
  const reload = () =>
    native
      .perf()
      .then((p) => {
        setPerf(p);
        setTdp(p.tdp ?? 15);
      })
      .catch(() => setPerf(null));
  useEffect(() => void reload(), []); // eslint-disable-line react-hooks/exhaustive-deps

  const apply = async (profile: Profile, t?: number) => {
    try {
      await native.setPerf(profile, t, undefined);
    } catch (e) {
      toast(failed("Couldn't change the performance profile.", e), "error");
    }
    await reload();
  };
  const hasPerf = !!perf?.available;
  return (
    <div className="fade flex flex-col gap-[1.25rem]">
      <Panel>
        {hasPerf && perf && (
          <>
            <ChoiceRow
              id="profile"
              title="Performance profile"
              body={`${Math.round(perf.profile === "custom" ? (perf.tdp ?? tdp) : (perf.profileWatts?.[perf.profile] ?? 15))} W. Kept after a restart. Each game starts on Balanced unless you save settings for it in Quick settings.`}
              value={perf.profile === "custom" ? null : perf.profile}
              choices={PROFILES.map((p) => ({ value: p.value, label: p.label }))}
              onPick={(v) => void apply(v)}
            />
            <SliderRow
              id="tdp"
              title="Power limit (TDP)"
              body="A limit of your own instead of a profile."
              value={tdp}
              min={perf.tdpMin}
              max={perf.tdpMax}
              step={1}
              suffix=" W"
              onChange={(v) => {
                setTdp(v);
                void apply("custom", v);
              }}
            />
            {perf.cpuBoost != null && (
              <SwitchRow
                id="cpu-boost"
                title="CPU boost"
                body="Faster CPU, more heat and battery."
                on={perf.cpuBoost}
                onChange={(v) => {
                  setPerf({ ...perf, cpuBoost: v });
                  native
                    .setCpuBoost(v)
                    .catch((e) => toast(failed("Couldn't change CPU boost.", e), "error"))
                    .finally(() => void reload());
                }}
              />
            )}
            {perf.fan != null && (
              <ChoiceRow
                id="fan"
                title="Fan"
                body="Cool spins up early. Quiet stays low to about 65 C."
                value={perf.fan}
                choices={FANS}
                onPick={(v) => {
                  setPerf({ ...perf, fan: v });
                  native
                    .setFan(v)
                    .catch((e) => toast(failed("Couldn't change the fan.", e), "error"))
                    .finally(() => void reload());
                }}
              />
            )}
          </>
        )}
        {perf && !hasPerf && <SettingRow title="Performance" body={perf.note || "This device does not let Rare OS change its power limits."} />}
      </Panel>
      <Panel>
        <SettingRow id="sleep-idle" title="Sleep when idle" body="Sleeps after this long with no input. Never while a game runs or sound plays. Press the power button to wake.">
          <Button onClick={() => void patch({ sleepIdleBattery: nextIdleChoice(cfg.sleepIdleBattery ?? DEFAULT_IDLE_BATTERY_MIN) })}>
            On battery: {idleLabel(cfg.sleepIdleBattery ?? DEFAULT_IDLE_BATTERY_MIN)}
          </Button>
          <Button onClick={() => void patch({ sleepIdleAc: nextIdleChoice(cfg.sleepIdleAc ?? 0) })}>Charging: {idleLabel(cfg.sleepIdleAc ?? 0)}</Button>
        </SettingRow>
        <BatteryRows />
      </Panel>
    </div>
  );
}
