import { describe, expect, it } from "vitest";
import { KIOSK_OWNER_CAP, PERSONAL_KIOSK_CAP, checkRareling, kioskOf, type RarelingReader } from "./detect";

const PKG = `0x${"7".repeat(64)}`;
const TYPE = `${PKG}::rareling::Rareling`;
const OWNER = `0x${"1".repeat(64)}`;

function reader(o: { wallet?: string[]; caps?: Array<Record<string, unknown>>; personal?: Array<Record<string, unknown>>; kiosks?: Record<string, Array<{ objectId: string; type: string }>> }): RarelingReader {
  return {
    owned: async (_owner, type) => {
      if (type === TYPE) return (o.wallet ?? []).map((objectId) => ({ objectId, json: null }));
      if (type === KIOSK_OWNER_CAP) return (o.caps ?? []).map((json, i) => ({ objectId: `cap${i}`, json }));
      if (type === PERSONAL_KIOSK_CAP) return (o.personal ?? []).map((json, i) => ({ objectId: `pcap${i}`, json }));
      return [];
    },
    kioskItems: async (k) => o.kiosks?.[k] ?? [],
  };
}

describe("checkRareling", () => {
  it("is unavailable until the type is set", async () => {
    expect(await checkRareling(OWNER, null, reader({}))).toEqual({ status: "unavailable" });
  });
  it("finds one in the wallet", async () => {
    expect(await checkRareling(OWNER, TYPE, reader({ wallet: ["0xr1", "0xr2"] }))).toEqual({ status: "owned", count: 2, where: "wallet", ids: ["0xr1", "0xr2"] });
  });
  it("finds one locked in a kiosk", async () => {
    const r = reader({ caps: [{ for: "0xk1" }, { for: "0xk2" }], kiosks: { "0xk1": [{ objectId: "0xother", type: "0x9::x::Y" }], "0xk2": [{ objectId: "0xr9", type: TYPE }] } });
    expect(await checkRareling(OWNER, TYPE, r)).toEqual({ status: "owned", count: 1, where: "kiosk", ids: ["0xr9"] });
  });
  it("reads a personal kiosk's wrapped cap", async () => {
    const r = reader({ personal: [{ cap: { vec: [{ for: "0xpk" }] } }], kiosks: { "0xpk": [{ objectId: "0xr3", type: TYPE }] } });
    expect((await checkRareling(OWNER, TYPE, r)).status).toBe("owned");
  });
  it("a look-alike type from another package does not count", async () => {
    const short = `0x${"7".repeat(64)}::rareling::Rareling`;
    const r = reader({ caps: [{ for: "0xk" }], kiosks: { "0xk": [{ objectId: "0xfake", type: `0x${"8".repeat(64)}::rareling::Rareling` }] } });
    expect(await checkRareling(OWNER, short, r)).toEqual({ status: "none" });
  });
  it("kioskOf handles both cap shapes", () => {
    expect(kioskOf({ for: "0xa" })).toBe("0xa");
    expect(kioskOf({ cap: { for: "0xb" } })).toBe("0xb");
    expect(kioskOf({ cap: null })).toBe(null);
    expect(kioskOf(null)).toBe(null);
  });
});
