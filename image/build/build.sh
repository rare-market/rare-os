#!/usr/bin/bash
# Turns the base image into Rare OS. Runs inside the final image stage.
set -xeuo pipefail

RARE_VERSION="${RARE_VERSION:-0.1.0}"
REPO="${RARE_IMAGE_REPO:?RARE_IMAGE_REPO (registry/name of the update source) is required}"
# The update source, read by rare-undo, keep-previous-os, signed-switch and os-update on the device.
install -Dm0644 /dev/stdin /usr/lib/rare/image-repo <<<"$REPO"
mkdir -p /etc/containers/registries.d
printf 'docker:\n  %s:\n    use-sigstore-attachments: true\n' "$REPO" > /etc/containers/registries.d/rare-os.yaml

echo "::group::packages"
# cage: fallback compositor for VMs without a real GPU. xprop: gamescope FPS limit atom.
dnf5 -y install --setopt=install_weak_deps=False cage xprop || dnf5 -y install cage xorg-x11-utils
# gpu-screen-recorder: the clip ring buffer in RAM on the GPU's encoder, and screenshots (rare-share
# prefers it; GStreamer on gamescope's PipeWire node is the fallback). From Terra when the base has
# it, else the COPR. The image still builds without it.
if ! rpm -q gpu-screen-recorder >/dev/null 2>&1; then
    if ! dnf5 -y install --setopt=install_weak_deps=False --enablerepo='terra*' gpu-screen-recorder; then
        if dnf5 -y copr enable brycensranch/gpu-screen-recorder-git; then
            dnf5 -y install --setopt=install_weak_deps=False gpu-screen-recorder || echo "gpu-screen-recorder not installed: clips use GStreamer"
            dnf5 -y copr disable brycensranch/gpu-screen-recorder-git || true
        fi
    fi
fi
# Sunshine: streaming this device to Moonlight (rare-stream host.rs runs it with Rare's own
# config; off until the owner turns it on). LizardByte's package sets cap_sys_admin on the binary
# for KMS capture under gamescope, and its uinput/uhid rules for the virtual controllers. The image
# still builds without it.
if ! rpm -q Sunshine >/dev/null 2>&1; then
    if dnf5 -y copr enable lizardbyte/stable; then
        dnf5 -y install --setopt=install_weak_deps=False Sunshine || echo "Sunshine not installed: streaming this device is off"
        dnf5 -y copr disable lizardbyte/stable || true
    fi
fi
if rpm -q Sunshine >/dev/null 2>&1; then
    # Rare starts it; its own user unit and menu entries stay out of the way.
    systemctl --global disable app-dev.lizardbyte.app.Sunshine.service 2>/dev/null || true
    for f in /usr/share/applications/dev.lizardbyte.app.Sunshine*.desktop; do
        [[ -f "$f" ]] || continue
        sed -i '0,/^\[Desktop Entry\]/s//[Desktop Entry]\nNoDisplay=true/' "$f"
    done
    getcap "$(readlink -f /usr/bin/sunshine)" | grep -q cap_sys_admin || setcap cap_sys_admin,cap_sys_nice+p "$(readlink -f /usr/bin/sunshine)"
fi
# gpu-screen-recorder captures the screen through its KMS helper, which needs cap_sys_admin.
if [[ -x /usr/bin/gsr-kms-server ]]; then
    getcap /usr/bin/gsr-kms-server | grep -q cap_sys_admin || setcap cap_sys_admin+ep /usr/bin/gsr-kms-server
