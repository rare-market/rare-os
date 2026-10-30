// Rare Wallet shots, agent C: send, send collectibles, the review sheet in its states, the PIN
// sheet and the web-game connect prompt. States that need a pending request start from the
// browser-only window.__rareWalletDev hook (wallet/ui/sign/devHook.ts).
const demo = "0xa0490f6fe09183700e2af6484b7fb2a09a12f0f691f5b8e5582c4782d6de887e";
const pause = (ms) => new Promise((r) => setTimeout(r, ms));
const base = `tab=wallet&nosignin&watch=${demo}&devsign&mockhw`;
const dev = (p, fn, ...args) => p.evaluate(fn, ...args);
const startSend = async (p) => {
  await dev(p, () => void window.__rareWalletDev.sendDemo());
  await pause(5500);
};
const approve = (p) =>
  dev(p, () => {
    const b = [...document.querySelectorAll("[role=dialog] button")].find((x) => x.textContent.trim() === "Approve");
    b && b.click();
  });
const HASH = "9f2c41d87ab0e35c6d18f4a2b7c90e13d5a68f0b2c47e91a3d6b85f0c2e7a419";

export default [
  { name: "wallet-send", q: `${base}&wv=send`, wait: 5000 },
  {
    name: "wallet-send-amount",
    q: `${base}&wv=send`,
    wait: 5000,
    after: async (p) => {
      await dev(p, () => window.__rareWalletDev.openSend("0x" + "ab".repeat(32)));
      await pause(1500);
      for (const k of ["1", ".", "5"]) await p.keyboard.press(k);
      await pause(800);
    },
  },
  {
    name: "wallet-send-review",
    q: `${base}&wv=send`,
    wait: 5000,
    after: async (p) => {
      await startSend(p);
    },
  },
  {
    name: "wallet-send-nfts",
    q: `${base}&wv=send`,
    wait: 5000,
    after: async (p) => {
      await dev(p, () => window.__rareWalletDev.openNfts(3));
      await pause(5000);
    },
  },
  {
    name: "wallet-review-ledger",
    q: `${base}&wv=send`,
    wait: 5000,
    after: async (p) => {
      await dev(p, () => window.__rareWalletDev.hw("ledger"));
      await startSend(p);
      await approve(p);
      await dev(p, () => window.__rareWalletDev.ledger("confirm", "Nano X 4F2A"));
      await pause(800);
    },
  },
  {
    name: "wallet-review-keystone",
    q: `${base}&wv=send`,
    wait: 5000,
    after: async (p) => {
      await dev(p, () => window.__rareWalletDev.hw("keystone"));
      await startSend(p);
      await approve(p);
      await dev(p, (h) => window.__rareWalletDev.keystone({ stage: "confirm", message: "Confirm on your Keystone", digestHex: h, error: null }), HASH);
      await pause(800);
    },
  },
  {
    name: "wallet-review-passkey",
    q: `${base}&wv=send`,
    wait: 5000,
    after: async (p) => {
      await dev(p, () => window.__rareWalletDev.hw("passkey"));
      await startSend(p);
      await approve(p);
      await dev(p, () => window.__rareWalletDev.passkey({ kind: "qr", url: "FIDO:/2848372610295583947261058374920183746502938475610293847561029384756102938475610293847561" }));
      await pause(800);
    },
  },
  {
    name: "wallet-unlock",
    q: `${base}&wv=send`,
    wait: 5000,
    after: async (p) => {
      await dev(p, () => window.__rareWalletDev.unlockSheet("Enter your PIN to approve this send"));
      await pause(600);
      for (const k of ["1", "2", "3"]) await p.keyboard.press(k);
    },
  },
  {
    name: "wallet-connect-game",
    q: `${base}`,
    wait: 5000,
    after: async (p) => {
      await dev(p, () => window.__rareWalletDev.gameRequest({ id: 1, method: "connect", params: {}, origin: "https://arcade.rare.market", game: "Rare Arcade" }));
      await pause(800);
    },
  },
  {
    name: "wallet-review-message",
    q: `${base}`,
    wait: 5000,
    after: async (p) => {
      await dev(p, () => void window.__rareWalletDev.message());
      await pause(1200);
    },
  },
];
