// Emulator rows for the Rare menu (backend: src-tauri/src/emu/actions.rs).
// `emuActions(gameId)` lists the rows the game's emulator can do (empty for anything else),
// `emuDo(action)` does one. Keyboard rows close the overlay first so the game has focus.

export interface EmuAction {
  appId: string;
  id: "save" | "load" | "menu";
  label: string;
  emulator: string;
  /** False when the emulator's config lacks the binding; `note` says why. */
  ready: boolean;
  note: string | null;
}

async function call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export const emuActions = (appId: string) => call<EmuAction[]>("emu_actions", { appId });
export const emuDo = (action: EmuAction) => call<void>("emu_do", { action });
