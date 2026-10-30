/**
 * A @ledgerhq/hw-transport Transport whose APDUs go through the Rust side (USB HID or Bluetooth,
 * see native.ts `ledgerNative`), so @mysten/ledgerjs-hw-app-sui runs unchanged on top. Status
 * words come back inside the response; Transport.send turns non-0x9000 ones into
 * TransportStatusError, which toLedgerError maps.
 */
import { Buffer } from './shims/buffer'
import Transport from '@ledgerhq/hw-transport'
import { ledgerNative, type LedgerDevice, type LedgerNative } from '../native'
import { toLedgerError } from './errors'

export class NativeLedgerTransport extends Transport {
  constructor(
    readonly device: LedgerDevice,
    private port: Pick<LedgerNative, 'exchange' | 'close'> = ledgerNative,
  ) {
    super()
  }

  async exchange(apdu: Buffer): Promise<Buffer> {
    let res: Uint8Array
    try {
      res = await this.port.exchange(new Uint8Array(apdu))
    } catch (e) {
      const le = toLedgerError(e)
      // ends any conversation waiting on a device that is gone (see device.ts withLedger)
      if (le.code === 'disconnected') this.emit('disconnect', le)
      throw le
    }
    return Buffer.from(res)
  }

  async close(): Promise<void> {
    await this.port.close()
  }
}
