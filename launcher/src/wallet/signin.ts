// Sign-in for Rare is a passkey, made and used inside Rare Wallet (phone over Bluetooth, or a
// USB security key). There is no Google, Apple or zkLogin path.
//
// "Sign in with Playtron" is not offered as an account sign-in: Playtron's issuer
// (https://oauth2.playtron.one) publishes an OIDC discovery document, but it has no client
// registration endpoint or third-party docs, and its only claim is `sub` (no name or
// profile to import). Borrowing Playtron's own client id would be impersonating their app.
// People bring a Playtron wallet over with "Move your assets" in Rare Wallet instead.

import { native } from "../lib/bridge";
import { useApp } from "../lib/store";
import { useWalletNav, type WalletView } from "./ui/store";

/**
 * The launcher's first-run overlay hands off to the wallet tab: close the overlay, remember that
 * sign-in was shown, and open the wallet on the page that was picked (B there goes back to the
 * welcome screen).
 */
export function openWalletPage(view: WalletView) {
  void native.config().then((c) => (c.onboarded ? undefined : native.setConfig({ ...c, onboarded: true })));
  useApp.getState().close();
  useWalletNav.getState().reset([view]);
  useApp.getState().setTab("wallet");
}