fi
# Node.js runs arRPC (Discord Rich Presence for Discord on the web, /usr/libexec/rare/arrpc).
dnf5 -y install --setopt=install_weak_deps=False nodejs || echo "nodejs not installed: Discord on the web shows no game"
# tpm2-tools: the launcher's device attestation (rare-attest; Device Pass, verified trophies). The base
# ships it today; installed here so a base change can't drop it.
rpm -q tpm2-tools >/dev/null || dnf5 -y install --setopt=install_weak_deps=False tpm2-tools
# InputPlumber 0.83.1 (upstream's RPM, pinned by sha256) over the base's 0.83.0: 0.83.0 never
# releases what its D-Bus target holds when the intercept mode goes back to Pass (a direction held
# over the Rare menu's switch kept repeating in the launcher), and its CompositeDevice.SendEvent
# panics ("Cannot block the current thread"), which the journeys harness uses. Skipped once the base
# ships 0.83.1 or newer. Upstream's RPM leaves out Fedora's modules-load.d file for the IMU's hrtimer
# trigger, so it is put back.
IP_VER=0.83.1
IP_RPM="inputplumber-${IP_VER}-1.x86_64.rpm"
IP_SHA=43cf4637e6fd36d2fbe42a3fadf7466f724ecbb6744309b5284a563414a957e4
if [[ "$(printf '%s\n' "$IP_VER" "$(rpm -q --qf '%{VERSION}' inputplumber 2>/dev/null || echo 0)" | sort -V | head -n1)" != "$IP_VER" ]]; then
    curl -fsSL --retry 3 -o "/tmp/$IP_RPM" "https://github.com/ShadowBlip/InputPlumber/releases/download/v${IP_VER}/${IP_RPM}"
    echo "$IP_SHA  /tmp/$IP_RPM" | sha256sum -c -
    dnf5 -y install --setopt=install_weak_deps=False "/tmp/$IP_RPM"
    rm -f "/tmp/$IP_RPM"
fi
[[ -f /usr/lib/modules-load.d/inputplumber-hrtimer.conf ]] || install -Dm0644 /dev/stdin /usr/lib/modules-load.d/inputplumber-hrtimer.conf <<<"iio_trig_hrtimer"
rpm -q inputplumber
dnf5 clean all
echo "::endgroup::"

echo "::group::session"
# Game mode starts Rare instead of Steam. Steam stays installed (one press away).
grep -rl "gamescope-session-ogui-steam.desktop\|gamescope-session-steam.desktop" \
    /usr/libexec/bazzite-autologin /usr/lib/sddm /etc/sddm.conf.d /usr/share/steamos-manager 2>/dev/null \
  | xargs -r sed -i \
      -e 's/gamescope-session-ogui-steam\.desktop/gamescope-session-rare.desktop/g' \
      -e 's/gamescope-session-steam\.desktop/gamescope-session-rare.desktop/g'
grep -rn "gamescope-session" /usr/libexec/bazzite-autologin /usr/lib/sddm/sddm.conf.d /usr/share/steamos-manager/platform.toml || true
# steamos-manager's game mode session. Its [session] table defaults (in its code, not in the file)
# to gamescope-session-plus@ogui-steam, so "Return to Rare" (return-to-gamemode), os-session-select
# gamescope and Steam's Return to Gaming Mode (all `steamosctl switch-to-game-mode` or
# set-default-login-mode game) logged in to OpenGamepadUI + Steam. It reads only this file
# (steamos-manager-powerstation, src/platform.rs: no /etc or .d overrides), so the keys go in here.
# The service is the unit it watches to report game mode; the desktop file is what it writes to sddm.
PT=/usr/share/steamos-manager/platform.toml
touch "$PT"
sed -i '/^gamescope_session_\(service\|desktop\)[[:space:]]*=/d' "$PT"
grep -q '^\[session\]' "$PT" || printf '\n[session]\n' >> "$PT"
sed -i '/^\[session\]/a gamescope_session_service = "gamescope-session-plus@rare.service"\ngamescope_session_desktop = "gamescope-session-rare.desktop"' "$PT"
[[ "$(grep -c '^gamescope_session_\(service = "gamescope-session-plus@rare.service"\|desktop = "gamescope-session-rare.desktop"\)$' "$PT")" == 2 ]] \
    || { echo "session: steamos-manager's game mode session is not Rare" >&2; exit 1; }
# No other game mode session: the Steam and OpenGamepadUI + Steam entries leave sddm's list, so
# nothing logs in to them (Steam runs inside Rare's session). rare-user-setup moves an old
# Session= that names them (a device switched from the base) to Rare.
rm -f /usr/share/wayland-sessions/gamescope-session-ogui-steam.desktop \
      /usr/share/wayland-sessions/gamescope-session-steam.desktop
