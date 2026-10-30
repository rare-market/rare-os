import { describe, expect, it } from "vitest";
import { NO_UPDATE, agoText, checkDone, toAnnounce, updateLine } from "./osupdate";

const now = 1_800_000_000_000;
const sec = now / 1000;

describe("agoText", () => {
  it("says it in plain words", () => {
    expect(agoText(null, now)).toBe("not yet");
    expect(agoText(sec - 20, now)).toBe("just now");
    expect(agoText(sec - 600, now)).toBe("10 minutes ago");
    expect(agoText(sec - 3600, now)).toBe("1 hour ago");
    expect(agoText(sec - 3 * 3600, now)).toBe("3 hours ago");
    expect(agoText(sec - 30 * 3600, now)).toBe("yesterday");
    expect(agoText(sec - 5 * 86400, now)).toBe("5 days ago");
  });
});

describe("updateLine", () => {
  it("staged wins over everything", () => {
    const l = updateLine({ ...NO_UPDATE, staged: true, stagedVersion: "43.2", checkOk: false }, true, now);
    expect(l.title).toBe("Update 43.2 is ready");
  });
  it("checking, failed, up to date", () => {
    expect(updateLine(NO_UPDATE, true, now).title).toBe("Checking for updates");
    expect(updateLine({ ...NO_UPDATE, checkOk: false, checkError: "offline" }, false, now).body).toContain("offline");
    const ok = updateLine({ ...NO_UPDATE, checkOk: true, checkedAt: sec - 7200 }, false, now);
    expect(ok.title).toBe("Rare OS is up to date");
    expect(ok.body).toContain("2 hours ago");
  });
  it("uses no dashes in its words", () => {
    for (const u of [NO_UPDATE, { ...NO_UPDATE, staged: true }, { ...NO_UPDATE, checkOk: false }]) {
      const l = updateLine(u, false, now);
      expect(l.title + l.body).not.toMatch(/—|–/);
    }
  });
});

describe("toAnnounce", () => {
  it("announces a staged version once per launch", () => {
    const u = { ...NO_UPDATE, staged: true, stagedVersion: "43.2" };
    expect(toAnnounce(u, null)).toBe("43.2");
    expect(toAnnounce(u, "43.2")).toBeNull();
    expect(toAnnounce({ ...u, stagedVersion: "43.3" }, "43.2")).toBe("43.3");
    expect(toAnnounce(NO_UPDATE, null)).toBeNull();
  });
});

describe("checkDone", () => {
  it("needs a newer check time", () => {
    expect(checkDone({ ...NO_UPDATE, checkedAt: 100 }, 200)).toBe(false);
    expect(checkDone({ ...NO_UPDATE, checkedAt: 200 }, 200)).toBe(true);
    expect(checkDone(NO_UPDATE, 0)).toBe(false);
  });
});
