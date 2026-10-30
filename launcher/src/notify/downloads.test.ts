import { describe, expect, it } from "vitest";
import { downloadFraction, emuToAnnounce, finishedDownloads, type Download } from "./downloads";

const d = (id: string, done: number, total = 100, paused = false): Download => ({ id, title: id, done, total, paused });

describe("downloads", () => {
  it("reads progress", () => {
    expect(downloadFraction(d("a", 50))).toBe(0.5);
    expect(downloadFraction(d("a", 0, 0))).toBeNull();
  });
  it("finishes only a download that reached the end and vanished", () => {
    expect(finishedDownloads([d("a", 100), d("b", 40), d("c", 99, 100, true)], [d("c", 99, 100, true)]).map((x) => x.id)).toEqual(["a"]);
    expect(finishedDownloads([d("a", 100)], [d("a", 100)])).toEqual([]);
  });
});

describe("emulator updates", () => {
  it("announces a release once", () => {
    expect(emuToAnnounce("rpcs3", "v1", true, [])).toBe("rpcs3@v1");
    expect(emuToAnnounce("rpcs3", "v1", true, ["rpcs3@v1"])).toBeNull();
    expect(emuToAnnounce("rpcs3", "v1", false, [])).toBeNull();
  });
});
