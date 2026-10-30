// The screens' Ledger API end to end: index.ts -> device.ts -> NativeLedgerTransport -> a fake
// native port backed by the ?mockhw simulated Sui app (real @mysten/ledgerjs-hw-app-sui on top).
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { Transaction, Inputs } from '@mysten/sui/transactions'
import { verifyPersonalMessageSignature, verifyTransactionSignature } from '@mysten/sui/verify'
import type { LedgerDevice } from '../native'

const h = vi.hoisted(() => ({
  devices: [{ id: 'usb:mock', transport: 'usb', name: 'Nano S Plus', model: 'nanoSP' }] as LedgerDevice[],
  bleDevices: [{ id: 'ble:mock', transport: 'ble', name: 'Nano X 4F2A', model: 'nanoX', paired: true }] as LedgerDevice[],
  openError: null as Error | null,
  opens: 0,
  closes: 0,
}))

vi.mock('../native', async (orig) => {
  const real = await orig<typeof import('../native')>()
  return {
    ...real,
    ledgerNative: {
      list: async (o?: { ble?: boolean }) => (o?.ble ? [...h.devices, ...h.bleDevices] : h.devices),
      open: async (id: string) => {
        if (h.openError) throw h.openError
        h.opens++
        const d = [...h.devices, ...h.bleDevices].find((x) => x.id === id)
        if (!d) throw new real.NativeError('notfound', 'gone')
        return d
      },
      exchange: async (apdu: Uint8Array) => (await import('./mock')).mockLedgerExchange(apdu),
      close: async () => void h.closes++,
    },
  }
})

vi.mock('../index', async () => {
  const { Keystore } = await import('../core/keys/keystore')
  const { MemoryKV } = await import('../core/store/kv')
  const { memoryVault } = await import('../testing/memoryVault')
  return {
    keystore: new Keystore({ disk: new MemoryKV(), vault: memoryVault() }),
    grpcClient: { core: { getObjects: async (o: { objectIds: string[] }) => ({ objects: o.objectIds.map(() => ({ objectBcs: Uint8Array.of(1, 2, 3) })) }) } },
  }
})

import { keystore } from '../index'
import { mockLedgerDevice } from './mock'
import { TEST_PHRASE, deviceKey } from './_testkit'
import { suiLedgerPath } from '../core/ledger/path'
import { NativeError } from '../native'
import {
  addLedgerAccount,
  cancelLedger,
  connectLedger,
  forgetLedger,
  ledgerAccounts,
  listLedgers,
  useLedgerPrompt,
  verifyLedgerAddress,
  type LedgerStage,
} from './index'

const OBJ = (n: number) => '0x' + n.toString(16).padStart(64, '0')
const DIGEST = '11111111111111111111111111111111'

function txBytes(sender: string) {
  const tx = new Transaction()
  tx.setSender(sender)
  tx.setGasPrice(1000)
  tx.setGasBudget(5_000_000)
  tx.setGasPayment([{ objectId: OBJ(1), version: '7', digest: DIGEST }])
  tx.transferObjects([tx.object(Inputs.ObjectRef({ objectId: OBJ(2), version: '3', digest: DIGEST }))], tx.pure.address(OBJ(0xbeef)))
  return tx.build()
}

function stages(): LedgerStage[] {
  const seen: LedgerStage[] = []
  useLedgerPrompt.subscribe((s) => {
    if (seen[seen.length - 1] !== s.stage) seen.push(s.stage)
  })
  return seen
}

const waitFor = async (f: () => boolean, ms = 4000) => {
  const end = Date.now() + ms
  while (!f()) {
    if (Date.now() > end) throw new Error('timed out')
    await new Promise((r) => setTimeout(r, 20))
  }
}

beforeEach(() => {
  Object.assign(mockLedgerDevice().dev, { app: 'Sui', locked: false, reject: false, blindOff: false, phrase: TEST_PHRASE, promptDelayMs: 0, signs: [], shown: [] })
  h.openError = null
  forgetLedger()
})

