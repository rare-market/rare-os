// Cross-check vectors between the TypeScript signing review (launcher/src/wallet/review,
// launcher/src/wallet/tx/amount.ts) and the Rust rare-wallet crate (src/review, src/amount.rs).
// This file runs the TS code over fixed inputs and compares the result with review-vectors.json;
// the Rust tests (src/review/tests_vectors.rs) read the same JSON (inputs and outputs) and must
// produce the same rows, risks, insights and strings. Bigints are decimal strings; null fields of
// outputs are left out.
//
//   npx vitest run crates/rare-wallet/testdata/review-vectors.test.ts                      check
//   RARE_WRITE_VECTORS=1 npx vitest run crates/rare-wallet/testdata/review-vectors.test.ts   rewrite

import { describe, expect, it } from "vitest";
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { analyze, callLabel, friendlyFailure, normType, SUI_TYPE, type MoveCallRef } from "../../../src/wallet/review/analyze";
import { describeReview, type CoinMeta } from "../../../src/wallet/review/describe";
import { addressChecks, lookalikeShort, riskChecks, type AddressHistory, type RiskContext, type TokenFlags } from "../../../src/wallet/review/risk";
import { knownFee, splitFees } from "../../../src/wallet/review/fees";
import { humanize, knownPackage, objectLabel, parseType, shortId, coinTypeOf, isCoinObject } from "../../../src/wallet/review/packages";
import type { SimFailure, SimObjectChange, Simulation } from "../../../src/wallet/review/types";
import { baseToUsd, chipAmount, cleanAmount, formatAmount, formatUnits, parseUnits, price, toInput, toNumber, usd, usdToBase, type Chip } from "../../../src/wallet/tx/amount";

const FILE = fileURLToPath(new URL("./review-vectors.json", import.meta.url));

const ADDR = (b: string) => "0x" + b.repeat(32);
const ME = ADDR("a1");
const BOB = ADDR("b2");
const SPONSOR = ADDR("c3");
const MARKET = ADDR("d4");
const GAS = ADDR("11");
const NFT = ADDR("22");
const NFT2 = ADDR("23");
const USDC = normType("0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC");
const CETUS = "0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb";
const ROUTER_FEE = "0x1dd4889f1ba614d0b44bd0fda31dedbb734d4edf8f25254f7d6b69671903ff06";
const NFT_T = ADDR("e5") + "::popkins::Popkins";
const FIELD_T = "0x2::dynamic_field::Field<0x2::dynamic_object_field::Wrapper<0x2::kiosk::Item>, 0x2::object::ID>";
const MY_KIOSK = ADDR("0a");
const NEW_KIOSK = ADDR("0b");
const CAP = ADDR("0c");
const SELLER_KIOSK = ADDR("5e");
const F_IN = ADDR("f1");
const F_OUT = ADDR("f2");
const FEE = 1_500_000n;
const SUI = "0x2::sui::SUI";
const own = (address: string) => ({ kind: "address" as const, address });
const objOwner = (id: string) => ({ kind: "object" as const, id });
const obj = (o: Partial<SimObjectChange> & { objectId: string }): SimObjectChange => ({
  type: NFT_T,
  inputOwner: own(ME),
  outputOwner: own(ME),
  idOperation: "None",
  inputExists: true,
  outputExists: true,
  outputKind: "object",
  ...o,
});
const sim = (o: Partial<Simulation>): Simulation => ({
  source: "node",
  status: "success",
  balanceChanges: [],
  objects: [],
  gas: { computation: "1000000", storage: "2000000", rebate: "1500000" },
  gasOwner: ME,
  gasObjectId: GAS,
  ...o,
});
const fee = (amount = -FEE) => ({ coinType: SUI, address: ME, amount: String(amount) });
const META: Record<string, CoinMeta> = { [SUI_TYPE]: { symbol: "SUI", decimals: 9 }, [USDC]: { symbol: "USDC", decimals: 6 } };

