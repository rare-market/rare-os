// Opening Settings on a section from elsewhere (a notice, for example). Settings clears it once shown.

import { create } from "zustand";
import { useApp } from "../../lib/store";
import type { SectionId } from "./sections";

export const useSettingsJump = create<{ section: SectionId | null; seq: number }>(() => ({ section: null, seq: 0 }));

export function openSettingsSection(section: SectionId) {
  const app = useApp.getState();
  app.close();
  useSettingsJump.setState((s) => ({ section, seq: s.seq + 1 }));
  app.setTab("settings");
}
