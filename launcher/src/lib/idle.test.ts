import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { activityAt, createActivity, dueIn, msToNextMinute, pollWith, type Activity } from "./idle";

describe("activityAt", () => {
  const s = { lastInput: 0, visible: true, focused: true };
  it("is active within the idle time, idle after it", () => {
    expect(activityAt(s, 59_999, 60_000)).toBe("active");
    expect(activityAt(s, 60_000, 60_000)).toBe("idle");
  });
  it("is away when hidden or another window has focus, whatever the input", () => {
    expect(activityAt({ ...s, visible: false }, 10, 60_000)).toBe("away");
    expect(activityAt({ ...s, focused: false }, 10, 60_000)).toBe("away");
    expect(activityAt({ ...s, focused: false }, 600_000, 60_000)).toBe("away");
  });
});

describe("the activity machine", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("goes idle after a minute without input, with one timer and no polling", () => {
    const m = createActivity({ idleMs: 60_000 });
    const seen: Activity[] = [];
    m.subscribe((a) => seen.push(a));
    expect(m.get()).toBe("active");
    vi.advanceTimersByTime(59_000);
    expect(m.get()).toBe("active");
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(1_000);
    expect(m.get()).toBe("idle");
    // Idle: nothing left scheduled.
    expect(vi.getTimerCount()).toBe(0);
    expect(seen).toEqual(["idle"]);
  });

  it("input pushes idle back and wakes it at once", () => {
    const m = createActivity({ idleMs: 60_000 });
    vi.advanceTimersByTime(50_000);
    m.input();
    vi.advanceTimersByTime(50_000);
    expect(m.get()).toBe("active");
    vi.advanceTimersByTime(10_000);
    expect(m.get()).toBe("idle");
    m.input();
    expect(m.get()).toBe("active");
  });

  it("a game in front is away; a press brings it back", () => {
    const m = createActivity({ idleMs: 60_000 });
    m.setFocused(false);
    expect(m.get()).toBe("away");
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(600_000);
    expect(m.get()).toBe("away");
    // Pad presses only reach the page while it has the screen.
    m.input();
    expect(m.get()).toBe("active");
    m.setVisible(false);
    expect(m.get()).toBe("away");
    m.setVisible(true);
    expect(m.get()).toBe("active");
  });

  it("focus back after a long time away lands on idle, not active", () => {
    const m = createActivity({ idleMs: 60_000 });
    m.setFocused(false);
    vi.advanceTimersByTime(120_000);
    m.setFocused(true);
    expect(m.get()).toBe("idle");
  });
});

describe("polls", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());
  const cadence = { active: 20_000, idle: 120_000, away: null };

  it("dueIn follows the state's interval and stops away", () => {
    expect(dueIn(0, 5_000, cadence, "active")).toBe(15_000);
    expect(dueIn(0, 25_000, cadence, "active")).toBe(0);
    expect(dueIn(0, 5_000, cadence, "idle")).toBe(115_000);
    expect(dueIn(0, 5_000, cadence, "away")).toBeNull();
  });

  it("slows down idle, stops away, and catches up on return", () => {
    const m = createActivity({ idleMs: 60_000 });
    const fn = vi.fn();
    pollWith(m, fn, cadence);
    vi.advanceTimersByTime(60_000);
    expect(fn).toHaveBeenCalledTimes(2); // 20 and 40 s; idle from 60 s
    expect(m.get()).toBe("idle");
    vi.advanceTimersByTime(99_000);
    expect(fn).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(1_000);
    expect(fn).toHaveBeenCalledTimes(3); // 2 minutes after the last run (160 s)
    m.setFocused(false);
    vi.advanceTimersByTime(600_000);
    expect(fn).toHaveBeenCalledTimes(3);
    // Back in front with a press: overdue for the active cadence, runs now.
    m.input();
    vi.advanceTimersByTime(0);
    expect(fn).toHaveBeenCalledTimes(4);
    vi.advanceTimersByTime(20_000);
    expect(fn).toHaveBeenCalledTimes(5);
  });

  it("a poll that is not due yet waits on return", () => {
    const m = createActivity({ idleMs: 60_000 });
    const fn = vi.fn();
    pollWith(m, fn, cadence);
    vi.advanceTimersByTime(5_000);
    m.setFocused(false);
    m.setFocused(true);
    vi.advanceTimersByTime(14_000);
    expect(fn).toHaveBeenCalledTimes(0);
    vi.advanceTimersByTime(1_000);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("stops for good, and a throwing poll keeps its schedule", () => {
    const m = createActivity({ idleMs: 600_000 });
    const fn = vi.fn(() => {
      throw new Error("offline");
    });
    const stop = pollWith(m, fn, cadence);
    vi.advanceTimersByTime(40_000);
    expect(fn).toHaveBeenCalledTimes(2);
    stop();
    vi.advanceTimersByTime(100_000);
    expect(fn).toHaveBeenCalledTimes(2);
  });
});

describe("the clock", () => {
  it("wakes just after the next whole minute", () => {
    expect(msToNextMinute(0)).toBe(60_050);
    expect(msToNextMinute(59_000)).toBe(1_050);
    expect(msToNextMinute(125_000)).toBe(55_050);
  });
});