interface RiskIn {
  prices?: Record<string, number | null>;
  decimals?: Record<string, number>;
  symbols?: Record<string, string>;
  portfolioUsd?: number | null;
  safety?: Record<string, TokenFlags | null>;
  originals?: Record<string, string>;
  addresses?: AddressHistory;
  self?: string;
}

interface Case {
  name: string;
  sim: Simulation;
  signer: string;
  calls?: MoveCallRef[];
  kiosks?: string[];
  meta?: Record<string, CoinMeta>;
  extras?: { coveredByRare?: boolean; paidBy?: string };
  risk?: RiskIn;
}

const kioskMove = (outputOwner: SimObjectChange["outputOwner"]) =>
  sim({
    balanceChanges: [fee()],
    objects: [
      obj({ objectId: NFT, inputOwner: objOwner(F_IN), outputOwner }),
      obj({ objectId: F_IN, type: FIELD_T, inputOwner: objOwner(MY_KIOSK), outputOwner: null, outputExists: false, idOperation: "Deleted" }),
      obj({ objectId: F_OUT, type: FIELD_T, inputOwner: null, inputExists: false, idOperation: "Created", outputOwner: objOwner(NEW_KIOSK) }),
      obj({ objectId: MY_KIOSK, type: "0x2::kiosk::Kiosk", inputOwner: { kind: "shared" }, outputOwner: { kind: "shared" } }),
    ],
  });