ls /usr/share/wayland-sessions
test -f /usr/share/wayland-sessions/gamescope-session-rare.desktop
# One Steam client branch in both modes. Rare's game mode starts Steam without -steamdeck (the
# public client), but the base's bazzite-steam (Desktop mode autostart and menu entries) adds
# -steamdeck on "deck" images "to maintain the Steam update branch between desktop & Steam Game
# Mode", so each switch flipped the client beta ('' <-> steamdeck_stable) and Steam reinstalled
# itself (~25 s, its updater window on screen) at the next start in the other mode.
if [[ -f /usr/bin/bazzite-steam ]]; then
    sed -i 's/^\([[:space:]]*\)DECK_OPTION="-steamdeck"/\1DECK_OPTION=""/' /usr/bin/bazzite-steam
    ! grep -q 'DECK_OPTION="-steamdeck"' /usr/bin/bazzite-steam \
        || { echo "session: bazzite-steam still starts Steam with -steamdeck" >&2; exit 1; }
fi
# The power button is the launcher's in Rare's session (powerbtn.rs: press sleeps, hold opens the
# power menu). steamos-powerbuttond would forward it to Steam, which is not this session's UI, so
# it stays with the Steam session only.
rm -f /usr/lib/systemd/user/gamescope-session-plus@rare.service.wants/steamos-powerbuttond.service
test -x /usr/share/gamescope-session-plus/gamescope-session-plus
echo "::endgroup::"

echo "::group::branding"
FEDORA_VERSION="$(rpm -E %fedora)"
BASE_VERSION="$(sed -n 's/^VERSION_ID=//p' /usr/lib/os-release | tr -d '"')"
# ID stays as the base sets it: its tools key off it. Everything people read says Rare OS.
sed -i \
    -e "s/^NAME=.*/NAME=\"Rare OS\"/" \
    -e "s/^PRETTY_NAME=.*/PRETTY_NAME=\"Rare OS\"/" \
    -e "s|^HOME_URL=.*|HOME_URL=\"https://rare.market\"|" \
    -e "s|^DOCUMENTATION_URL=.*|DOCUMENTATION_URL=\"https://rare.market\"|" \
    -e "s|^SUPPORT_URL=.*|SUPPORT_URL=\"https://rare.market\"|" \
    -e "s|^BUG_REPORT_URL=.*|BUG_REPORT_URL=\"https://rare.market\"|" \
    -e "s/^DEFAULT_HOSTNAME=.*/DEFAULT_HOSTNAME=\"rare\"/" \
    -e "s/^LOGO=.*/LOGO=rare-logo/" \
    -e "s/^ANSI_COLOR=.*/ANSI_COLOR=\"0;38;2;61;90;254\"/" \
    -e "s/^VERSION=.*/VERSION=\"${RARE_VERSION} (Fedora ${FEDORA_VERSION})\"/" \
    -e "s/^VERSION_CODENAME=.*/VERSION_CODENAME=\"Rare\"/" \
    -e "s/^BOOTLOADER_NAME=.*/BOOTLOADER_NAME=\"Rare OS ${RARE_VERSION}\"/" \
    /usr/lib/os-release
grep -q '^DEFAULT_HOSTNAME=' /usr/lib/os-release || echo 'DEFAULT_HOSTNAME="rare"' >> /usr/lib/os-release
grep -q '^BOOTLOADER_NAME=' /usr/lib/os-release || echo "BOOTLOADER_NAME=\"Rare OS ${RARE_VERSION}\"" >> /usr/lib/os-release
grep -q '^RARE_VERSION=' /usr/lib/os-release || echo "RARE_VERSION=\"${RARE_VERSION}\"" >> /usr/lib/os-release
grep -q '^RARE_BASE_VERSION=' /usr/lib/os-release || echo "RARE_BASE_VERSION=\"${BASE_VERSION}\"" >> /usr/lib/os-release
# grub2-mkconfig takes the distributor name from here.
echo "Rare OS release ${FEDORA_VERSION}" > /etc/system-release
# No hostname is written to /etc/hostname: fresh installs are called "rare" through DEFAULT_HOSTNAME,
# and a name there would rename devices that switch from another OS (rare-keep-hostname keeps theirs).
cat /usr/lib/os-release

