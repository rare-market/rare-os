/* Transfer policy helpers: pick a policy whose rules
 * we can all satisfy and resolve a TransferRequest before confirming it. rules.json is Drip's
 * registry (shared with its indexer). */

import type { Transaction, TransactionObjectArgument } from '@mysten/sui/transactions'
import { KIOSK_EXT_PACKAGE, KIOSK_EXT_VERSIONS } from './ids'
import type { KioskHandle } from './kiosk'
import registry from './rules.json'
import type { PolicyRule, TransferPolicyInfo } from './types'

export interface RuleSpec {
  id: string
  module: string
  struct?: string
  packages: string[]
}
export interface UnsupportedRuleSpec {
  module: string
  struct?: string
  packages: string[]
  reason: string
}

export const RULES: RuleSpec[] = registry.rules
export const UNSUPPORTED_RULES: UnsupportedRuleSpec[] = registry.unsupported

function normPkg(x: string): string {
  return `0x${x.toLowerCase().replace(/^0x/, '').padStart(64, '0')}`
}
const samePackage = (a: string, b: string) => normPkg(a) === normPkg(b)
const isMysten = (pkg: string) => KIOSK_EXT_VERSIONS.some((v) => samePackage(v, pkg))

/** Package, module and struct (without type arguments) of a rule type string. */
export function parseRuleType(ruleType: string): { pkg: string; module: string; struct: string } | null {
  const [pkg, module, rest] = (ruleType || '').split('::')
  if (!pkg || !module) return null
  return { pkg: normPkg(pkg), module, struct: (rest || '').split('<')[0] }
}

export interface MatchedRule {
  spec: RuleSpec
  pkg: string
  /** Package to call: the latest Mysten version for Mysten rules, else the defining package. */
  callPkg: string
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  config: any
}

/** The registry entry for a rule, or null when it can't be satisfied. */
export function matchRule(rule: PolicyRule): MatchedRule | null {
  const parsed = parseRuleType(rule.ruleType || '')
  if (parsed) {
    const spec = RULES.find(
      (r) => r.module === parsed.module && (!r.struct || !parsed.struct || r.struct === parsed.struct) && r.packages.some((p) => samePackage(p, parsed.pkg)),
    )
    if (!spec) return null
    return { spec, pkg: parsed.pkg, callPkg: isMysten(parsed.pkg) ? KIOSK_EXT_PACKAGE : parsed.pkg, config: rule.config ?? {} }
  }
  const spec = RULES.find((r) => r.id === rule.type && r.packages.some(isMysten))
  return spec ? { spec, pkg: KIOSK_EXT_PACKAGE, callPkg: KIOSK_EXT_PACKAGE, config: rule.config ?? {} } : null
}

/** Why a rule can't be satisfied, in plain words. */
export function unsupportedReason(rule: PolicyRule): string {
  const parsed = parseRuleType(rule.ruleType || '')
  const known = parsed && UNSUPPORTED_RULES.find((u) => u.module === parsed.module && u.packages.some((p) => samePackage(p, parsed.pkg)))
  const name = parsed ? `${parsed.module}::${parsed.struct}` : rule.type
  return known ? known.reason : `${name} is a custom rule with no way to satisfy it`
}

export interface PolicyNeeds {
  lock: boolean
  personal: boolean
  royalty: boolean
  floor: bigint | null
  rules: MatchedRule[]
  unsupported: string[]
}

function bigintOf(value: unknown, fallback = 0n): bigint {
  if (value === null || value === undefined || value === '') return fallback
  try {
    return BigInt(typeof value === 'number' ? Math.floor(value) : String(value))
  } catch {
    return fallback
  }
}

export function policyNeeds(policy: TransferPolicyInfo | null | undefined): PolicyNeeds {
  const needs: PolicyNeeds = { lock: false, personal: false, royalty: false, floor: null, rules: [], unsupported: [] }
  for (const rule of policy?.rules ?? []) {
    const m = matchRule(rule)
    if (!m) {
      needs.unsupported.push(rule.ruleType || rule.type || 'unknown')
      continue
    }
    needs.rules.push(m)
    const config = m.config ?? {}
    switch (m.spec.id) {
      case 'royalty_rule':
      case 'royalty_rule_copy':
        needs.royalty = true
        break
      case 'kiosk_lock_rule':
      case 'kiosk_lock_rule_copy':
        needs.lock = true
        break
      case 'personal_kiosk_rule':
        needs.personal = true
        break
      case 'floor_price_rule':
        needs.floor = bigintOf(config.floor_price ?? config.floorPrice ?? config.floor ?? config.price)
        break
    }
  }
  return needs
}

export function isPolicySupported(policy: TransferPolicyInfo | null | undefined): boolean {
  return !!policy && policy.isShared !== false && policyNeeds(policy).unsupported.length === 0
}

function policyCost(policy: TransferPolicyInfo): number {
  const needs = policyNeeds(policy)
  return needs.rules.length * 10_000 + (needs.personal ? 5_000 : 0) + (needs.lock ? 1_000 : 0) + (needs.royalty ? 100 : 0)
}

/** Among the shared policies whose rules we can all resolve, the cheapest one. */
export function pickPolicy(policies: TransferPolicyInfo[] | null | undefined): TransferPolicyInfo | null {
  const list = (policies ?? []).filter((p) => p && p.id).filter(isPolicySupported)
  return list.map((p, i) => ({ p, i })).sort((a, b) => policyCost(a.p) - policyCost(b.p) || a.i - b.i)[0]?.p ?? null
}

