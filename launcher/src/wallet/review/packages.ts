import { normalizeSuiAddress } from '@mysten/sui/utils'

/** Original (v1) package ids of well-known protocols, checked on mainnet 2026-10-24.
 * Upgraded packages resolve to these through Simulation.packages (id -> original id). */
const KNOWN: Record<string, string> = {
  '0x1': 'Move stdlib',
  '0x2': 'Sui',
  '0x3': 'Sui system',
  // Rare's own contracts (rare.market's review names them the same way)
  '0x465753839605ea682f147cfe5db736c7e8718798d64cd8f0fe7bd60869d64b03': 'Rare',
  '0x94b8e1cf51652ea35680a55cb8a7b1a16978f2ba1dc15e0c93ed7536d7659475': 'Rare',
  '0x9252bf93053dcd0cd5ad0562303e66d6a5526d6022453b88c00dc463a012a7bf': 'Rare',
  '0xda09fa9db4a54d4b0c337fb7ee41311f4679a7f6049e58372a4951968a32f73c': 'Rare',
  '0xdee9': 'DeepBook',
  '0x2c8d603bc51326b8c13cef9dd07031a408a48dddb541963357661df5d3204809': 'DeepBook',
  '0x1eabed72c53feb3805120a081dc15963c204dc8d091542592abaf7a35689b2fb': 'Cetus',
  '0x91bfbc386a41afcfd9b2533058d7e915a1d3829089cc268ff4333d54d6339ca1': 'Turbos',
  '0x3492c874c1e3b3e2984e8c41b589e642d4d0a5d6459e5a9cfc2d52fd7c89c267': 'Bluefin',
  '0xefe170ec0be4d762196bedecd7a065816576198a6527c99282a2551aaa7da38c': 'Aftermath',
  '0xa0eba10b173538c8fecca1dff298e488402cc9ff374f8a12ca7758eebe830b66': 'Kriya',
  '0xba153169476e8c3114962261d1edc70de5ad9781b83cc617ecc8c1923191cae0': 'FlowX',
  '0xf95b06141ed4a174f239417323bde3f209b972f5930d8521ea38a52aff3a6ddf': 'Suilend',
  '0xefe8b36d5b2e43728cc323298626b83177803521d195cfb11e15b910e892fddf': 'Scallop',
  '0xd899cf7d2b5db716bd2cf55599fb0d5ee38a3061e7b6bb6eebf73fa5bc4c81ca': 'NAVI',
  '0xd22b24490e0bae52676651b4f56660a5ff8022a2576e0089f79b3c88d44e08f0': 'SuiNS',
  // Mysten kiosk extensions (royalty, lock, personal kiosk rules), original id and the v4 we call
  '0x434b5bd8f6a7b05fede0ff46c6e511d71ea326ed38056e3bcd681d2d7c2a7879': 'Kiosk',
  '0xdfb4f1d4e43e0c3ad834dcd369f0d39005c872e118c9dc1c5da9765bb93ee5f3': 'Kiosk',
  // NFT marketplaces Rare Marketplace buys from (rare.market packages/venues/src/ids.ts), origin and latest ids
  '0xb42dbb7413b79394e1a0175af6ae22b69a5c7cc5df259cd78072b6818217c027': 'Tradeport',
  '0xd85a131b82ef9f4aa5eb99a0ea758a3f209a8771eba7e9402e8181273734bf9a': 'Tradeport',
  '0xec175e537be9e48f75fa6929291de6454d2502f1091feb22c0d26a22821bbf28': 'Tradeport',
  '0xafc035fc8e321da96a2cb1bb88cec64ef6bf7c83483653c259184312b3c0bb96': 'Tradeport',
  '0xf938f27ce9460838643ac04fac05254f73329caf59643b7843c883bb4fa7402f': 'Tradeport',
  '0x53134eb544c5a0b5085e99efaf7eab13b28ad123de35d61f941f8c8c40b72033': 'Tradeport',
  '0xe2680c552bd3a5540c2f2986e92927ef2ebb6308e41ae395a1cec24e51d2d951': 'Tradeport',
  '0xff2251ea99230ed1cbe3a347a209352711c6723fcdcd9286e16636e65bb55cab': 'Tradeport',
  '0x81e16ff388eacc2908f4e8248f54d140eb6d7834ff01dc7a86042a3b701b68f6': 'Tradeport',
  '0x06cefebb11191abd347654200a6fd8ae47fa97b54506e4e680db64dae54d0b12': 'Tradeport',
  '0xe50387176587c707c428c537b7b8fb530c3eaeadd16a45b1d975369f372dbc0a': 'Drip',
  '0x7dcbc405496f4f6ecb70921b571ea9bfca09db7004efddd3dad33a524e7489e4': 'Drip',
  '0x0cb4bcc0560340eb1a1b929cabe56b33fc6449820ec8c1980d69bb98b649b802': 'Kiosk',
  '0x34cc6762780f4f6f153c924c0680cfe2a1fb4601e7d33cc28a92297b62de1e0e': 'Kiosk',
}
const BY_ID = new Map(Object.entries(KNOWN).map(([k, v]) => [normalizeSuiAddress(k), v]))

export const normId = (id: string) => {
  try {
    return normalizeSuiAddress(id)
  } catch {
    return id.toLowerCase()
  }
}

/** `0x1eab…b2fb`; framework ids stay `0x2`. */
export function shortId(id: string): string {
  const n = normId(id)
  const trimmed = '0x' + n.slice(2).replace(/^0+/, '')
  if (trimmed.length <= 6) return trimmed === '0x' ? '0x0' : trimmed
  return `${n.slice(0, 6)}…${n.slice(-4)}`
}

/** Protocol name for a package (or its original id), else null. */
export function knownPackage(id: string, originals?: Record<string, string>): string | null {
  const n = normId(id)
  const orig = originals?.[n] ?? originals?.[id]
  return BY_ID.get(n) ?? (orig ? BY_ID.get(normId(orig)) ?? null : null)
}

export interface ParsedType {
  pkg: string
  module: string
  name: string
}

/** `0xabc::pool::Pool<A, B>` -> { pkg, module, name }; null for primitives. */
export function parseType(t: string | null | undefined): ParsedType | null {
  if (!t) return null
  const base = t.split('<')[0]
  const parts = base.split('::')
  if (parts.length !== 3) return null
  return { pkg: normId(parts[0]), module: parts[1], name: parts[2] }
}

const SUI_COIN = /^0x0*2::coin::Coin</
/** Coin<T> objects: covered by balance changes, never listed as objects. */
export const isCoinObject = (t: string | null | undefined) => !!t && SUI_COIN.test(t)
/** `T` out of `0x2::coin::Coin<T>` */
export const coinTypeOf = (t: string) => t.replace(SUI_COIN, '').replace(/>$/, '')

/** `StakedSui` -> `Staked Sui`, `PositionNFT` stays readable. */
export function humanize(name: string): string {
  return name
    .replace(/_/g, ' ')
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .replace(/\s+/g, ' ')
    .trim()
}

/** "Cetus Pool", or `0x1eab…b2fb::pool` for unknown packages. */
export function objectLabel(type: string | null, originals?: Record<string, string>): { title: string; sub: string } {
  const p = parseType(type)
  if (!p) return { title: 'Object', sub: '' }
  const proto = knownPackage(p.pkg, originals)
  const name = humanize(p.name)
  return proto
    ? { title: name.toLowerCase().startsWith(proto.toLowerCase()) ? name : `${proto} ${name}`, sub: `${p.module}::${p.name}` }
    : { title: humanize(p.name), sub: `${shortId(p.pkg)}::${p.module}` }
}
