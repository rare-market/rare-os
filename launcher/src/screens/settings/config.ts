// Settings writes: read the saved config again, change only the asked fields, write it back.
// A screen that loaded the config a minute ago never overwrites what other screens saved since
// (web games, onboarding step, clips). Writes run one after the other.

import { native } from "../../lib/bridge";
import type { Config } from "../../lib/types";

let chain: Promise<unknown> = Promise.resolve();

export function patchConfig(p: Partial<Config>): Promise<Config> {
  const run = async () => {
    const cur = await native.config();
    const next = { ...cur, ...p };
    await native.setConfig(next);
    return next;
  };
  const r = chain.then(run, run);
  chain = r.catch(() => {});
  return r;
}

export interface SectionProps {
  cfg: Config;
  patch: (p: Partial<Config>) => Promise<void>;
}