const CASES: Case[] = [
  {
    name: "coin send",
    signer: ME,
    meta: META,
    sim: sim({ balanceChanges: [fee(-2_500_000_000n - FEE), { coinType: SUI, address: BOB, amount: "2500000000" }] }),
    risk: { addresses: { sentTo: [], receivedFrom: [], contacts: [] }, self: ME },
  },
  {
    name: "token send by its decimals",
    signer: ME,
    meta: META,
    sim: sim({ balanceChanges: [{ coinType: USDC, address: ME, amount: "-12345678" }, fee()] }),
  },
  {
    name: "nft out with display",
    signer: ME,
    meta: META,
    sim: sim({ balanceChanges: [fee()], objects: [obj({ objectId: NFT, outputOwner: own(BOB) })], display: { [NFT]: { name: "Popkins #8475", image: "https://x/1.png" } } }),
    risk: { addresses: { sentTo: [BOB], receivedFrom: [], contacts: [] } },
  },
  {
    name: "kiosk purchase",
    signer: ME,
    meta: META,
    sim: sim({
      balanceChanges: [fee(-87_000_000_000n - FEE), { coinType: SUI, address: BOB, amount: "85000000000" }, { coinType: SUI, address: MARKET, amount: "2000000000" }],
      objects: [
        obj({ objectId: NFT, inputOwner: objOwner(ADDR("99")), outputOwner: own(ME) }),
        obj({ objectId: "0xk1", type: "0x2::kiosk::Kiosk", inputOwner: { kind: "shared" }, outputOwner: { kind: "shared" } }),
      ],
    }),
    calls: [{ package: "0xdfb4f1d4e43e0c3ad834dcd369f0d39005c872e118c9dc1c5da9765bb93ee5f3", module: "kiosk_lock_rule", function: "prove" }],
    risk: { prices: { [SUI_TYPE]: 3 }, portfolioUsd: 1000, addresses: { sentTo: [], receivedFrom: [], contacts: [] }, self: ME },
  },
  { name: "kiosk item unknown kiosks", signer: ME, meta: META, sim: kioskMove(objOwner(F_OUT)) },
  { name: "kiosk item into a new kiosk", signer: ME, meta: META, kiosks: [MY_KIOSK], sim: kioskMove(objOwner(F_OUT)) },
  {
    name: "kiosk item into a kiosk made for bob",
    signer: ME,
    meta: META,
    kiosks: [MY_KIOSK],
    sim: (() => {
      const s = kioskMove(objOwner(F_OUT));
      s.objects.push(
        obj({ objectId: NEW_KIOSK, type: "0x2::kiosk::Kiosk", inputOwner: null, inputExists: false, idOperation: "Created", outputOwner: { kind: "shared" } }),
        obj({ objectId: CAP, type: "0x2::kiosk::KioskOwnerCap", inputOwner: null, inputExists: false, idOperation: "Created", outputOwner: own(BOB) }),
      );
      return s;
    })(),
    risk: { addresses: { sentTo: [], receivedFrom: [], contacts: [{ address: BOB, name: "Bob" }] } },
  },
  {
    name: "first purchase makes my kiosk",
    signer: ME,
    meta: META,
    sim: sim({
      balanceChanges: [fee(-21_000_000_000n - FEE), { coinType: SUI, address: BOB, amount: "21000000000" }],
      objects: [
        obj({ objectId: NFT, inputOwner: objOwner(F_IN), outputOwner: objOwner(F_OUT) }),
        obj({ objectId: F_IN, type: FIELD_T, inputOwner: objOwner(SELLER_KIOSK), outputOwner: null, outputExists: false, idOperation: "Deleted" }),
        obj({ objectId: F_OUT, type: FIELD_T, inputOwner: null, inputExists: false, idOperation: "Created", outputOwner: objOwner(NEW_KIOSK) }),
        obj({ objectId: SELLER_KIOSK, type: "0x2::kiosk::Kiosk", inputOwner: { kind: "shared" }, outputOwner: { kind: "shared" } }),
        obj({ objectId: NEW_KIOSK, type: "0x2::kiosk::Kiosk", inputOwner: null, inputExists: false, idOperation: "Created", outputOwner: { kind: "shared" } }),
        obj({
          objectId: CAP,
          type: "0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802::personal_kiosk::PersonalKioskCap",
          inputOwner: null,
          inputExists: false,
          idOperation: "Created",
          outputOwner: own(ME),
        }),
      ],
    }),
  },
  { name: "kiosk item sent to bob", signer: ME, meta: META, kiosks: [MY_KIOSK], sim: kioskMove(own(BOB)) },
  { name: "kiosk item back to my wallet", signer: ME, meta: META, kiosks: [MY_KIOSK], sim: kioskMove(own(ME)) },
  {
    name: "failure not enough balance",
    signer: ME,
    meta: META,
    sim: sim({ status: "failure", failure: { message: "MoveAbort", abort: { code: "2", package: "0x2", module: "balance", function: "split" } }, balanceChanges: [fee()] }),
  },
  {
    name: "sponsored by someone else",
    signer: ME,
    meta: META,
    sim: sim({ gasOwner: SPONSOR, balanceChanges: [{ coinType: USDC, address: ME, amount: "-5000000" }, { coinType: USDC, address: BOB, amount: "5000000" }, { coinType: SUI, address: SPONSOR, amount: String(-FEE) }] }),
    risk: { addresses: { sentTo: [], receivedFrom: [], contacts: [] } },
  },
  {
    name: "sponsored by a game",
    signer: ME,
    meta: META,
    extras: { coveredByRare: true, paidBy: "Starlight Studio" },
    sim: sim({ gasOwner: SPONSOR, balanceChanges: [{ coinType: SUI, address: SPONSOR, amount: String(-FEE) }], objects: [obj({ objectId: NFT, inputOwner: null, inputExists: false, idOperation: "Created" })] }),
    calls: [{ package: ADDR("44"), module: "game", function: "mint" }],
  },
  { name: "sponsored by rare", signer: ME, meta: META, extras: { coveredByRare: true }, sim: sim({ gasOwner: SPONSOR, balanceChanges: [] }) },
  {
    name: "storage refund",
    signer: ME,
    meta: META,
    sim: sim({ gas: { computation: "1000000", storage: "1000000", rebate: "9000000" }, balanceChanges: [{ coinType: SUI, address: ME, amount: "7000000" }] }),
  },
  {
    name: "unknown coin fallback",
    signer: ME,
    sim: sim({ balanceChanges: [{ coinType: normType("0xabc::x::XCOIN"), address: ME, amount: "1500000000" }, fee()] }),
  },
  {
    name: "object classes",
    signer: ME,
    meta: META,
    sim: sim({
      gas: { computation: "1000", storage: "5000", rebate: "2000" },
      objects: [
        obj({ objectId: GAS, type: "0x2::coin::Coin<0x2::sui::SUI>" }),
        obj({ objectId: "0x01", type: "0xabc::nft::Nft", outputOwner: own(BOB) }),
        obj({ objectId: "0x02", type: "0xabc::nft::Nft", inputOwner: null, inputExists: false, idOperation: "Created" }),
        obj({ objectId: "0x03", type: "0xabc::nft::Nft", outputOwner: null, outputExists: false, idOperation: "Deleted" }),
        obj({ objectId: "0x04", type: "0xabc::nft::Nft", outputOwner: null, outputExists: false }),
        obj({ objectId: "0x05", type: "0xabc::nft::Nft", outputOwner: objOwner("0xk1") }),
        obj({ objectId: "0x06", type: "0xabc::nft::Nft" }),
        obj({ objectId: "0x07", type: `0x2::coin::Coin<${USDC}>`, outputOwner: own(BOB) }),
        obj({ objectId: "0x08", type: "0xabc::nft::Nft", inputOwner: own(BOB), outputOwner: own(ME) }),
        obj({ objectId: "0x09", outputKind: "accumulator", type: "0x2::accumulator::X" }),
        obj({ objectId: "0x0a", type: "0xabc::staked::StakedSui", outputOwner: { kind: "shared" } }),
        obj({ objectId: "0x0b", type: "0xabc::art::Art_piece", outputOwner: { kind: "immutable" } }),
      ],
    }),
    risk: { addresses: { sentTo: [], receivedFrom: [], contacts: [] } },
  },
  {
    name: "shared pools and publish",
    signer: ME,
    meta: META,
    sim: sim({
      objects: [
        obj({ objectId: "0xp1", type: `${CETUS}::pool::Pool<0x2::sui::SUI, ${USDC}>`, inputOwner: { kind: "shared" }, outputOwner: { kind: "shared" } }),
        obj({ objectId: "0xp2", type: "0x9999::vault::Vault", inputOwner: { kind: "shared" }, outputOwner: { kind: "shared" } }),
        obj({ objectId: "0xp3", type: "0x9999::vault::Vault", inputOwner: null, inputExists: false, idOperation: "Created", outputOwner: { kind: "shared" } }),
        obj({ objectId: "0xp4", type: "0x3::sui_system::SuiSystemState", inputOwner: { kind: "shared" }, outputOwner: { kind: "shared" } }),
        obj({ objectId: "0xpk", type: null, inputOwner: null, inputExists: false, outputKind: "package", outputOwner: { kind: "immutable" } }),
      ],
    }),
    calls: [{ package: "0x2", module: "package", function: "make_immutable" }],
  },
  {
    name: "cetus swap with clever abort",
    signer: ME,
    meta: META,
    sim: sim({
      status: "failure",
      failure: { message: "MoveAbort in 2nd command, 'EAmountOutBelowMinLimit', in '0x1eab::router::swap'", kind: "MoveAbort", command: 1, abort: { code: "6", package: CETUS, module: "router", function: "swap", constant: "EAmountOutBelowMinLimit" } },
      balanceChanges: [fee()],
    }),
    calls: [
      { package: ADDR("77"), module: "pool_script", function: "swap" },
      { package: "0x2", module: "coin", function: "split" },
    ],
    risk: { originals: { [ADDR("77")]: CETUS } },
  },
  {
    name: "abort in an unknown package",
    signer: ME,
    meta: META,
    sim: sim({ status: "failure", failure: { message: "x", abort: { code: "7", package: ADDR("78"), module: "game", function: "" } }, balanceChanges: [fee()] }),
    calls: [{ package: ADDR("78"), module: "game", function: "play" }, { package: ADDR("79"), module: "other", function: "go" }],
  },
  {
    name: "failure long message with ids",
    signer: ME,
    meta: META,
    sim: sim({
      status: "failure",
      failure: { message: `Unexpected thing with ${ADDR("ab")} and 0X${"CD".repeat(12)} and 0x1234 in a very long message that goes on and on and on and on and on and on and on and on and on and on and on` },
      balanceChanges: [fee()],
    }),
  },
  {
    name: "risky swap",
    signer: ME,
    meta: { ...META, [normType("0xdead::scam::SCAM")]: { symbol: "SCAM", decimals: 2 } },
    sim: sim({
      balanceChanges: [
        { coinType: "0xdead::scam::SCAM", address: ME, amount: "100" },
        { coinType: "0xbeef::spam::SPAM", address: ME, amount: "5" },
        { coinType: USDC, address: ME, amount: "-1000000" },
        { coinType: USDC, address: BOB, amount: "1000000" },
        fee(-20_000_000_000n - FEE),
        { coinType: "0xcafe::junk::JUNK", address: ME, amount: "-3" },
      ],
    }),
    risk: {
      prices: { [SUI_TYPE]: 4, [USDC]: 1 },
      decimals: { [USDC]: 6 },
      portfolioUsd: 100,
      safety: { [normType("0xdead::scam::SCAM")]: { spam: false, honeypot: true }, [normType("0xbeef::spam::SPAM")]: { spam: true, honeypot: false }, [normType("0xcafe::junk::JUNK")]: { spam: true, honeypot: true } },
    },
  },
  {
    name: "lookalike recipients",
    signer: ME,
    meta: META,
    sim: sim({
      balanceChanges: [
        fee(-3n - FEE),
        { coinType: SUI, address: "0xabcd" + "7".repeat(56) + "1234", amount: "1" },
        { coinType: SUI, address: "0x9999" + "0".repeat(56) + "8888", amount: "1" },
        { coinType: SUI, address: BOB, amount: "1" },
      ],
      objects: [obj({ objectId: NFT, outputOwner: own(SPONSOR) }), obj({ objectId: NFT2, outputOwner: own(SPONSOR) })],
    }),
    risk: {
      addresses: { sentTo: ["0xABCD" + "0".repeat(56) + "1234"], receivedFrom: [MARKET], contacts: [{ address: BOB, name: "Bob" }, { address: BOB, name: "Bobby" }] },
      self: ME,
    },
  },
  {
    name: "swap with rare fee",
    signer: ME,
    meta: META,
    sim: sim({
      balanceChanges: [
        fee(-100_000_000_000n - FEE),
        { coinType: SUI, address: ROUTER_FEE, amount: "200000000" },
        { coinType: SUI, address: ADDR("77"), amount: "99800000000" },
        { coinType: USDC, address: ME, amount: "398000000" },
      ],
    }),
    calls: [{ package: CETUS, module: "pool", function: "swap" }],
    risk: { prices: { [SUI_TYPE]: 4, [USDC]: 1 }, decimals: { [USDC]: 6 }, portfolioUsd: 1000, addresses: { sentTo: [], receivedFrom: [], contacts: [], warnNew: false } },
  },
  {
    name: "rare fee only",
    signer: ME,
    meta: META,
    sim: sim({ balanceChanges: [fee(-1_000_000_000n - FEE), { coinType: SUI, address: ROUTER_FEE, amount: "2500000" }] }),
    risk: { addresses: { sentTo: [], receivedFrom: [], contacts: [] } },
  },
  {
    name: "no gas owner and unknown status",
    signer: ME,
    meta: { [SUI_TYPE]: { symbol: "SUI", decimals: 9, iconUrl: "https://x/sui.png" } },
    sim: { status: "failure", balanceChanges: [{ coinType: SUI, address: ME, amount: "-1" }], objects: [], gas: { computation: "x", storage: "", rebate: "0x10" }, gasOwner: null },
  },
];

