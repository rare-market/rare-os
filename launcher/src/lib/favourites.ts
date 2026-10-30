// Favourite games, picked in first run (and later with X on a game page). They lead the Home
// shelf. Kept in local storage: per device, survives updates, no native config change needed.

import { create } from "zustand";

const KEY = "rare.favourites";

function read(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export const useFavourites = create<{ ids: string[]; toggle: (id: string) => void; set: (ids: string[]) => void }>((set, get) => ({
  ids: read(),
  toggle: (id) => get().set(get().ids.includes(id) ? get().ids.filter((x) => x !== id) : [...get().ids, id]),
  set: (ids) => {
    try {
      localStorage.setItem(KEY, JSON.stringify(ids));
    } catch {
      /* private mode: favourites last for this session */
    }
    set({ ids });
  },
}));
