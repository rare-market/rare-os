// End-to-end Share on Walrus TESTNET: a real WebP screenshot and a real H.264 clip (with its
// poster) go up as quilts through the upload relay, paid by a testnet key, then every file is read
// back from the public aggregator by quilt id and identifier and compared byte for byte. Prints
// the links, sizes and what was really spent. Never touches mainnet or real funds.
//
//   RARE_WALRUS_E2E=1 RARE_WALRUS_E2E_KEYS=<keys.json> RARE_SHARE_FIXTURES=<dir> \
//     npx vitest run src/share/share.e2e.test.ts
//
// Fixtures stay out of git. Make them with ffmpeg:
//   ffmpeg -f lavfi -i "testsrc2=size=1920x1080:rate=1,noise=alls=12:allf=t" -frames:v 1 -c:v libwebp -quality 88 shot.webp
//   ffmpeg -f lavfi -i testsrc2=size=1920x1080:rate=60 -t 30 -c:v libx264 -pix_fmt yuv420p -b:v 4M -movflags +faststart clip.mp4
//   ffmpeg -ss 1 -i clip.mp4 -frames:v 1 -vf scale=1280:-2 -q:v 4 poster.jpg
// (In the app the poster is grabbed in the webview, src/share/poster.ts.) RARE_SHARE_E2E_OUT
// appends the results to a file.

import { appendFileSync, existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { walrusClient } from "../sui/saves/walrus";
import { balances, keypairSigner, swapForWal, testKey } from "../sui/saves/testnet";
import { buildShareJson, shareEpochs, type DurationId } from "./format";
import type { Capture } from "./native";
import { quiltFileUrl, quiltSize, shareEstimate, shareFiles, uploadShare } from "./upload";

const dir = process.env.RARE_SHARE_FIXTURES ?? "";
const run = process.env.RARE_WALRUS_E2E === "1" && !!dir && existsSync(join(dir, "shot.webp")) && existsSync(join(dir, "clip.mp4"));
const out = process.env.RARE_SHARE_E2E_OUT;

const fmt = (n: bigint) => (Number(n) / 1e9).toFixed(6);
function log(line: string) {
  console.log(line);
  if (out) appendFileSync(out, `${line}\n`);
}

async function fetchBack(url: string): Promise<Uint8Array> {
  let last = "";
  for (let i = 0; i < 20; i++) {
    const r = await fetch(url).catch((e) => ({ ok: false, status: String(e) }) as const);
    if (r.ok) return new Uint8Array(await (r as Response).arrayBuffer());
    last = String(r.status);
    await new Promise((res) => setTimeout(res, 3000));
  }
  throw new Error(`${url}: ${last}`);
}

async function shareOne(kind: Capture["kind"], duration: DurationId) {
  const kp = testKey();
  const signer = keypairSigner(kp);
  const before = await balances(signer.address);
  if (before.wal < 300_000_000n) {
    await swapForWal(kp);
    Object.assign(before, await balances(signer.address));
  }

  const media = new Uint8Array(readFileSync(join(dir, kind === "clip" ? "clip.mp4" : "shot.webp")));
  const poster = kind === "clip" ? new Uint8Array(readFileSync(join(dir, "poster.jpg"))) : null;
  const capture: Capture = {
    id: `e2e-${kind}`,
    kind,
    path: "",
    mime: kind === "clip" ? "video/mp4" : "image/webp",
    bytes: media.length,
    width: 1920,
    height: 1080,
    durationMs: kind === "clip" ? 30_000 : null,
    createdMs: Date.now(),
    game: kind === "clip" ? { id: "steam:1145350", name: "Hades II" } : { id: "web:act", name: "ACT" },
  };
  const json = buildShareJson(capture, { bytes: media.length, slug: kind === "clip" ? null : "act", poster: !!poster });
  const files = shareFiles(json, media, poster);
  const size = await quiltSize("testnet", files);
  const { epochs, days } = shareEpochs(duration, "testnet");
  const est = await shareEstimate("testnet", size, duration === "year" ? 365 : 30);
  expect(est.epochs).toBe(epochs);

  const steps: string[] = [];
  const t0 = Date.now();
  const r = await uploadShare("testnet", signer, files, { kind, epochs, onStep: (s) => steps.push(s.step) });
  const secs = ((Date.now() - t0) / 1000).toFixed(1);
  expect(steps).toEqual(["encode", "register", "upload", "certify", "done"]);
  const after = await balances(signer.address);

  const attrs = await walrusClient("testnet").walrus.readBlobAttributes({ blobObjectId: r.blobObjectId! });
  expect(attrs).toMatchObject({ app: "rare-share", v: "1", kind, _walrusBlobType: "quilt" });
  const obj = await walrusClient("testnet").core.getObject({ objectId: r.blobObjectId!, include: { json: true } });
  const deletable = (obj.object.json as { deletable?: boolean } | null)?.deletable;
  expect(deletable).toBe(false);

  const got: Record<string, Uint8Array> = {};
  for (const f of files) {
    const back = (got[f.identifier] = await fetchBack(quiltFileUrl("testnet", r.quiltId, f.identifier)));
    expect(back.length, f.identifier).toBe(f.bytes.length);
    expect(Buffer.from(back).equals(Buffer.from(f.bytes)), f.identifier).toBe(true);
  }
  expect(JSON.parse(new TextDecoder().decode(got["share.json"]))).toEqual(json);

  const sum = files.reduce((a, f) => a + f.bytes.length, 0);
  log(`${kind} (${duration}, ${epochs} testnet epochs = ${days} days, ${deletable === false ? "permanent" : "deletable"})`);
  log(`  link       ${r.link}`);
  log(`  quilt id   ${r.quiltId}`);
  log(`  blob obj   ${r.blobObjectId}`);
  log(`  files      ${files.map((f) => `${f.identifier} ${f.bytes.length}`).join(", ")} (sum ${sum})`);
  log(`  quilt      ${size} bytes`);
  log(`  estimate   ${fmt(est.walFrost)} WAL + ${fmt(est.tipMist + est.gasMist)} SUI (tip ${fmt(est.tipMist)}, gas ${fmt(est.gasMist)})`);
  log(`  spent      ${fmt(before.wal - after.wal)} WAL + ${fmt(before.sui - after.sui)} SUI (balance diff)`);
  log(`  read back  ${files.map((f) => f.identifier).join(", ")} byte-identical from ${quiltFileUrl("testnet", r.quiltId, "")}<identifier>`);
  log(`  took       ${secs} s`);
  return r;
}

describe.skipIf(!run)("Share on Walrus testnet", () => {
  it("shares a WebP screenshot for 1 month", async () => {
    if (out) appendFileSync(out, `\n# ${new Date().toISOString()} Share e2e, Walrus testnet\n`);
    await shareOne("screenshot", "month");
  }, 600_000);
  it("shares a 30 s H.264 clip with its poster for 1 month", async () => {
    await shareOne("clip", "month");
  }, 900_000);
});
