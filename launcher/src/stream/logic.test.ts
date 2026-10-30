import { describe, expect, it } from "vitest";
import { autoCodec, cycle, homeHost, ordered, parseRunId, pcGames, playState, qualityLine, runId, tileLine, validAddress, DEFAULT_SETTINGS } from "./logic";
import type { Host, StreamApp } from "./types";

const host = (o: Partial<Host> = {}): Host => ({
  uuid: "A",
  name: "Gaming-PC",
  address: "192.168.0.20",
  paired: true,
  online: true,
  currentGame: null,
  canWake: true,
  software: "Sunshine",
  codecs: 0x1 | 0x100 | 0x10000,
  ...o,
});
const elden: StreamApp = { id: 1245620, name: "ELDEN RING", art: "/c/1245620.png", hdr: false };
const desktop: StreamApp = { id: 881448767, name: "Desktop", art: null, hdr: false };

describe("stream state", () => {
  it("round-trips running ids", () => {
    const id = runId("5F6A-71C3", 1093255277);
    expect(id).toBe("stream:5F6A-71C3:1093255277");
    expect(parseRunId(id)).toEqual({ uuid: "5F6A-71C3", app: 1093255277 });
    expect(parseRunId("app:netflix")).toBeNull();
    expect(parseRunId("stream:nope")).toBeNull();
  });

  it("puts the first paired PC that answers on Home", () => {
    expect(homeHost([host({ uuid: "U", paired: false }), host({ uuid: "S", online: false }), host({ uuid: "P" })])?.uuid).toBe("P");
    expect(homeHost([host({ online: false })])).toBeNull();
    expect(homeHost([])).toBeNull();
  });

  it("lists paired PCs' games for the Library, games with art first", () => {
    const g = pcGames([host(), host({ uuid: "B", paired: false })], { A: [desktop, elden], B: [elden] });
    expect(g.map((x) => `${x.host.uuid}:${x.app.id}`)).toEqual(["A:881448767", "A:1245620"]);
    expect(ordered([desktop, elden]).map((a) => a.name)).toEqual(["ELDEN RING", "Desktop"]);
  });

  it("knows what Play does", () => {
    expect(playState(host(), elden, [runId("A", elden.id)])).toBe("playing");
    expect(playState(host({ currentGame: elden.id }), elden, [])).toBe("on-pc");
    expect(playState(host({ currentGame: 42 }), elden, [])).toBe("busy");
    expect(playState(host(), elden, [])).toBe("ready");
    expect(playState(host({ online: false }), elden, [])).toBe("asleep");
    expect(playState(host({ online: false, canWake: false }), elden, [])).toBe("offline");
    // A stream from another PC doesn't count.
    expect(playState(host(), elden, [runId("B", elden.id)])).toBe("ready");
    expect(tileLine(host(), "asleep")).toBe("Gaming-PC is asleep");
    expect(tileLine(host(), "ready")).toBe("Gaming-PC");
    expect(tileLine(host(), "on-pc")).toBe("Running on your PC");
  });

  it("picks the codec both ends support", () => {
    const all = { av1: true, hevc: true, h264: true };
    expect(autoCodec(all, 0x1 | 0x100 | 0x10000)).toBe("av1");
    expect(autoCodec(all, 0x1 | 0x100)).toBe("hevc");
    expect(autoCodec({ av1: false, hevc: false, h264: true }, 0x1 | 0x100)).toBe("h264");
    expect(autoCodec(null, 0x1)).toBeNull();
    expect(autoCodec(all, null)).toBeNull();
    expect(qualityLine(DEFAULT_SETTINGS, all, 0x10101)).toBe("1920 x 1200, 60 fps, 20 Mbps, AV1");
    expect(qualityLine({ ...DEFAULT_SETTINGS, resolution: "1280x800", bitrateMbps: 10, codec: "hevc" }, null, null)).toBe("1280 x 800, 60 fps, 10 Mbps, HEVC");
  });

  it("cycles settings and checks addresses", () => {
    expect(cycle(["10", "20", "30"], "30")).toBe("10");
    expect(cycle(["a", "b"], "a")).toBe("b");
    expect(validAddress("192.168.0.20")).toBe(true);
    expect(validAddress("gaming-pc.local:48989")).toBe(true);
    expect(validAddress("fe80::2")).toBe(true);
    expect(validAddress("pc; rm -rf ~")).toBe(false);
    expect(validAddress(" ")).toBe(false);
  });
});
