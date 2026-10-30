// Rare Wallet shots, UI agent B: welcome, sign-in overlay, passkey, new phrase, import, watch,
// add account, Ledger, Keystone. Empty wallet, with simulated hardware (&mockhw).
const base = "tab=wallet&nosignin&mockhw";
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const click = (p, text) =>
  p.evaluate((t) => {
    const b = [...document.querySelectorAll("button")].find((x) => x.textContent.trim().startsWith(t));
    b && b.click();
  }, text);
// The soft authenticator waits 700 ms between steps; never firing that timer holds a ceremony on screen.
const freeze = (p) => p.evaluate(() => { const o = window.setTimeout; window.setTimeout = (f, ms, ...a) => (ms === 700 ? 0 : o(f, ms, ...a)); });
const showWords = async (p) => {
  await click(p, "Show my words");
  await pause(500);
};
const toQuiz = async (p) => {
  await showWords(p);
  await click(p, "I wrote it down");
  await pause(500);
};
// Reads the phrase off the page and answers the three questions.
const solveQuiz = async (p) => {
  const words = await p.evaluate(() => [...document.querySelectorAll("ol li span:last-child")].map((s) => s.textContent));
  await click(p, "I wrote it down");
  await pause(400);
  for (let i = 0; i < 3; i++) {
    const n = await p.evaluate(() => Number(/#(\d+)/.exec(document.body.innerText)?.[1]));
    const w = words[n - 1];
    await p.evaluate((x) => [...document.querySelectorAll("button")].find((b) => b.textContent.trim() === x)?.click(), w);
    await pause(400);
  }
  await pause(500);
};

export default [
  { name: "wallet-welcome", q: "tab=wallet&nosignin", fresh: true },
  { name: "wallet-signin-overlay", q: "tab=home", fresh: true },
  { name: "wallet-passkey-qr", q: `${base}&wv=passkey/signin`, after: async (p) => { await freeze(p); await click(p, "Continue"); await pause(800); } },
  { name: "wallet-passkey-pin", q: `${base}&wv=passkey/signin`, after: async (p) => {
      await freeze(p);
      await click(p, "Use a security key");
      await click(p, "Continue");
      await pause(1500);
      await p.evaluate(async () => { const u = performance.getEntriesByType("resource").map((e) => e.name).find((n) => /wallet\/passkey\/index\.ts/.test(n)); const m = await import(u); m.ceremonyStore.setState({ state: { kind: "pin", attemptsLeft: 7 } }); });
      await pause(400);
    } },
  { name: "wallet-create-phrase", q: `${base}&wv=create-phrase`, after: showWords },
  { name: "wallet-create-verify", q: `${base}&wv=create-phrase`, after: toQuiz },
  {
    name: "wallet-set-pin",
    q: `${base}&wv=create-phrase`,
    after: async (p) => {
      await showWords(p);
      await solveQuiz(p);
      for (const k of "123456") await p.keyboard.press(k);
      await pause(400);
    },
  },
  { name: "wallet-import-phrase", q: `${base}&wv=import/phrase` },
  { name: "wallet-import-key", q: `${base}&wv=import/key` },
  { name: "wallet-watch", q: `${base}&wv=watch` },
  { name: "wallet-add", q: `${base}&wv=add` },
  { name: "wallet-ledger", q: `${base}&wv=ledger` },
  { name: "wallet-ledger-accounts", q: `${base}&wv=ledger`, wait: 3000, after: async (p) => { await click(p, "Nano S Plus"); await pause(6000); } },
  { name: "wallet-ledger-udev", q: `${base}&mockudev&wv=ledger` },
  { name: "wallet-keystone", q: `${base}&wv=keystone`, wait: 3000 },
  { name: "wallet-firstrun-passkey", q: "tab=home&firstrun&mockhw&step=2", fresh: true, wait: 3000 },
  { name: "wallet-firstrun-passkey-qr", q: "tab=home&firstrun&mockhw&step=2", fresh: true, wait: 3000, after: async (p) => { await freeze(p); await click(p, "Sign in with a passkey"); await pause(600); await click(p, "Continue"); await pause(800); } },
];
