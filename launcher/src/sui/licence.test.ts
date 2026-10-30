import { describe, expect, it } from "vitest";
import { checkLicence, licenceTypes, type LicenceReader } from "./licence";

const PKG = "0xabc";
const GAME = "0x0000000000000000000000000000000000000000000000000000000000000077";
const { licence: L, resale: R } = licenceTypes(PKG);

const reader = (o: Partial<LicenceReader>): LicenceReader => ({ owned: async () => [], kioskFields: async () => [], objects: async () => [], ...o });

describe("checkLicence (rare_store licence::Licence / ResaleLicence)", () => {
  it("reads the real types, not the old license::License", () => {
    expect(L).toBe("0x0000000000000000000000000000000000000000000000000000000000000abc::licence::Licence");
    expect(R).toBe("0x0000000000000000000000000000000000000000000000000000000000000abc::licence::ResaleLicence");
  });
  it("says unavailable while the package isn't published", async () => {
    expect(await checkLicence("0x1", GAME, null, reader({}))).toEqual({ status: "unavailable" });
  });
  it("finds the soulbound Licence in the wallet", async () => {
    const r = await checkLicence("0x1", "0x77", PKG, reader({ owned: async (_o, t) => (t === L ? [{ objectId: "0xl1", json: { game: GAME, slug: "act", edition: 1, purchased_at_ms: "5" } }] : []) }));
    expect(r).toEqual({ status: "owned", where: "wallet", licence: { id: "0xl1", game: GAME, slug: "act", edition: 1, purchasedAtMs: 5, resale: false } });
  });
  it("finds a ResaleLicence held in the wallet", async () => {
    const r = await checkLicence("0x1", GAME, PKG, reader({ owned: async (_o, t) => (t === R ? [{ objectId: "0xr1", json: { game: GAME, edition: 2 } }] : []) }));
    expect(r).toMatchObject({ status: "owned", where: "wallet", licence: { id: "0xr1", edition: 2, resale: true } });
  });
  it("finds a ResaleLicence inside the owner's kiosk", async () => {
    const r = await checkLicence(
      "0x1",
      GAME,
      PKG,
      reader({
        owned: async (_o, t) => (t.includes("KioskOwnerCap") ? [{ objectId: "0xcap", json: { for: "0xk" } }] : []),
        kioskFields: async () => [{ objectId: "0xl2", type: R }],
        objects: async () => [{ objectId: "0xl2", type: R, json: { game: GAME } }],
      }),
    );
    expect(r).toMatchObject({ status: "owned", where: "kiosk", kioskId: "0xk", licence: { resale: true } });
  });
  it("a licence for another game doesn't count", async () => {
    const r = await checkLicence("0x1", GAME, PKG, reader({ owned: async (_o, t) => (t === L ? [{ objectId: "0xl1", json: { game: "0x78" } }] : []) }));
    expect(r).toEqual({ status: "none" });
  });
  it("the old field name (game_id) is not read", async () => {
    const r = await checkLicence("0x1", GAME, PKG, reader({ owned: async (_o, t) => (t === L ? [{ objectId: "0xl1", json: { game_id: GAME } }] : []) }));
    expect(r).toEqual({ status: "none" });
  });
});
