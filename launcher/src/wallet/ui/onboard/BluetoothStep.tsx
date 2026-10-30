// "Turn on Bluetooth": the phone-passkey and Ledger BLE flows need the adapter on, and on the
// SuiPlay it often starts off. This powers it on over BlueZ without sudo, then calls onReady.
// Also used when a flow hits "ble-off" because Bluetooth was turned off partway through.

import { useState } from "react";
import { Bluetooth } from "lucide-react";
import { bluetooth, NativeError } from "../../native";
import { Btn } from "../kit";

export function BluetoothStep({ onReady, onCancel, note }: { onReady: () => void; onCancel?: () => void; note?: string }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const turnOn = async () => {
    setBusy(true);
    setError(null);
    try {
      const st = await bluetooth.powerOn();
      if (st.powered) {
        onReady();
        return;
      }
      setError(st.blocked ? "Bluetooth is blocked by a switch on this device." : "Could not turn Bluetooth on. Try again.");
    } catch (e) {
      setError(e instanceof NativeError ? e.message : "Could not turn Bluetooth on.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col items-center gap-5 py-10 text-center">
      <Bluetooth size="2.5rem" className="text-icon-secondary" strokeWidth={1.5} />
      <div>
        <p className="text-xl font-semibold">Turn on Bluetooth</p>
        <p className="mt-2 max-w-[30rem] text-sm text-secondary">{note ?? "Your phone and this SuiPlay talk over Bluetooth to set up your passkey."}</p>
      </div>
      {error && <p className="max-w-[30rem] text-sm text-negative">{error}</p>}
      <Btn variant="primary" def className="w-64" onClick={() => void turnOn()}>
        {busy ? "Turning on" : "Turn on Bluetooth"}
      </Btn>
      {onCancel && (
        <Btn variant="ghost" size="md" onClick={onCancel}>
          Cancel
        </Btn>
      )}
    </div>
  );
}

/** True when a phone-passkey flow can start; false once a BluetoothStep is needed (adapter off). */
export async function bluetoothReady(): Promise<boolean> {
  try {
    const st = await bluetooth.state();
    return !st.present || st.powered;
  } catch {
    return true;
  }
}
