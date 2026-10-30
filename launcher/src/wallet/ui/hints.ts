import { create } from "zustand";
import type { Hint } from "../../components/Chrome";

/** Bottom-bar hints for the wallet tab's top page; null = the launcher default. Its own file so the shell can read it without loading the wallet. */
export const useWalletHints = create<{ hints: Hint[] | null }>(() => ({ hints: null }));