/** Why a locked item can't be moved under these policies, or null when one of them works. */
export function policyProblem(policies: TransferPolicyInfo[] | null | undefined): string | null {
  const shared = (policies ?? []).filter((p) => p && p.id && p.isShared !== false)
  if (!shared.length) return 'No transfer policy found for this collection, so a locked item can not leave its kiosk'
  if (pickPolicy(shared)) return null
  const reasons = new Set<string>()
  for (const p of shared) for (const r of p.rules) if (!matchRule(r)) reasons.add(unsupportedReason(r))
  return `The collection's transfer rules block it: ${[...reasons].join('; ')}`
}

export interface ResolveRequestOptions {
  nftType: string
  policy: TransferPolicyInfo
  request: TransactionObjectArgument
  item: TransactionObjectArgument
  /** Kiosk that receives the item. Required when the policy has a lock or personal kiosk rule. */
  kiosk?: KioskHandle | null
  recipient: string
  /** Put the item into `kiosk` even when no rule requires it. */
  preferKiosk?: boolean
}

interface RuleCallContext {
  tx: Transaction
  nftType: string
  policyArg: TransactionObjectArgument
  request: TransactionObjectArgument
  kiosk: KioskHandle | null
  rule: MatchedRule
}

const RESOLVERS: Record<string, (c: RuleCallContext) => void> = {
  royalty_rule: payRoyalty,
  royalty_rule_copy: payRoyalty,
  kiosk_lock_rule: proveLock,
  kiosk_lock_rule_copy: proveLock,
  personal_kiosk_rule: ({ tx, nftType, request, kiosk, rule }) => {
    tx.moveCall({ target: `${rule.callPkg}::personal_kiosk_rule::prove`, typeArguments: [nftType], arguments: [kiosk!.kiosk, request] })
  },
  floor_price_rule: ({ tx, nftType, policyArg, request, rule }) => {
    tx.moveCall({ target: `${rule.callPkg}::floor_price_rule::prove`, typeArguments: [nftType], arguments: [policyArg, request] })
  },
  staking_lock_rule: ({ tx, policyArg, request, rule }) => {
    const registryId = rule.config?.registry_id ?? rule.config?.registryId
    if (!registryId) throw new Error('staking_lock_rule config is missing registry_id')
    tx.moveCall({ target: `${rule.callPkg}::staking_lock_rule::prove`, arguments: [policyArg, request, tx.object(registryId)] })
  },
}

/** Royalty computed on chain with fee_amount(policy, paid), so the split is always exact. */
function payRoyalty({ tx, nftType, policyArg, request, rule }: RuleCallContext) {
  const paid = tx.moveCall({ target: '0x2::transfer_policy::paid', typeArguments: [nftType], arguments: [request] })
  const fee = tx.moveCall({ target: `${rule.callPkg}::royalty_rule::fee_amount`, typeArguments: [nftType], arguments: [policyArg, paid] })
  const [feeCoin] = tx.splitCoins(tx.gas, [fee])
  tx.moveCall({ target: `${rule.callPkg}::royalty_rule::pay`, typeArguments: [nftType], arguments: [policyArg, request, feeCoin] })
}

function proveLock({ tx, nftType, request, kiosk, rule }: RuleCallContext) {
  tx.moveCall({ target: `${rule.callPkg}::kiosk_lock_rule::prove`, typeArguments: [nftType], arguments: [request, kiosk!.kiosk] })
}

/** Deliver `item`, satisfy every rule on `policy`, then confirm the request. */
export function resolveTransferRequest(tx: Transaction, opts: ResolveRequestOptions): void {
  const { nftType, policy, request, item, kiosk } = opts
  const needs = policyNeeds(policy)
  if (needs.unsupported.length) throw new Error(`Transfer policy ${policy.id} has rules that can't be resolved: ${needs.unsupported.join(', ')}`)
  const policyArg = tx.object(policy.id)

  if (needs.lock || needs.personal || (opts.preferKiosk && kiosk)) {
    if (!kiosk) throw new Error('This collection requires the item to go into a kiosk')
    if (needs.personal && !kiosk.isPersonal) throw new Error('This collection requires a personal kiosk')
    if (needs.lock) tx.moveCall({ target: '0x2::kiosk::lock', typeArguments: [nftType], arguments: [kiosk.kiosk, kiosk.cap, policyArg, item] })
    else tx.moveCall({ target: '0x2::kiosk::place', typeArguments: [nftType], arguments: [kiosk.kiosk, kiosk.cap, item] })
    kiosk.used = true
  } else {
    tx.transferObjects([item], opts.recipient)
  }

  const confirm = () => {
    tx.moveCall({ target: '0x2::transfer_policy::confirm_request', typeArguments: [nftType], arguments: [policyArg, request] })
  }
  let deferred = false
  for (const rule of needs.rules) {
    const ctx: RuleCallContext = { tx, nftType, policyArg, request, kiosk: kiosk ?? null, rule }
    if (rule.spec.id === 'personal_kiosk_rule' && kiosk!.onPersonal) {
      // a kiosk created for another address only becomes personal when the session finishes
      // (personal_kiosk::create_for), so the proof and confirmation wait for it
      deferred = true
      kiosk!.onPersonal.push(() => {
        RESOLVERS.personal_kiosk_rule(ctx)
        confirm()
      })
      continue
    }
    RESOLVERS[rule.spec.id](ctx)
  }
  if (!deferred) confirm()
}
