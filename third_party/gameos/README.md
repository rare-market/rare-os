# Files from Playtron GameOS

Source: https://github.com/playtron-os/gameos (MIT licence, see `LICENSE` here), commit
`457a06cda7e0e4b6fb29697c71b21e218c3a1d31`. Only these two open configuration files are
used. Nothing from Playtron's closed components (Grid, Playserve) is used, downloaded or
studied, and their attribution files and package lists are not part of Rare OS.

| File | What it does on the SuiPlay0X1 | Why Rare OS still needs it |
|---|---|---|
| `rootfs/usr/lib/udev/hwdb.d/59-sui.hwdb` | Maps the extra buttons (LC, RC, the logo button and Quick Access), which arrive as raw scancodes on the AT keyboard, to F15-F18, for both BIOS scancode layouts. | Bazzite's keyboard hwdb override matches DMI vendor `AYANEO` only; the SuiPlay reports `Mysten Labs, Inc.`. InputPlumber's upstream SuiPlay profile expects these keys. |
| `rootfs/usr/lib/udev/rules.d/50-suiplay0x1.rules` | Gives the logged-in user access to `/dev/ttyS2` on a SuiPlay0X1 (the controller/LED microcontroller's serial port, AYANEO 2S design). | Not in Bazzite. Lets user-level tools talk to the MCU without root. |

Already handled upstream, so not copied: the InputPlumber device profile
(`50-suiplay_0x1.yaml` in InputPlumber), PowerStation's TDP range (5-28 W), and Bazzite's
handheld detection for the SuiPlay0X1 (ublue-os/bazzite PR #5830).
