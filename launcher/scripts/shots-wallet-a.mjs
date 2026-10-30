// Rare Wallet shots, UI agent A: home, coin, collectible, activity detail, receive, accounts,
// settings, reveal phrase, Playtron guide. Uses the demo watch address for real data.
const demo = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const base = `tab=wallet&nosignin&watch=${demo}`;
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const click = (p, text) =>
  p.evaluate((t) => {
    const b = [...document.querySelectorAll("button")].find((x) => x.textContent.trim().startsWith(t));
    b && b.click();
  }, text);

export default [
  { name: "wallet-home", q: base, wait: 6000 },
  { name: "wallet-home-collectibles", q: base, wait: 5000, after: async (p) => { await click(p, "Collectibles"); await pause(3500); } },
  { name: "wallet-home-activity", q: base, wait: 5000, after: async (p) => { await click(p, "Activity"); await pause(9000); } },
  { name: "wallet-coin", q: `${base}&wv=coin/0x2%3A%3Asui%3A%3ASUI`, wait: 6000 },
  { name: "wallet-nft", q: `${base}&wv=nft/0x00307cb705cc7e7232faaee310001003f6774e0b07dad6dc8f18c31216b93284`, wait: 7000 },
  { name: "wallet-activity-detail", q: base, wait: 5000, after: async (p) => { await click(p, "Activity"); await pause(9000); await p.evaluate(() => document.querySelector("[data-nav-group=wallet-activity] button")?.click()); await pause(3500); } },
  { name: "wallet-receive", q: `${base}&wv=receive`, wait: 4000 },
  { name: "wallet-accounts", q: `${base}&wv=accounts`, wait: 5000 },
  { name: "wallet-settings", q: "tab=wallet&nosignin&devphrase&wv=settings", wait: 4000 },
  {
    name: "wallet-reveal-phrase",
    q: "tab=wallet&nosignin&devphrase&wv=settings",
    wait: 5000,
    after: async (p) => {
      await click(p, "Show recovery phrase");
      await pause(800);
      for (const k of "123456") await p.keyboard.press(k);
      await pause(300);
      await click(p, "OK");
      await pause(2500);
    },
  },
  { name: "wallet-playtron", q: `${base}&wv=playtron`, wait: 4000 },
];