const AMOUNTS: [string, number][] = [
  ["0", 9], ["1", 9], ["1997880", 9], ["2500000000", 9], ["-1500000000", 9], ["1234567890000", 9], ["1234567890123", 9],
  ["999999999", 9], ["100000000000000000", 9], ["12345678", 6], ["5", 0], ["123456789", 0], ["10", 1], ["-1", 9], ["-1997880", 9],
  ["170141183460469231731687303715884105727", 18], ["1000", 3], ["999", 3], ["50", 2],
];
const MONEY = ["0", "-0", "-0.001", "1.005", "1234567.891", "0.5", "0.125", "-5", "1e21", "Infinity", "-Infinity", "NaN", "2.675", "0.0000005", "0.00012345", "0.12345", "0.009999", "99.995", "1000000", "0.01", "1", "123456789012345680000"];
const PARSE: [string, number][] = [
  ["1.5", 9], [".25", 6], ["1.1234567", 6], ["", 9], ["1e5", 9], [".", 9], ["  12  ", 2], ["007", 3], ["0", 9], ["1.", 9], ["1,5", 9], ["1.2.3", 9], ["99999999999999999999999999999999999999999", 0], ["-1", 9], ["5", 0],
];
const USD_TO_BASE: [string, string | null, number][] = [
  ["10", "4", 9], ["1", "3", 6], ["", "4", 9], ["5", null, 9], ["5", "0", 9], ["abc", "1", 9], ["1e3", "2", 9], ["0x10", "1", 0], [" 2.5 ", "1", 2], ["-1", "1", 9], ["1", "0.000001", 9], ["3", "7", 30], ["Infinity", "1", 9], ["0.5", "1", 0],
];
const BASE_TO_USD: [string, number, string | null][] = [["2500000000", 9, "2"], ["1", 9, "3.3"], ["123456789", 6, null], ["5", 0, "0"], ["-1500000000", 9, "1.5"]];
const CHIPS: [string, Chip, boolean][] = [
  ["10000000000", 25, true], ["10000000000", 50, true], ["10000000000", 100, true], ["50000000", 100, false], ["50000000", 50, false], ["10000000", 100, true], ["12000000", 50, true], ["3", 25, false], ["0", 100, true],
];
const TYPES: [string | null, Record<string, string> | null][] = [
  ["0x3::sui_system::SuiSystemState", null], [`${CETUS}::pool::Pool<0x2::sui::SUI, ${USDC}>`, null], ["0x9999::vault::Vault", null], [null, null], ["u64", null],
  [`${ADDR("77")}::pool::Pool`, { [ADDR("77")]: CETUS }], ["0x2::kiosk::KioskOwnerCap", null], ["0xdee9::clob_v2::Pool<A, B>", null], ["0x2::coin::Coin<0x2::sui::SUI>", null],
  ["0xd22b24490e0bae52676651b4f56660a5ff8022a2576e0089f79b3c88d44e08f0::suins_registration::SuinsRegistration", null], ["0xabc::m::cetus_position", null], ["0xabc::a::b::c", null],
  ["0xe50387176587c707c428c537b7b8fb530c3eaeadd16a45b1d975369f372dbc0a::drip::DripListing", null],
];
const IDS = ["0x2", "0x0", "0x", CETUS, "0xdee9", "0x123456", "0x1234567", ADDR("ab"), "0xABCDEF0123456789", "abc", "0x" + "0".repeat(64)];
const NORM_TYPES = ["0x2::sui::SUI", "0x2::coin::Coin<0x2::sui::SUI>", "0xabc::x::Y<u64, vector<0x2::sui::SUI>>", "vector<u8>", "u64", "not a type", "0x2::a", "0x2::m::T<", `0xABC::Mod::T<0x1::string::String,0x2::object::ID>`, "@mysten/app::m::T", "0x2::m::T<>"];
const HUMANIZE = ["StakedSui", "PositionNFT", "Art_piece", "aBcDe", "__x__y__", "already spaced", "ABC", "pool_v2Lp"];
const FAILURES: (SimFailure | null)[] = [
  null,
  { message: "InsufficientCoinBalance in command 0" },
  { message: "MoveAbort ...", abort: { code: "2", package: "0x2", module: "balance", function: "split" } },
  { message: "x", abort: { code: "6", package: CETUS, module: "router", function: "swap", constant: "EAmountOutBelowMinLimit" } },
  { message: "x", abort: { code: "7", package: CETUS, module: "router", function: "swap" } },
  { message: "x", abort: { code: "7", package: CETUS, module: "router" } },
  { message: "x", abort: { code: "1", constant: "ENotOwner" } },
  { message: "x", abort: { code: "1", constant: "Eabc", module: "m", function: "f" } },
  { message: "x", abort: { code: "1", constant: "ELOCKED_until_later", module: "m" } },
  { message: "x", abort: { code: "3" } },
  { message: "Error checking transaction input objects: ObjectNotFound" },
  { message: "Balance of gas object 10 is lower than the needed amount: 100" },
  { message: "GasBudgetTooLow" },
  { message: "" },
  { message: "Something odd at 0x" + "f".repeat(40) },
  { message: "Kaboom ".repeat(30) },
  { message: "x".repeat(140) },
];
const KNOWN_ADDR = "0xabcd" + "0".repeat(56) + "1234";
const FAKE_ADDR = "0xabcd" + "7".repeat(56) + "1234";
const NEW_ADDR = "0x9999" + "0".repeat(56) + "8888";
const H = (o: Partial<AddressHistory> = {}): AddressHistory => ({ sentTo: [], receivedFrom: [], contacts: [], ...o });
const ADDRESS_CHECKS: { recipients: string[]; history: AddressHistory | null; self?: string }[] = [
  { recipients: [NEW_ADDR], history: H({ sentTo: [KNOWN_ADDR] }) },
  { recipients: [KNOWN_ADDR], history: H({ sentTo: [KNOWN_ADDR.toUpperCase().replace("0X", "0x")] }) },
  { recipients: [FAKE_ADDR], history: H({ sentTo: [KNOWN_ADDR] }) },
  { recipients: [FAKE_ADDR], history: H({ receivedFrom: [KNOWN_ADDR] }) },
  { recipients: [FAKE_ADDR], history: H({ contacts: [{ address: KNOWN_ADDR, name: "Alice" }] }) },
  { recipients: ["0xabcd" + "0".repeat(56) + "9999"], history: H({ sentTo: [KNOWN_ADDR] }) },
  { recipients: [NEW_ADDR], history: H({ contacts: [{ address: NEW_ADDR, name: "Alice" }] }) },
  { recipients: [NEW_ADDR], history: H({ warnNew: false }) },
  { recipients: [FAKE_ADDR], history: H({ sentTo: [KNOWN_ADDR], warnNew: false }) },
  { recipients: [NEW_ADDR], history: H(), self: NEW_ADDR },
  { recipients: [FAKE_ADDR], history: null },
  { recipients: [` ${NEW_ADDR.toUpperCase().replace("0X", "0x")} `, NEW_ADDR, FAKE_ADDR], history: H({ receivedFrom: [KNOWN_ADDR] }) },
  { recipients: ["0x12", "0x1"], history: H({ sentTo: ["0x0012"] }) },
];

