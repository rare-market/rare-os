# Installing Rare OS

Three ways in, easiest first. All three are undoable.

| | Switch in place | USB installer | From the image builder (developers) |
|---|---|---|---|
| For | SuiPlay0X1 on GameOS, Bazzite, Fedora Atomic | Any x86_64 handheld or PC, any OS | Testing on a Linux build machine or in CI |
| Needs | Wi-Fi and one command | A USB-C stick of 16 GB or more | Linux with podman |
| Your files and games | Kept | Erased (whole internal drive) | n/a |
| Undo | `sudo rare-undo`, or the boot menu | Reinstall the old OS from its own installer | n/a |

> Status: the switch path is tested in virtual machines (CI and the build machine, see the end of this
> page), including one with Secure Boot on and a GameOS-like setup, and Rare OS runs on a
> SuiPlay0X1 (switched from Bazzite, not from GameOS). The switch from a stock GameOS
> unit has not been run on real hardware yet. The image is not published yet, so the commands
> below work once the maintainers flip the switches in "Before the first public install".

---

## 1. Switch in place (no USB, keeps everything)

Playtron GameOS, Bazzite and Fedora Atomic all boot from a container image (bootc and
rpm-ostree). Rare OS is one too, so the device can download Rare OS and boot into it,
keeping the current system installed next to it as the fallback.

### On a SuiPlay0X1 running GameOS

You need the SuiPlay on Wi-Fi, plugged in, and a computer (or phone with an SSH app) on the
same Wi-Fi. About 30 minutes, most of it waiting for the download.

1. On the SuiPlay, open **Settings > Advanced** and turn **Remote Access** on.
2. Open **Settings > Internet > Registered Wi-Fi networks**, pick your network and write down
   its IP address (four numbers with dots, like `192.168.0.42`).
3. On the computer, open a terminal (Windows: PowerShell; Mac: Terminal) and type:

   ```
   ssh playtron@192.168.0.42
   ```

   with your SuiPlay's address. Type `yes` if it asks about a fingerprint. The password is
   `playtron` (nothing shows while you type it).
4. Type the one command:

   ```
   curl -fsSL https://rare.market/os/get | sudo bash
   ```

   It asks for the password once more: `playtron`. It checks the device, then asks
   "Download Rare OS now?": type `y` and press Enter. The download is about 8 GB and takes 10
   to 30 minutes on good Wi-Fi. Leave the SuiPlay alone until it is done (do not install a
   GameOS update in the meantime).
5. When it asks "Restart now?", type `y`. The SuiPlay restarts into Rare OS. The first start
   takes a minute or two longer than usual, then the Rare Launcher walks you through setup.

