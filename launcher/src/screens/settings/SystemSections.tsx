// System, Display, Sound and Controls: the small sections.

import { useEffect, useState } from "react";
import { native } from "../../lib/bridge";
import { LANGUAGES, languageName } from "../../lib/locales";
import { useApp } from "../../lib/store";
import { setFeedback } from "../../nav/feedback";
import { Button, Panel } from "../../ui";
import { DisplayRows } from "./DisplayRows";
import { ChoiceRow, SettingRow, SliderRow, SwitchRow } from "./rows";
import type { ButtonAction } from "../../lib/types";
import type { SectionProps } from "./config";
import { SystemUpdateRows } from "./SystemUpdateRows";
import { failed } from "../../lib/fail";

export function SystemSection({ cfg, patch }: SectionProps) {
  const toast = useApp((s) => s.toast);
  return (
    <div className="fade flex flex-col gap-[1.25rem]">
      <SystemUpdateRows />
      <Panel>
      <SettingRow id="language" title="Language" body={LANGUAGES.length > 1 ? "The language of Rare OS." : "Rare OS is in English for now."}>
        {LANGUAGES.length > 1 ? (
          <Button
            onClick={() => {
              const i = LANGUAGES.findIndex((l) => l.code === (cfg.language ?? "en"));
              void patch({ language: LANGUAGES[(i + 1) % LANGUAGES.length].code });
            }}
          >
            {languageName(cfg.language)}
          </Button>
        ) : (
          <span className="shrink-0 text-small text-muted">{languageName(cfg.language)}</span>
        )}
      </SettingRow>
      <SettingRow id="desktop-mode" title="Desktop mode" body="A full desktop for installing apps and managing files.">
        <Button onClick={() => void native.power("desktop").catch((e) => toast(failed("Couldn't switch to Desktop mode.", e), "error"))}>Switch</Button>
      </SettingRow>
      </Panel>
    </div>
  );
}

export function DisplaySection() {
  const toast = useApp((s) => s.toast);
  const [level, setLevel] = useState<number | null | undefined>(undefined);
  const [dimIdle, setDimIdle] = useState<boolean | null>(null);
  useEffect(() => {
    void native.levels().then((l) => setLevel(l.brightness)).catch(() => setLevel(null));
    void native.brightnessInfo().then((i) => setDimIdle(i.dimIdle)).catch(() => setDimIdle(null));
  }, []);
  return (
    <Panel className="fade">
      <DisplayRows />
      {level != null && (
        <SliderRow
          id="brightness"
          title="Brightness"
          body={level < 10 ? "Night dim. Darker to look at, but it does not save battery." : "Left and right to change."}
          value={level}
          min={0}
          max={100}
          step={5}
          onChange={(v) => {
            setLevel(v);
            void native.setLevels(v, undefined).catch((e) => toast(failed("Couldn't change the brightness.", e), "error"));
          }}
        />
      )}
      {level != null && dimIdle != null && (
        <ChoiceRow
          id="dim-idle"
          title="Dim after idle"
          body="Dims the screen to 30% after 60 seconds with no input in the launcher. Never in a game."
          value={dimIdle ? "on" : "off"}
          choices={[
            { value: "off", label: "Off" },
            { value: "on", label: "60 s" },
          ]}
          onPick={(v) => {
            setDimIdle(v === "on");
            void native.setDimIdle(v === "on").catch((e) => toast(failed("Couldn't change screen dimming.", e), "error"));
          }}
        />
      )}
      {level === null && <SettingRow title="Screen" body="This device has no screen brightness to change." />}
    </Panel>
  );
}

export function SoundSection({ cfg, patch }: SectionProps) {
  const toast = useApp((s) => s.toast);
  const [volume, setVolume] = useState<number | null | undefined>(undefined);
  useEffect(() => {
    void native.levels().then((l) => setVolume(l.volume)).catch(() => setVolume(null));
  }, []);
  return (
    <Panel className="fade">
      {volume != null && (
        <SliderRow
          id="volume"
          title="Volume"
          body="Left and right to change."
          value={volume}
          min={0}
          max={100}
          step={5}
          onChange={(v) => {
            setVolume(v);
            void native.setLevels(undefined, v).catch((e) => toast(failed("Couldn't change the volume.", e), "error"));
          }}
        />
      )}
      {volume === null && <SettingRow title="Volume" body="No sound output found." />}
      <SwitchRow
        id="sounds"
        title="Sounds"
        body="Soft clicks when you move, select and go back."
        on={cfg.uiSounds ?? true}
        onChange={(v) => {
          setFeedback({ sounds: v });
          void patch({ uiSounds: v });
        }}
      />
    </Panel>
  );
}