/** JSON with bigints as strings and null fields left out (outputs). */
const out = (v: unknown) => JSON.parse(JSON.stringify(v, (_k, x) => (typeof x === "bigint" ? x.toString() : x === null ? undefined : x)));
const num = (s: string | null) => (s === null ? null : Number(s));

function runCase(c: Case) {
  const ins = analyze(c.sim, c.signer, c.calls ?? [], { kiosks: c.kiosks });
  const rows = describeReview(ins, c.meta ?? {}, { display: c.sim.display, originals: c.sim.packages, coveredByRare: c.extras?.coveredByRare, paidBy: c.extras?.paidBy });
  const r = c.risk ?? {};
  const ctx: RiskContext = {
    prices: r.prices ?? {},
    decimals: (t) => r.decimals?.[t] ?? 9,
    symbol: (t) => r.symbols?.[t] ?? t.split("::").pop()!,
    portfolioUsd: r.portfolioUsd,
    safety: r.safety,
    originals: r.originals,
    addresses: r.addresses,
    self: r.self,
  };
  return out({ insights: ins, rows, risks: riskChecks(ins, ctx), fees: splitFees(ins) });
}

function generate() {
  return {
    cases: CASES.map((c) => ({ ...c, out: runCase(c) })),
    amounts: AMOUNTS.map(([base, decimals]) => ({
      base,
      decimals,
      formatAmount: formatAmount(base, decimals),
      formatUnits: formatUnits(base, decimals),
      formatUnitsFull: formatUnits(BigInt(base), decimals, decimals),
      formatUnits2: formatUnits(base, decimals, 2),
      toInput: toInput(base, decimals),
      toNumber: String(toNumber(base, decimals)),
    })),
    money: MONEY.map((n) => ({ n, usd: usd(Number(n)), price: price(Number(n)) })),
    parse: PARSE.map(([input, decimals]) => ({ input, decimals, parseUnits: parseUnits(input, decimals), cleanAmount: cleanAmount(input) })),
    usdToBase: USD_TO_BASE.map(([text, p, decimals]) => ({ text, price: p, decimals, out: usdToBase(text, num(p), decimals) })),
    baseToUsd: BASE_TO_USD.map(([base, decimals, p]) => {
      const v = baseToUsd(base, decimals, num(p));
      return { base, decimals, price: p, out: v === null ? null : String(v) };
    }),
    chips: CHIPS.map(([available, pct, isSui]) => ({ available, pct, isSui, out: chipAmount(BigInt(available), pct, isSui).toString() })),
    labels: TYPES.map(([type, originals]) => ({
      type,
      originals,
      label: objectLabel(type, originals ?? undefined),
      parsed: parseType(type),
      isCoin: isCoinObject(type),
      coinTypeOf: type ? coinTypeOf(type) : null,
    })),
    ids: IDS.map((id) => ({ id, shortId: shortId(id), known: knownPackage(id), fee: knownFee(id)?.label ?? null, lookalike: lookalikeShort(id) })),
    normTypes: NORM_TYPES.map((t) => ({ t, norm: normType(t) })),
    humanize: HUMANIZE.map((s) => ({ s, out: humanize(s) })),
    failures: FAILURES.map((f) => ({ failure: f, friendly: friendlyFailure(f ?? undefined) })),
    addressChecks: ADDRESS_CHECKS.map((a) => ({ ...a, out: out(addressChecks(a.recipients, a.history ?? undefined, a.self)) })),
    callLabels: [
      { call: { package: CETUS, module: "router", function: "swap" }, originals: null },
      { call: { package: ADDR("77"), module: "pool_script", function: "swap" }, originals: { [ADDR("77")]: CETUS } },
      { call: { package: ADDR("78"), module: "game", function: "play" }, originals: null },
    ].map((c) => ({ ...c, label: callLabel(c.call, c.originals ?? undefined) })),
  };
}

describe("TS/Rust review vectors", () => {
  it("the TS review still produces review-vectors.json", () => {
    const got = JSON.parse(JSON.stringify(generate(), (_k, x) => (typeof x === "bigint" ? x.toString() : x)));
    if (process.env.RARE_WRITE_VECTORS) writeFileSync(FILE, JSON.stringify(got, null, 2) + "\n");
    expect(got).toEqual(JSON.parse(readFileSync(FILE, "utf8")));
  });
});
