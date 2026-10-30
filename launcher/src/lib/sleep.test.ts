import { describe, expect, it } from "vitest";
import { idleLabel, idleLimitMin, nextIdleChoice, shouldSleep } from "./sleep";

const base = { idleMs: 16 * 60_000, limitMin: 15, running: 0, audioPlaying: false, setup: false };

describe("idle sleep", () => {
  it("defaults to 15 minutes on battery and never while charging", () => {
    expect(idleLimitMin({}, true)).toBe(15);
    expect(idleLimitMin({}, false)).toBe(0);
    expect(idleLimitMin({ sleepIdleBattery: 0 }, true)).toBe(0);
    expect(idleLimitMin({ sleepIdleAc: 30 }, false)).toBe(30);
  });
  it("sleeps once idle past the limit", () => {
    expect(shouldSleep(base)).toBe(true);
    expect(shouldSleep({ ...base, idleMs: 14 * 60_000 })).toBe(false);
  });
  it("never sleeps under a game, audio, setup or a zero limit", () => {
    expect(shouldSleep({ ...base, running: 1 })).toBe(false);
    expect(shouldSleep({ ...base, audioPlaying: true })).toBe(false);
    expect(shouldSleep({ ...base, setup: true })).toBe(false);
    expect(shouldSleep({ ...base, limitMin: 0 })).toBe(false);
  });
  it("cycles the Settings choices", () => {
    expect(nextIdleChoice(15)).toBe(30);
    expect(nextIdleChoice(60)).toBe(0);
    expect(nextIdleChoice(7)).toBe(0);
    expect(idleLabel(0)).toBe("Never");
    expect(idleLabel(60)).toBe("1 h");
  });
});