const ACTIONS: Array<{ value: ButtonAction; label: string }> = [
  { value: "capture", label: "Capture" },
  { value: "keyboard", label: "Keyboard" },
  { value: "perf", label: "Performance overlay" },
  { value: "none", label: "Nothing" },
];

export function ControlsSection({ cfg, patch }: SectionProps) {
  const [pad, setPad] = useState<{ available: boolean; name: string | null; motion: boolean } | null>(null);
  useEffect(() => {
    void native.inputStatus().then(setPad);
  }, [cfg.gyroEnabled, cfg.gyroInSteam]);
  const pct = (v: number | null | undefined, d: number) => v ?? d;
  return (
    <div className="fade flex flex-col gap-[1.25rem]">
      <Panel>
        <SettingRow id="buttons" title="Buttons" body="The SuiPlay's own buttons. Rare and Quick always do the same thing, so no game or app can take them." />
        <SettingRow title="Rare" body="Press for the Rare menu, press again for Home, hold for the Switcher. Fixed." />
        <SettingRow title="Quick" body="Quick Settings. Fixed." />
        <ChoiceRow id="btn-lc" title="LC" body="The small button at the top left. Press." value={cfg.lcAction ?? "capture"} choices={ACTIONS} onPick={(v) => void patch({ lcAction: v })} />
        <ChoiceRow id="btn-rc" title="RC" body="The small button at the top right. Press." value={cfg.rcAction ?? "keyboard"} choices={ACTIONS} onPick={(v) => void patch({ rcAction: v })} />
        <SettingRow title="Back paddles" body="This device has none." />
        <SettingRow title="Holds" body="Holding LC saves the last 30 seconds and holding RC changes the performance overlay, whatever you pick above. Inside Steam Game Mode Steam keeps Rare and Quick." />
      </Panel>
      <Panel>
        <SliderRow id="deadzone-left" title="Left stick deadzone" value={pct(cfg.deadzoneLeft, 10)} min={0} max={40} step={1} onChange={(v) => void patch({ deadzoneLeft: v })} />
        <SliderRow id="deadzone-right" title="Right stick deadzone" value={pct(cfg.deadzoneRight, 10)} min={0} max={40} step={1} onChange={(v) => void patch({ deadzoneRight: v })} />
        <SwitchRow id="invert-left" title="Invert left stick, up and down" body="Pushing up moves down." on={cfg.invertLeftY ?? false} onChange={(v) => void patch({ invertLeftY: v })} />
        <SwitchRow id="invert-right" title="Invert right stick, up and down" body="For looking around in games." on={cfg.invertRightY ?? false} onChange={(v) => void patch({ invertRightY: v })} />
      </Panel>
      <Panel>
        <SwitchRow
          id="gyro-on"
          title="Gyro"
          body={pad === null ? "Motion controls." : pad.available ? (pad.motion ? "On. The controller carries motion right now." : "Motion controls, through InputPlumber.") : "InputPlumber is not running, so the controller can't be told."}
          on={cfg.gyroEnabled ?? true}
          onChange={(v) => void patch({ gyroEnabled: v })}
        />
        <SliderRow id="gyro-sens" title="Gyro sensitivity" value={pct(cfg.gyroSensitivity, 100)} min={25} max={200} step={5} onChange={(v) => void patch({ gyroSensitivity: v })} />
        <SwitchRow
          id="gyro"
          title="Gyro in Steam games"
          body="Motion controls in Steam games. The controller shows up as a Steam Deck while a Steam game runs."
          on={cfg.gyroInSteam ?? false}
          onChange={(v) => void patch({ gyroInSteam: v })}
        />
      </Panel>
      <Panel>
        <SwitchRow
          id="rumble"
          title="Rumble"
          body="A light buzz on the same moves, on controllers that support it."
          on={cfg.rumble ?? true}
          onChange={(v) => {
            setFeedback({ rumble: v });
            void patch({ rumble: v });
          }}
        />
        <SliderRow id="rumble-strength" title="Rumble strength" value={pct(cfg.rumbleStrength, 100)} min={0} max={100} step={10} onChange={(v) => void patch({ rumbleStrength: v })} />
      </Panel>
    </div>
  );
}