That is all for a SuiPlay as it comes from the box: its Secure Boot setting is off (GameOS
needs it off: Playtron's own install guide turns it off first), so there is no extra screen
and nothing to unlock. If someone
turned Secure Boot on, the command tells you, and the next start shows one blue screen; see
"Secure Boot" below.

No computer? Plug a USB-C keyboard into the SuiPlay, press `Ctrl` + `Alt` + `F3`, log in
as `playtron` / `playtron`, and run step 4 there.

The QR code below opens the same instructions on a phone:

![QR code for rare.market/os](get-rare-os-qr.png)

Where this comes from: the Remote Access setting, the `playtron` / `playtron` login with
`sudo` and the `Ctrl` + `Alt` + `F3` console are in the README of Playtron's open-source
[gameos](https://github.com/playtron-os/gameos) repository; its `Taskfile.yml` installs builds
on GameOS devices over SSH with the same `bootc switch` this command uses, and its
`hwctl ssh enable` (behind Remote Access) is `systemctl enable --now sshd`.

### Secure Boot

Secure Boot is a firmware setting that only starts software signed by keys the device
trusts. Rare OS is built on Bazzite, so its kernel is signed by Universal Blue (Bazzite's
makers), not by Fedora or Microsoft.

- **Secure Boot off** (every SuiPlay from the box, Steam Decks, most handhelds): nothing to do.
- **Secure Boot on**: the device has to trust Universal Blue's key once. The command does the
  first half for you (it queues the key with `mokutil --import`, one-time password `rare`) and
  lists the steps below before it downloads anything. At the next start a blue screen
  titled "Perform MOK management" appears. It needs a **USB-C keyboard**: it is driven with the
  arrow keys and Enter and asks you to type the password, which the SuiPlay's own buttons
  cannot do. Then:

  1. "Perform MOK management": press Down once to **Enroll MOK**, Enter. (The first item,
     "Continue boot", skips it.)
  2. "[Enroll MOK]": pick **Continue**, Enter.
  3. "Enroll the key(s)?": pick **Yes**, Enter.
  4. "Password:": type `rare` (nothing shows), Enter.
  5. "Perform MOK management": pick **Reboot**, Enter.

  Rare OS starts, and the screen never comes back. Skipped it? The next screen says "bad shim
  signature" and shows a menu: pick the entry that is not Rare OS to start the old system as
  before, then run the command again (or turn Secure Boot off).
- **No USB-C keyboard?** Turn Secure Boot off instead, then run the command. On AYANEO-made
  handhelds (the SuiPlay0X1 is one) Playtron's guide opens the firmware settings by holding
  **Volume Up + LC (left) + Power**, then **Security > Secure Boot > Disabled**, **Save & Exit**.
  This button combination is from Playtron's AYANEO instructions and has not been tried on a
  SuiPlay.

The key is in Rare OS at `/etc/pki/akmods/certs/akmods-ublue.der` (also published as
`secure_boot.der` in the Bazzite repository): "ublue kernel", SHA-256
`4e5c6847...dd9e70d1`. From Rare OS, `ujust enroll-secure-boot-key` queues it again
(that recipe's password is `universalblue`). With Secure Boot on, the SuiPlay fan driver Rare OS
builds itself is unsigned and may be refused; the fan then stays on the
firmware's own curve.

Tested in a VM on the build machine (`image/vm/test.sh secureboot`, Rare OS 0.1.0-3a8f2d3, 2026-10-29): OVMF
with Secure Boot and the Microsoft keys, Fedora's signed boot chain, user `playtron`, the command
run with `sudo`: it queued the key (taken from the downloaded Rare OS), the blue screen came up
straight on its menu, the five steps above enrolled it, and Rare OS reached its first-run screen
with Secure Boot still on; `rare-undo` went back. Picking "Continue boot" instead gave GRUB's
"bad shim signature" for Rare OS's kernel and its menu with the old system, as described.

### On Bazzite or Fedora Atomic

Open a terminal (desktop mode, or `Ctrl` + `Alt` + `F3`) and run the same command:

```
curl -fsSL https://rare.market/os/get | sudo bash
sudo reboot
```

### What the command does

`tools/get-rare-os.sh` shows a QR code for this guide, then runs five numbered steps and stops
with a plain explanation (and nothing changed) if one fails:

1. **Device:** x86_64, and boots from an image (bootc or rpm-ostree). Already on Rare OS: it says so and stops.
   Secure Boot on and Rare OS's key not trusted yet: it says a blue key screen follows at the
   next start and lists its steps (see "Secure Boot").
2. **Power:** battery at 30% or more, or the charger connected (`RARE_IGNORE_BATTERY=1` skips this).
3. **Space:** about 20 GB free on the system drive.
4. **Network:** the download server answers, and the Rare OS image is published and public.
5. **Switch:** lists what stays, what changes and how to undo, asks once, then runs one of:

```
sudo bootc switch --transport registry $RARE_IMAGE_REPO:stable      # bootc systems (GameOS)
sudo rpm-ostree rebase ostree-unverified-registry:$RARE_IMAGE_REPO:stable   # older rpm-ostree systems
```

   With Secure Boot on it then queues Rare OS's key (`mokutil --timeout -1`, then
   `mokutil --import` with the password `rare`; `RARE_MOK_PASSWORD` changes it), using the copy
   inside the downloaded Rare OS or the script's own copy (SHA-256 checked). If the firmware does
   not take the request, it removes the download again, so the device starts as before. On
   GameOS it reminds you to restart before using GameOS again (GameOS's own updater would
   replace the download), and it offers to restart.

Every run is logged to `/var/log/rare-os-get.log` for support.

The old OS does not have Rare's signing key, so this first download is checked by HTTPS only.
Rare OS carries the key (`/etc/pki/containers/rare-os.pub`) and a signature policy for
`$RARE_IMAGE_REPO`; on its first boot it moves its own update source to the
signature-checked one (`ostree-image-signed:docker://$RARE_IMAGE_REPO:stable`), so
every later update must carry a valid Rare signature.

### What is kept

- **Kept:** everything under `/var`: your home folder (on GameOS: `/var/home/playtron`), Steam
  and other game libraries, downloaded games, Flatpak apps, Wi-Fi networks and Bluetooth
  pairings. Rare Launcher scans Steam libraries, `~/Games` folders, other users' homes and SD
  cards, so games already on the device show up in its library.
- **Merged:** `/etc` (system settings) is carried over and merged with Rare OS's defaults.
- **Replaced:** `/usr` (the OS itself). GameOS's own apps are not part of Rare OS.
- **User:** game mode logs in as the user with id 1000. On GameOS that is `playtron`, so your
  home folder is used as it is. On a fresh install Rare OS creates a user called `rare`.

After switching from GameOS, change the well-known default password: in desktop mode open
a terminal and run `passwd`.

### Updates

Rare OS downloads the next version in the background (`rare-update.service`, daily, run by
`/usr/libexec/rare/os-update`) and stages it. It takes effect at the next restart; Settings >
System shows "Restart now" and the launcher puts "Restart to update" in the notification centre.
It never restarts the device by itself.

A staged update is a file in `/run` that `ostree-finalize-staged.service` turns into a boot entry
while the device shuts down cleanly. A forced power-off (holding the power button) skips that, and
the staged update is not installed. Nothing is broken and the downloaded data stays in the ostree
repo: `os-update` keeps a marker of what it staged (`/var/lib/rare/update-staged`), and
`rare-update-recover.service` at the next boot sees that the update was neither installed nor still
staged, logs it (`journalctl -t rare-update`) and downloads it again straight away. The update then
waits for the next clean restart. What cannot be recovered at boot is the staged state itself,
because `/run` is gone, so use Restart now (or a normal restart) rather than a forced power-off.

### Undo (rollback)

The previous system stays installed. Any of these brings it back:

- From Rare OS: `sudo rare-undo`, then reboot. It makes the system you switched from the default
  again. (`sudo bootc rollback` only steps back one Rare OS version: the first start of Rare OS
  can already add a version of its own, for hardware kernel arguments and signed updates.)
- Without a working system: with a USB-C keyboard plugged in, hold `Shift` (or tap `Esc`)
  while powering on to show the boot menu, and pick the older entry. The boot menu also
  appears by itself after a start that did not finish.
- Without a keyboard, from Rare OS: Settings > System > Boot menu > "Show next start" shows the
  boot menu once on the next start. Settings > System > "Restore previous system" does what
  `sudo rare-undo` does, with a confirmation (it is hidden when no previous system is installed;
  `rare-undo --check` tells).

Your files are the same in both systems. Updates later in Rare OS work the same way: each
update is a new system image, and `sudo bootc rollback` goes back to the previous Rare OS version.

The system you switched from (GameOS on a SuiPlay) is pinned on Rare OS's first start
(`rare-keep-previous-os.service`), so updates never push it out: it stays in the boot menu,
which matters while a Playtron wallet still needs GameOS to sign its moves. Once you no longer
need it, find its index with `rpm-ostree status` and run `sudo ostree admin pin --unpin <index>`;
the next update then removes it.

On a Rare OS installed before `rare-undo` existed, the same by hand (the old system may be third
in `rpm-ostree status` once Rare OS's first start adds a version, so `bootc rollback` is not it):

```
sudo rpm-ostree cancel; sudo rpm-ostree cleanup -p
sudo ostree admin set-default $(rpm-ostree status --json | jq '[.deployments[] | (.["container-image-reference"] // "") | test("rare-os") | not] | index(true)')
sudo systemctl reboot
```

(`cancel` stops a kernel-argument change still running from the first start; `cleanup -p` drops
anything staged, so the position counted next is the one `set-default` uses. `rare-undo` does both.)

### Switch from a file (no download server)

Until the image is published, or on a device without good internet, Rare OS can be switched to
from an OCI archive copied onto the device. Build and export it on the image builder:

```
podman build -f image/Containerfile -t localhost/rare-os:local .
skopeo copy --dest-compress --dest-compress-format gzip \
    containers-storage:localhost/rare-os:local oci-archive:rare-os.ociarchive:rare-os
```

Copy it to `/var/tmp` on the device (not `/tmp`, which is in memory and too small; about 7 GB),
then switch and restart:

```
scp rare-os.ociarchive <device>:/var/tmp/rare-os.ociarchive
ssh -t <device> 'sudo bootc switch --transport oci-archive /var/tmp/rare-os.ociarchive'
ssh -t <device> 'sudo systemctl reboot'
```

Rehearsed in a VM with `image/vm/test.sh handheld` (Bazzite with the device's own bootc 1.7.1).
What carries over matters most when SSH is the only way in. SSH does not: the first start of Rare OS
turns it off even when the previous system had it on (GameOS ships Remote Access with the password
`playtron`), until it is turned on in Settings > System; while the password is still `playtron`,
password sign-in stays off and only keys work, until a new one is set in Settings (see docs/SSH.md).
A device that already chose in Rare OS (`/var/lib/rare/ssh/state`) keeps its choice. The Wi-Fi
profiles and `~/.ssh/authorized_keys` are untouched, the SSH host keys
stay (no new fingerprint), and a device without a name of its own keeps its old default name
(`rare-keep-hostname.service`), so `ssh bazzite.local` still finds it. The name changes only when
asked: `rare.hostname=SuiPlay` in `/etc/rare/settings.conf` makes it `SuiPlay` at the next boot
(Steam's computer name is the hostname; `ssh suiplay.local` from then on). On a SuiPlay the pretty
name is `SuiPlay` and the chassis `handheld` either way. Updates from a file come
only from that file; once the image is published, move to it with
`sudo bootc switch $RARE_IMAGE_REPO:stable`.

---

## 2. USB-C installer (erase and install)

For devices that do not boot from an image (SteamOS, Windows, most Linux installs), or for a
clean start.

### Make the stick (on any computer)

1. Download `rare-os-installer.iso` from https://rare.market/os.
2. Write it to a USB-C stick (16 GB or more) with one of:
   - **Fedora Media Writer** (Windows, macOS, Linux): "Select .iso file", pick the stick, "Write".
   - **balenaEtcher** (Windows, macOS, Linux): "Flash from file", "Select target", "Flash".
   - **Rufus** (Windows): pick the ISO, keep "GPT" and "UEFI", choose **DD image mode** when asked.

A browser-only flasher is not possible: WebUSB cannot write to USB storage sticks (browsers
block the mass-storage class), so one of the apps above is needed.

### Install

1. Power off the handheld. Plug in the stick.
2. Open the firmware boot menu and pick the USB stick. On AYANEO-made handhelds (the
   SuiPlay0X1 is one) this is usually done by holding a volume key while pressing power;
   check the device's manual, this has not been confirmed on a SuiPlay yet. On other PCs it
   is usually `F12`, `F7` or `Esc` with a keyboard attached.
3. One screen: it names the internal drive it will erase, with a 20-second countdown. Hold the
   power button to cancel. Otherwise it installs and restarts into Rare OS by itself (about 10
   minutes).

The installer erases the whole internal drive. Games and files on it are gone; games on an SD
card are kept and Rare Launcher picks them up.

### Undo

Reinstall the previous OS from its own installer (GameOS, SteamOS and Windows all provide
one). A USB install cannot be rolled back in place.

How the stick is built: `image/installer/config.toml` is the configuration for
`bootc-image-builder --type anaconda-iso`; its kickstart picks the largest non-removable,
non-USB disk, shows the countdown, partitions it (EFI + btrfs) and installs the Rare OS image.

---

## 3. Developers: build and test

```
podman build -f image/Containerfile -t localhost/rare-os:test .
sudo IMAGE=localhost/rare-os:test image/vm/test.sh fresh     # install to a VM disk, boot to the launcher
sudo IMAGE=localhost/rare-os:test image/vm/test.sh rebase    # stock Fedora bootc VM -> get-rare-os.sh -> Rare OS -> rollback
sudo RARE_ARCHIVE=rare-os.ociarchive SOURCE_IMGREF=registry:quay.io/fedora/fedora-bootc:44 \
    image/vm/test.sh secureboot                               # same with Secure Boot on, GameOS-like user/os-release,
                                                              # key screen walked by the harness, rare-undo
```

`image/vm/harness.py` drives QEMU (KVM when available), screenshots through QMP and checks
the guest over SSH. In a VM there is no real GPU, so the session runs the launcher in `cage`
with software rendering instead of gamescope (`/usr/bin/rare-session` decides; set
`RARE_COMPOSITOR=gamescope` to force gamescope with GPU passthrough).

Image builds run on any x86_64 Linux machine with podman (KVM makes the VM tests fast): the two commands above.
CI: the "Rare OS image" workflow runs only when started by hand (Actions tab, "Run workflow")
or for a release tag `v*`, never on ordinary pushes. It builds the image, runs both VM tests
and uploads screenshots and logs. Options: build the installer ISO, publish.

---

