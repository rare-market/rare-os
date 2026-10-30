import { beforeEach, describe, expect, it } from "vitest";
import { applyNetworkConfig, currentNetwork, DEFAULT_RPC, rpcOk, rpcUrl } from "./network";

describe("network setting", () => {
  beforeEach(() => applyNetworkConfig({}));

  it("is mainnet until set", () => {
    expect(currentNetwork()).toBe("mainnet");
    expect(rpcUrl("mainnet")).toBe(DEFAULT_RPC.mainnet);
  });

  it("switches to testnet with the public RPC", () => {
    applyNetworkConfig({ network: "testnet" });
    expect(currentNetwork()).toBe("testnet");
    expect(rpcUrl("testnet")).toBe(DEFAULT_RPC.testnet);
  });

  it("takes a testnet RPC override, for testnet only", () => {
    applyNetworkConfig({ network: "testnet", testnetRpc: "http://localhost:9000/" });
    expect(rpcUrl("testnet")).toBe("http://localhost:9000");
    expect(rpcUrl("mainnet")).toBe(DEFAULT_RPC.mainnet);
  });

  it("ignores an unsafe override", () => {
    applyNetworkConfig({ network: "testnet", testnetRpc: "http://example.com" });
    expect(rpcUrl("testnet")).toBe(DEFAULT_RPC.testnet);
    expect(rpcOk("not a url")).toBe(false);
  });
});
