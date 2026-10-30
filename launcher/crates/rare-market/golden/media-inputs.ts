// The golden inputs for the Rust port of the image resolver (rare-market src/media.rs). site-chain.mts
// writes media.json from rare.market's own sources (packages/venues/src/media.ts and the site's
// web/lib/media.ts chain); media.test.ts checks the web launcher's copy of the resolver against it.

import { Buffer } from "node:buffer";

export const CID0 = "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG";
const CID1 = "bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi";
const BLOB = "M4hsZGQ1oCktdzegB6HnI6Mi28S2nqOPHxK-W7_4BUk";
const PATCH = "M4hsZGQ1oCktdzegB6HnI6Mi28S2nqOPHxK-W7_4BUkAQEBAQ";
const AR = "bNbA3TEQVL60xlgCcqdz4ZPHFZ711cJ3hmkzA5gjsVs";

export const INPUTS: string[] = [
  "",
  "null",
  "  'https://example.com/a.png'  ",
  "http://example.com/a.png",
  "https://asset.birds.dog/img/1/2.png",
  "https://walrus.tusky.io/abc",
  `ipfs://${CID0}/1.png`,
  `ipfs://ipfs/${CID1}`,
  `ipfs:${CID0}`,
  `/ipfs/${CID0}//x.png`,
  `${CID1}/meta.json`,
  "ipfs://notacid",
  "ipns://k51qzi5uqu5d/x",
  `https://ipfs.io/ipfs/${CID0}/a%20b.png?x=1#frag`,
  `https://mygateway.example:8443/ipfs/${CID0}`,
  `https://${CID1}.ipfs.dweb.link/1.png`,
  `walrus://${BLOB}`,
  `walrus://${PATCH}`,
  `https://aggregator.walrus-mainnet.walrus.space/v1/blobs/${BLOB}`,
  `https://aggregator.walrus-mainnet.walrus.space/v1/blobs/by-quilt-patch-id/${PATCH}`,
  `https://aggregator.walrus-mainnet.walrus.space/v1/${BLOB}`,
  `https://wal.gg/${BLOB}`,
  `https://walrus.tusky.io/${BLOB}`,
  BLOB,
  `ar://${AR}`,
  `https://arweave.net/${AR}?ext=png`,
  `https://abc.arweave.net/${AR}`,
  "https://arweave.net/short",
  "//cdn.example.com/x.jpg",
  "cdn.example.com/x.jpg",
  "ftp://example.com/x.png",
  "https://user:pw@example.com/x.png",
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==",
  "data:application/json;base64," + Buffer.from(JSON.stringify({ name: "x", image: `ipfs://${CID0}` })).toString("base64"),
  "data:application/json," + encodeURIComponent(JSON.stringify({ image_url: "https://example.com/i.webp" })),
  "<svg xmlns='http://www.w3.org/2000/svg'><rect/></svg>",
  "https://example.com/placeholder.png",
  "https://example.com/images/null.png",
  "https://img.example.xyz/a.png",
  "https://thebirds.ai/a.png",
  "/img/w/abc.webp",
  "/img/pass?url=" + encodeURIComponent("https://img.example.fun/a.avif"),
  '"[https://example.com/q.gif],"',
  "https://example.com/a.png​",
  "https://EXAMPLE.com/A.PNG",
  // rare.market API paths (own renditions) and a collection logo on ipfs:// (Doonies).
  "/img/collection/0xc90581173eee14f2bdc9429a15278daa0c6fe51b4d3c488bafbdb5a783f2d22c%3A%3Adeadskulls%3A%3ANFT/full",
  "/img/item/0xac5a0ee449ddf5a05b3dbc46060cd09e2a1357fd409051817f6c94e6c50d863e/thumb",
  "/img/data/0xac5a0ee449ddf5a05b3dbc46060cd09e2a1357fd409051817f6c94e6c50d863e/9b594ec8f17ba7c9.svg",
  "ipfs://bafkreibscwog6huz2pwwbedhrx5lqrzkemtrmfdw5ccu5qfadz6ox6keua",
  "https://storage.claynosaurz.com/popkins/images/0x817dac94fff3a567b80e23a3b55fca2fd108641e0dd06fc614e5ecc0acbb4e03",
];

export const JSONS: Array<Record<string, unknown>> = [
  { image_url: "https://e.com/1.png" },
  { url: "https://e.com/page" , thumbnail: "https://e.com/t.jpg" },
  { animation_url: "https://e.com/v.mp4", image: "null", img_url: `ipfs://${CID0}` },
  { metadata: { image_uri: "https://e.com/m.png" } },
  { contents: ["data:image/svg+xml;base64," + "A".repeat(80)] },
  { name: "none" },
];

