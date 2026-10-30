# First run in Rare OS

What a person sees the first time Rare Launcher starts, after installing Rare OS or switching
from GameOS. The shell is `launcher/src/screens/FirstRun.tsx`; the Wi-Fi and Steam
steps are in `launcher/src/onboarding/` (the passkey step is Rare Wallet's) and plug into it with `registerFirstRunStep`
(`onboarding/index.ts`, imported once from `main.tsx`).

## Shape

- A full-screen overlay above the launcher (overlay kind `firstrun`, also from Settings > Run
  setup). Top row: the Rare mark, four progress dots. Bottom: the hint bar.
- Four screens: Welcome, Wi-Fi, passkey, Steam. Then a full Home.
- Controls: **A** selects, **B** goes back through the steps that were shown (a step with pages of
  its own, like the passkey ceremony, goes back one page first), **Y** skips the rest of setup,
  **Quick** opens Quick Settings (Wi-Fi, brightness and volume are needed during setup) and closing
  it returns to the same step. LB/RB, Menu, the Rare button, LC and the holds do nothing until setup is done (RC opens the on-screen keyboard).
- Controller only, start to end. Focus lands on each step's main button. No skeletons: work in
  progress shows a thin bar with a line of text. No art tiles: a step never shows placeholder covers.
- Wi-Fi is passed over going forward when the device is online already (`shouldSkip` in
  `FirstRun.tsx`). Going back walks the steps that were shown. The shortest path (online, passkey
  "Not now", Steam skipped) is three A presses.
- Finishing (last step, or Y) scans the library and reads the wallet first (a thin bar and "Getting
  your games ready", at most 4 s), then shows Home with one toast: "Press the Rare button in any game
  for the Rare menu".

## Steps

| # | Step | What it calls | Done when |
|---|---|---|---|
| 1 | **Welcome** | FirstRun shell. A language row next to Get started lists `lib/locales.ts` (English now) and saves `language` in the config. | Get started |
| 2 | **Wi-Fi** | `native.netStatus` every 3 s, `native.wifiScan`, `native.wifiConnect(ssid, password)` (NetworkManager through `nmcli`, `src-tauri/src/onboard.rs`); the password comes from the on-screen keyboard | Online. Coming online on this screen moves on by itself after a moment. Offline: "Continue offline". Skipped when already online. |
| 3 | **Sign in** | Registered by Rare Wallet itself (`wallet/ui/onboard/firstrun.tsx`): passkey sign-in and create (phone over Bluetooth, or USB security key, device PIN). "Other ways to sign in" opens Rare Wallet's own pages (recovery phrase, private key, Ledger, Keystone, watch) inside setup; B on the first of them comes back. Offline it says a phone passkey needs the internet and offers "Connect to Wi-Fi". A failed or cancelled passkey stays on the step with the reason and the same buttons. After an account that can sign lands, the same step shows "You're @name" with a free suggested username (`player` plus four characters of the address, checked with `friends/username.ts`) and **Change** (the on-screen keyboard picker). Continue saves the name with one wallet signature; a failed save toasts and setup goes on. Watch-only accounts skip the name. | An account exists and Continue, or Not now |
| 4 | **Steam** | `native.steamBigPicture` to sign in; `native.steamAccount` (reads Steam's `loginusers.vdf`) on entry and whenever the launcher gets focus back | Signed in, or "Skip Steam for now". Offline it says Steam can wait. Finishing this step finishes setup. |

Target: power-on to a full Home in under two minutes. Remapping the buttons, deadzones and gyro are not part of setup; they live in Settings > Controls.

## What is remembered (launcher config, `~/.config/rare/launcher.json`)

- `onboardingStep`: the step on screen, written on every move while setup is unfinished. A
  restart reopens the flow at that step.
- `onboarded`: set by finishing or by Y (skip setup); `onboardingStep` is cleared then. The
  flow opens at start only while `onboarded` is false.

