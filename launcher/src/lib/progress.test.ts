import { describe, expect, it } from "vitest";
import { cancellable, startLine, STILL_STARTING } from "./progress";

describe("launch progress", () => {
  it("steps through Steam, the game, then still starting", () => {
    expect(startLine("Hades II", { phase: "steam", since: 0 }, 1000)).toBe("Starting Steam");
    expect(startLine("Hades II", { phase: "game", since: 0 }, 1000)).toBe("Starting Hades II");
    expect(startLine("Hades II", { phase: "game", since: 0 }, STILL_STARTING)).toBe("Still starting. B to cancel.");
    expect(startLine("Hades II", { phase: "playing", since: 0 }, 99_000)).toBeNull();
    expect(startLine("Hades II", undefined, 0)).toBeNull();
  });

  it("B cancels the newest slow start only", () => {
    const starts = { "steam:1": { phase: "game" as const, since: 0 }, "emu:2": { phase: "game" as const, since: 5_000 }, "app:x": { phase: "playing" as const, since: 0 } };
    expect(cancellable(starts, 10_000)).toBeNull();
    expect(cancellable(starts, 16_000)).toBe("steam:1");
    expect(cancellable(starts, 21_000)).toBe("emu:2");
  });
});
