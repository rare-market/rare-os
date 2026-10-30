// Plugs the first-run steps built here into the shell in screens/FirstRun.tsx: Wi-Fi and the Steam hand-off.
// Moving assets from a Playtron wallet (rescue/ scan and plan) is a Home card and Wallet > Move from Playtron.
// Imported once from main.tsx. The passkey step is Rare Wallet's (wallet/ui/onboard/firstrun.tsx).

import { registerFirstRunStep } from "../screens/FirstRun";
import { Steam } from "./steps/Steam";
import { Wifi } from "./steps/Wifi";

registerFirstRunStep("wifi", Wifi);
registerFirstRunStep("steam", Steam);

export { MoveAssets } from "./move/MoveAssets";
