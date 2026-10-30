// Price alerts: device scoped on rare.market (POST /v1/notify/alerts with this launcher's device
// id). Set from a coin's page in Trade, listed and deleted in Settings > Notifications.

import { create } from "zustand";
import * as api from "./api";
import { deviceId } from "./device";
import type { PriceAlert } from "./types";

interface AlertsState {
  list: PriceAlert[] | null;
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  add: (a: { coinType: string; symbol: string; dir: "above" | "below"; target: number; addr?: string }) => Promise<void>;
  remove: (id: string) => Promise<void>;
}

export const useAlerts = create<AlertsState>((set, get) => ({
  list: null,
  loading: false,
  error: null,
  load: async () => {
    set({ loading: true });
    try {
      set({ list: await api.listAlerts(deviceId()), error: null });
    } catch (e) {
      set({ error: e instanceof Error ? e.message : "Could not load price alerts" });
    } finally {
      set({ loading: false });
    }
  },
  add: async (a) => {
    await api.addAlert({ device: deviceId(), ...a });
    // The stream watches this list: a device with alerts but no accounts connects for them.
    await get().load();
  },
  remove: async (id) => {
    set({ list: (get().list ?? []).filter((x) => x.id !== id) });
    try {
      await api.deleteAlert(id, deviceId());
    } finally {
      await get().load();
    }
  },
}));