# Image info: keys stay (the base's scripts read them), values point at Rare OS. The name keeps
# "deck": the base's hardware setup deletes the game-mode autologin on images whose name lacks
# it, and its user setup and Steam launcher pick handheld behaviour from it (first VM boot
# landed on a password prompt with "rare-os").
INFO=/usr/share/ublue-os/image-info.json
if [[ -f $INFO ]]; then
    jq --arg ref "ostree-image-signed:docker://${REPO}" --arg v "$RARE_VERSION" \
       '.["image-name"]="rare-os-deck" | .["image-vendor"]="rare" | .["image-ref"]=$ref | .["version-pretty"]=("Rare OS " + $v)' \
       "$INFO" > "$INFO.new" && mv "$INFO.new" "$INFO"
    cat "$INFO"
fi

# Boot splash: our gem on rare's background, the stock spinner animation.
if [[ ! -d /usr/share/plymouth/themes/spinner ]]; then dnf5 -y install plymouth-theme-spinner; fi
for f in /usr/share/plymouth/themes/spinner/*.png; do
    b="$(basename "$f")"
    [[ "$b" == watermark.png ]] && continue
    cp -n "$f" /usr/share/plymouth/themes/rare/
done
# plymouthd reads each [Daemon] key from /etc/plymouth/plymouthd.conf, then from the distribution
# defaults, and needs DeviceTimeout from one of them: without it the timeout is NaN and plymouthd
# aborts at start (assert seconds > 0 in ply_event_loop_watch_for_timeout), so plymouth-start
# failed and boot showed text. Both files carry every key; dracut below copies both into the
# initramfs, and the check after it fails the build if they did not make it.
plymouth-set-default-theme rare
mkdir -p /usr/share/plymouth /etc/plymouth
PLYMOUTH_CONF='[Daemon]\nTheme=rare\nShowDelay=0\nDeviceTimeout=8\n'
printf "$PLYMOUTH_CONF" > /usr/share/plymouth/plymouthd.defaults
printf "$PLYMOUTH_CONF" > /etc/plymouth/plymouthd.conf

# Terminal greeting.
if [[ -e /usr/libexec/ublue-motd ]]; then
    install -m 0755 /usr/libexec/rare/motd /usr/libexec/ublue-motd
fi
# hostnamectl's "OS Image" and "CPE OS Name" (nothing in the base reads either; ID and VARIANT_ID stay).
sed -i \
    -e "s/^IMAGE_ID=.*/IMAGE_ID=\"rare-os-${RARE_VERSION}\"/" \
    -e "s|^CPE_NAME=.*|CPE_NAME=\"cpe:/o:rare:rare-os:${FEDORA_VERSION}\"|" \
    /usr/lib/os-release

BRAND=/usr/share/rare/brand
# Steam's startup and suspend clips. The base's steam-tweaks (run at every game mode start) copies
# /usr/share/ublue-os/bazzite/bazzite*.webm into ~/.local/share/Steam/config/uioverrides/movies for any
# missing file, and `ujust configure-override-videos enable` does the same: both get Rare's clips
# from here on. The hashes of the base's clips go to bazzite-files.sha256 first, so the session-start
# rare-user-branding.service can tell Bazzite's copies in a home folder from the user's own.
for f in /usr/share/ublue-os/bazzite/bazzite*.webm; do
    [[ -f "$f" ]] || continue
    sha256sum "$f" | cut -d' ' -f1 >> "$BRAND/bazzite-files.sha256"
    case "$(basename "$f")" in
        *suspend*) cp "$BRAND/steam-movies/rare-suspend.webm" "$f" ;;
        *)         cp "$BRAND/steam-movies/rare-startup.webm" "$f" ;;
    esac
done
sort -u -o "$BRAND/bazzite-files.sha256" "$BRAND/bazzite-files.sha256"
grep -q steam-tweaks /usr/share/gamescope-session-plus/sessions.d/steam || echo "branding: steam-tweaks is no longer run by the Steam session" >&2
systemctl --global enable rare-user-branding.service

# The Rare mark replaces the base's logo icons (Kickoff menu button, About this System, dialogs).
for name in start-here distributor-logo distributor-logo-white distributor-logo-steamdeck \
            bazzite-logo bazzite-logo-white bazzite-logo-le bazzite-logo-icon; do
    find /usr/share/icons/hicolor -name "${name}.png" -o -name "${name}.svg" | while read -r icon; do
        size="$(sed -n 's|.*/hicolor/\([0-9]*\)x[0-9]*/.*|\1|p' <<<"$icon")"
        if [[ "$icon" == *.svg ]]; then
            cp "$BRAND/icons/rare-logo.svg" "$icon"
        elif [[ -n "$size" && -f "$BRAND/icons/${size}.png" ]]; then
            cp "$BRAND/icons/${size}.png" "$icon"
        else
            rm -f "$icon"
        fi
    done
