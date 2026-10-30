import { existsSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { APPS, appByRunId, controllerOf, controlsFor, installCommand, runId, statusFor, wayKind } from "./catalog";

const pub = (p: string) => new URL(`../../public/${p}`, import.meta.url);

describe("apps catalog", () => {
  it("has the media row, each app valid", () => {
    expect(APPS.map((a) => a.id)).toEqual(expect.arrayContaining(["steam", "telegram", "x", "netflix", "chrome", "youtube", "discord"]));
    expect(new Set(APPS.map((a) => a.id)).size).toBe(APPS.length);
    for (const a of APPS) {
      expect(existsSync(pub(a.icon)), `${a.id} icon ${a.icon}`).toBe(true);
      expect(a.ways.length, a.id).toBeGreaterThan(0);
      for (const w of a.ways) {
        const k = wayKind(w);
        expect(k, `${a.id}: a way sets exactly one kind`).not.toBeNull();
        expect(w.label, a.id).toBeTruthy();
        if (a.drm) expect(["chromeApp", "chrome"], `${a.id}: DRM opens in Chrome`).toContain(k);
        if (controllerOf(w) === "focus") expect(k, `${a.id}: focus is for Rare's webview`).toBe("webview");
        for (const u of [w.webview, w.chromeApp].filter(Boolean)) expect(u!.startsWith("https://"), u).toBe(true);
      }
    }
    expect(existsSync(pub("apps/NOTICE"))).toBe(true);
  });

  it("maps run ids", () => {
    expect(runId("netflix")).toBe("app:netflix");
    expect(appByRunId("app:netflix")?.name).toBe("Netflix");
    expect(appByRunId("steam:1")).toBeUndefined();
  });

  it("plans like the native side", () => {
    const get = (id: string) => APPS.find((a) => a.id === id)!;
    // Netflix needs Chrome with Widevine; Chromium is not enough.
    const n = statusFor(get("netflix"), ["org.chromium.Chromium"]);
    expect(n.way).toBeNull();
    expect(n.offer).toEqual({ flatpak: "com.google.Chrome", name: "Google Chrome", command: "flatpak install --user -y flathub com.google.Chrome" });
    expect(statusFor(get("netflix"), ["com.google.Chrome"])).toMatchObject({ way: 0, opensIn: "Google Chrome", controller: "pointer", offer: null });
    // Telegram: the desktop app when installed, else Telegram Web with the app on offer.
    expect(statusFor(get("telegram"), [])).toMatchObject({ way: 1, opensIn: "Telegram Web", controller: "focus", offer: { flatpak: "org.telegram.desktop" } });
    expect(statusFor(get("telegram"), ["org.telegram.desktop"])).toMatchObject({ way: 0, opensIn: "Telegram Desktop", otherWays: [1], offer: null });
    // X opens in Rare; Chrome becomes a second way once installed.
    expect(statusFor(get("x"), []).otherWays).toEqual([]);
    expect(statusFor(get("x"), ["com.google.Chrome"]).otherWays).toEqual([1]);
    expect(installCommand("org.telegram.desktop", false)).toBe(
      "flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo && flatpak install --user -y flathub org.telegram.desktop",
    );
  });

  it("describes the controls", () => {
    const back = (id: string) => controlsFor("pointer", APPS.find((a) => a.id === id)!.ways[0]).find((r) => r.b === "b")!.does;
    expect(back("netflix")).toBe("Back a page");
    expect(back("telegram")).toBe("Back (Escape)");
    expect(controlsFor("focus").map((r) => r.b)).toContain("select");
  });
});
