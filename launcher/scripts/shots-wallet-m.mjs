// Rare Wallet shots, marketplace buy: the in-app buy sheet and the wallet review sheet over it.
// Uses the demo watch address with ?devsign (dry run only, nothing is signed or submitted).
// BUY_ITEM picks the listed item (listings move: pick a new one under 50 SUI if it sells).
const demo = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const item = process.env.BUY_ITEM ?? "0xe75d0290227ebb480d29602a42d4ae7c3c1f8000a571c221d7a136d2602cfd1c";
const q = `tab=market&nosignin&watch=${demo}&devsign&mk=item/${item}`;
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
// Enter on the focused button once it has the given text (the page lands focus after data loads).
const enterOn = async (p, text) => {
  await p.waitForFunction((t) => document.activeElement?.textContent?.trim().startsWith(t), { timeout: 20000 }, text).catch(() => {});
  await p.keyboard.press("Enter");
};

export default [
  { name: "wallet-buy", q, wait: 6000, after: async (p) => { await enterOn(p, "Buy now"); await pause(1500); } },
  {
    name: "wallet-buy-review",
    q,
    wait: 6000,
    after: async (p) => {
      await enterOn(p, "Buy now");
      await enterOn(p, "Buy for");
      // the review sheet simulates over gRPC, then lands on Approve
      await p.waitForFunction(() => document.activeElement?.textContent?.trim() === "Approve", { timeout: 30000 }).catch(() => {});
      await pause(1500);
    },
  },
];