describe('Ledger API for screens', () => {
  it('lists devices, connects and reports the Sui app version, then closes', async () => {
    expect(await listLedgers()).toHaveLength(1)
    expect(await listLedgers({ ble: true })).toHaveLength(2)
    const closes = h.closes
    const c = await connectLedger('usb:mock')
    expect(c).toEqual({ id: 'usb:mock', model: 'nanoSP', name: 'Nano S Plus', transport: 'usb', appVersion: '1.4.0' })
    await waitFor(() => h.closes > closes)
    expect(useLedgerPrompt.getState().stage).toBe('done')
  })

  it('reads accounts without a device prompt, shows one on request, adds it', async () => {
    await connectLedger('usb:mock')
    const list = await ledgerAccounts(0, 3)
    expect(list.map((a) => a.address)).toEqual([0, 1, 2].map((i) => deviceKey(TEST_PHRASE, suiLedgerPath(i)).toSuiAddress()))
    expect(list[2].path).toBe("m/44'/784'/2'/0'/0'")
    expect(mockLedgerDevice().dev.shown).toEqual([])

    const seen = stages()
    const v = await verifyLedgerAddress(1)
    expect(v.address).toBe(list[1].address)
    expect(mockLedgerDevice().dev.shown).toEqual([suiLedgerPath(1)])
    expect(seen).toContain('verify')

    const acc = await addLedgerAccount(1, 'Main')
    expect(acc).toMatchObject({ kind: 'ledger', label: 'Main', address: list[1].address, ledger: { path: suiLedgerPath(1), device: 'Nano S Plus' } })
  })

  it('signs a transaction with clear-signing objects and a personal message through the keystore signer', async () => {
    await connectLedger('usb:mock')
    const acc = await addLedgerAccount(0)
    forgetLedger() // the signer finds the device itself
    const signer = await keystore.getSigner(acc.address)
    const bytes = await txBytes(acc.address)
    const seen = stages()
    const sig = await signer.signTransaction(bytes)
    await expect(verifyTransactionSignature(bytes, sig, { address: acc.address })).resolves.toBeTruthy()
    expect(mockLedgerDevice().dev.signs[0].objects.map((o) => [...o])).toEqual([
      [1, 2, 3],
      [1, 2, 3],
    ])
    expect(seen).toContain('confirm')
    expect(useLedgerPrompt.getState().stage).toBe('idle')

    const msg = new TextEncoder().encode('Sign in to Rare')
    const r = await signer.signPersonalMessage(msg)
    await expect(verifyPersonalMessageSignature(msg, r.signature, { address: acc.address })).resolves.toBeTruthy()
  })

  it('waits on "Open the Sui app" and carries on once it is open', async () => {
    await connectLedger('usb:mock')
    const acc = await addLedgerAccount(2)
    mockLedgerDevice().dev.app = 'BOLOS'
    const signer = await keystore.getSigner(acc.address)
    const p = signer.signTransaction(await txBytes(acc.address))
    await waitFor(() => useLedgerPrompt.getState().stage === 'open-app')
    expect(useLedgerPrompt.getState().message).toBe('Open the Sui app on your Ledger')
    mockLedgerDevice().dev.app = 'Sui'
    await expect(p).resolves.toBeTruthy()
  })

  it('reports rejection, a different Ledger, and cancel in the prompt store', async () => {
    await connectLedger('usb:mock')
    const acc = await addLedgerAccount(3)
    const signer = await keystore.getSigner(acc.address)
    const bytes = await txBytes(acc.address)

    mockLedgerDevice().dev.reject = true
    await expect(signer.signTransaction(bytes)).rejects.toMatchObject({ code: 'rejected' })
    expect(useLedgerPrompt.getState()).toMatchObject({ stage: 'error', message: 'Rejected on your Ledger.' })

    mockLedgerDevice().dev.reject = false
    mockLedgerDevice().dev.phrase = 'legal winner thank year wave sausage worth useful legal winner thank yellow'
    await expect(signer.signTransaction(bytes)).rejects.toMatchObject({ code: 'mismatch' })

    mockLedgerDevice().dev.phrase = TEST_PHRASE
    mockLedgerDevice().dev.promptDelayMs = 400
    const p = signer.signTransaction(bytes)
    await waitFor(() => useLedgerPrompt.getState().stage === 'confirm')
    cancelLedger()
    await expect(p).rejects.toMatchObject({ code: 'cancelled' })
    await new Promise((r) => setTimeout(r, 450)) // let the device finish before the next test
  })

  it('shows the udev fix when Rare OS has no access to the Ledger', async () => {
    const fix = 'pkexec sh -c "..."'
    h.openError = new NativeError('udev', 'no access', { fix })
    await expect(connectLedger('usb:mock')).rejects.toMatchObject({ code: 'udev' })
    expect(useLedgerPrompt.getState().error).toMatchObject({ code: 'udev', fix })
  })
})