done
install -Dm0644 "$BRAND/icons/rare-logo.svg" /usr/share/icons/hicolor/scalable/apps/rare-logo.svg
for s in 16 22 24 32 48 64 96 128; do
    install -Dm0644 "$BRAND/icons/${s}.png" "/usr/share/icons/hicolor/${s}x${s}/apps/rare-logo.png"
done
cp "$BRAND/icons/256.png" /usr/share/pixmaps/system-logo-white.png

# Desktop mode: About this System, the login splash, the wallpaper, the "Return" shortcut.
for f in /etc/xdg/kcm-about-distrorc /usr/etc/xdg/kcm-about-distrorc; do
    [[ -e "$f" || "$f" == /etc/* ]] || continue
    mkdir -p "$(dirname "$f")"
    printf '[General]\nLogoPath=/usr/share/pixmaps/rare-logo.png\nName=Rare OS\nWebsite=https://rare.market\nVersion=%s\n' "$RARE_VERSION" > "$f"
done
find /usr/share/plasma/look-and-feel -path '*/splash/images/bazzite_logo.svgz' | while read -r f; do
    gzip -9n < "$BRAND/icons/rare-logo.svg" > "$f"
done
install -d /usr/share/wallpapers/Rare/contents/images
cp "$BRAND/wallpaper/metadata.json" /usr/share/wallpapers/Rare/
cp "$BRAND/wallpaper/contents/screenshot.png" /usr/share/wallpapers/Rare/contents/
cp "$BRAND/wallpaper/contents/images/"*.png /usr/share/wallpapers/Rare/contents/images/
find /usr/share/plasma/look-and-feel -path '*plasmoidsetupscripts/org.kde.plasma.folder.js' \
    -exec sed -i 's#/usr/share/wallpapers/convergence\.jxl#/usr/share/wallpapers/Rare/#' {} +
find /usr/share/plasma/look-and-feel -path '*plasmoidsetupscripts/org.kde.plasma.kickoff.js' \
    -exec sed -i -E 's/distributor-logo-steamdeck|bazzite-logo[a-z-]*/rare-logo/g' {} +
for f in /etc/skel/Desktop/Return.desktop /usr/etc/skel/Desktop/Return.desktop; do
    [[ -f "$f" ]] || continue
    sed -i -e 's/^Name=Return to Gaming Mode$/Name=Return to Rare/' -e 's/^Icon=.*/Icon=rare-logo/' "$f"
done
# The base's announcements, updater, setup portal and docs: Rare has its own updates and help.
rm -f /etc/xdg/autostart/bazzite-announcement.desktop /usr/etc/xdg/autostart/bazzite-announcement.desktop
for app in io.github.rfrench3.bazzite-updater io.github.ublue_os.yafti_gtk bazzite-documentation; do
    f="/usr/share/applications/${app}.desktop"
    [[ -f "$f" ]] || continue
    sed -i '/^NoDisplay=/d; /^Hidden=/d' "$f"
    sed -i '0,/^\[Desktop Entry\]/s//[Desktop Entry]\nNoDisplay=true/' "$f"
done
# Terminal: fastfetch with Rare's name instead of the base's logo and image line.
if [[ -f /etc/profile.d/bazzite-neofetch.sh ]]; then
    rm -f /etc/profile.d/bazzite-neofetch.sh /usr/etc/profile.d/bazzite-neofetch.sh
fi
install -Dm0644 /usr/share/rare/fastfetch.jsonc /etc/xdg/fastfetch/config.jsonc
gtk-update-icon-cache -f /usr/share/icons/hicolor 2>/dev/null || true
# Android (Waydroid, Library > Android): the SuiPlay's panel size and density for the base's
# container start script (/usr/libexec/waydroid-container-start reads this file as root).
WD=/etc/default/waydroid-launcher
if [[ -f "$WD" ]] && ! grep -q SuiPlay0X1 "$WD"; then
    sed -i 's/^\t"Jupiter" | /\t"SuiPlay0X1")\n\t\tWAYDROID_WIDTH=1920\n\t\tWAYDROID_HEIGHT=1200\n\t\tWAYDROID_DENSITY=320 ;;\n&/' "$WD"
    grep -q SuiPlay0X1 "$WD" || echo "waydroid-launcher: SuiPlay0X1 case not added" >&2
fi
echo "::endgroup::"

echo "::group::updates and signatures"
# Every update after the first is checked against our key.
POLICY=/etc/containers/policy.json
jq --arg repo "$REPO" '.transports.docker[$repo] = [{"type":"sigstoreSigned","keyPath":"/etc/pki/containers/rare-os.pub","signedIdentity":{"type":"matchRepository"}}]' \
    "$POLICY" > "$POLICY.new" && mv "$POLICY.new" "$POLICY"
jq --arg repo "$REPO" '.transports.docker[$repo]' "$POLICY"
systemctl enable rare-user-setup.service rare-keep-previous-os.service rare-signed-switch.timer rare-power-defaults.service rare-audio-defaults.service rare-boot-menu.service rare-update.timer rare-update-recover.service rare-factory-reset.service
# Speaker EQ chain: the gate drops it on non-SuiPlay hardware; the user
# service flips wet/dry with the active port.
systemctl enable rare-speaker-eq-gate.service
# Rare Wallet PIN accounts: the wrong-PIN count and lock live with root, sealed by the TPM.
systemctl enable rare-pin-guard.service
systemctl --global enable rare-speaker-eq-route.service
# SSH is off by default (owner request; it used to be forced on). The owner turns it on in Settings
# > System (os-admin ssh on); rare-keep-ssh applies that choice at every boot (10-rare.preset).
systemctl disable sshd.service 2>/dev/null || true
systemctl enable rare-keep-ssh.service
# A device switched from another OS keeps its network name (bazzite.local stays bazzite.local);
# rare.hostname in /etc/rare/settings.conf renames it, and a SuiPlay's pretty name is SuiPlay.
systemctl enable rare-keep-hostname.service
# Rare's own updater replaces the base's.
systemctl disable uupd.timer 2>/dev/null || true
# A bad update rolls back by itself: greenboot runs Rare's check (/usr/lib/greenboot/check/
# required.d/50-rare-session.sh: the launcher stays up) on the first starts of a new version.
# One retry, then the previous version (each try waits up to 5 minutes for the launcher).
GB=/etc/greenboot/greenboot.conf
if [[ -f "$GB" ]]; then
    if grep -q '^GREENBOOT_MAX_BOOT_ATTEMPTS=' "$GB"; then
        sed -i 's/^GREENBOOT_MAX_BOOT_ATTEMPTS=.*/GREENBOOT_MAX_BOOT_ATTEMPTS=1/' "$GB"
    else
        echo 'GREENBOOT_MAX_BOOT_ATTEMPTS=1' >>"$GB"
    fi
fi
systemctl enable greenboot-healthcheck.service greenboot-set-rollback-trigger.service 2>/dev/null || true
echo "::endgroup::"

echo "::group::background services"
# bazzite-iwd-migration is disabled in the preset below. At run time it unmasked wpa_supplicant
# (not masked in this image) and deleted this NetworkManager file (wifi.powersave=2) on every boot,
# so the file never took effect; delete it here to keep that behaviour.
rm -f /etc/NetworkManager/conf.d/99-valve-wifi-backend.conf /usr/etc/NetworkManager/conf.d/99-valve-wifi-backend.conf
# Units a handheld never needs (VMs, RAID/LVM/iSCSI, modem, CEC, Intel thermald, ...).
# The decisions live in 05-rare-os.preset, which sorts before every other preset file;
# apply it to each unit it names that the image has.
PRESET=/usr/lib/systemd/system-preset/05-rare-os.preset
UNITS=()
while read -r verb pattern _; do
    [[ "$verb" == enable || "$verb" == disable ]] || continue
    for f in /usr/lib/systemd/system/$pattern; do
        if [[ -e "$f" ]]; then UNITS+=("$(basename "$f")"); fi
    done
done < "$PRESET"
systemctl preset "${UNITS[@]}"
for u in "${UNITS[@]}"; do echo "$u $(systemctl is-enabled "$u" 2>/dev/null || true)"; done
echo "::endgroup::"

echo "::group::initramfs"
KVER="$(ls /usr/lib/modules | sort -V | tail -n1)"
# Still generic (no-hostonly), but without nouveau: the image targets AMD and Intel handhelds, and
# nouveau pulls 100 MB of NVIDIA GSP firmware into the initramfs (254 MB -> 149 MB on the SuiPlay
# build, which GRUB reads from disk and the kernel unpacks on every boot). The initramfs only needs
# a GPU driver for the splash; an NVIDIA GPU still gets nouveau from the root filesystem after
# switch-root, and the splash runs on simpledrm until then.
/usr/bin/dracut --no-hostonly --omit-drivers "nouveau" --kver "$KVER" --reproducible --zstd -v --add ostree --add fido2 -f "/usr/lib/modules/$KVER/initramfs.img"
chmod 0600 "/usr/lib/modules/$KVER/initramfs.img"
# The boot splash's settings made it in (a missing DeviceTimeout crashes plymouthd).
# Unpacked, not read with lsinitrd -f: the two files are identical, dracut hardlinks them, and cpio
# keeps the data on the last link only, so -f shows the first one as empty. The kernel (and cpio -i)
# resolve the links, so the booted initramfs has both files in full.
INITRD_CHECK="$(mktemp -d)"
(cd "$INITRD_CHECK" && lsinitrd --unpack "/usr/lib/modules/$KVER/initramfs.img" etc/plymouth/plymouthd.conf usr/share/plymouth/plymouthd.defaults)
for f in etc/plymouth/plymouthd.conf usr/share/plymouth/plymouthd.defaults; do
    grep -q '^DeviceTimeout=' "$INITRD_CHECK/$f" || { echo "initramfs: $f has no DeviceTimeout" >&2; exit 1; }
done
rm -rf "$INITRD_CHECK"
# (Whole outputs into variables: grep -q closing a pipe early would trip pipefail.)
listing="$(lsinitrd "/usr/lib/modules/$KVER/initramfs.img")"
grep -q 'plymouth/themes/rare/rare.plymouth' <<<"$listing" || { echo "initramfs: no rare plymouth theme" >&2; exit 1; }
# The initramfs stays small: the NVIDIA firmware nouveau would bring is 100 MB of boot-time disk reads.
if grep -q 'lib/firmware/nvidia/' <<<"$listing"; then echo "initramfs: NVIDIA firmware is in it" >&2; exit 1; fi
ls -l "/usr/lib/modules/$KVER/initramfs.img"
echo "::endgroup::"

echo "::group::rare:// links"
# rare-launcher.desktop handles x-scheme-handler/rare (docs/LOCAL-API.md): xdg-open rare://... from a
# game, a browser or a notice starts the launcher, or hands the link to the running one.
update-desktop-database /usr/share/applications
grep -q 'x-scheme-handler/rare=.*rare-launcher.desktop' /usr/share/applications/mimeinfo.cache || { echo "rare:// is not registered" >&2; exit 1; }
echo "::endgroup::"

echo "::group::check branding"
# Fail the build if the obvious places still name the base.
if grep -qi "bazzite\|universal blue\|playtron" /etc/system-release; then exit 1; fi
if grep -qE "^(NAME|PRETTY_NAME|BOOTLOADER_NAME|IMAGE_ID|CPE_NAME)=.*[Bb]azzite" /usr/lib/os-release; then exit 1; fi
if grep -qi bazzite /etc/xdg/kcm-about-distrorc; then echo "About this System still says Bazzite" >&2; exit 1; fi
for f in /usr/share/ublue-os/bazzite/bazzite*.webm; do
    [[ -f "$f" ]] || continue
    if grep -q "^$(sha256sum "$f" | cut -d' ' -f1)\$" /usr/share/rare/brand/bazzite-files.sha256; then
        echo "$f is still Bazzite's clip" >&2; exit 1
    fi
done
if [[ -e /etc/xdg/autostart/bazzite-announcement.desktop ]]; then exit 1; fi
# Report (not fail) other user-visible strings for follow-up.
grep -rIl -i "bazzite" /usr/share/wayland-sessions /usr/share/xsessions 2>/dev/null || true
echo "::endgroup::"

rm -rf /var/cache/* /var/log/* /tmp/* 2>/dev/null || true
