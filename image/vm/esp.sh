#!/usr/bin/bash
# Guest side of `test.sh bootloader`: what is on the EFI system partition, against the running image.
#
#   esp.sh show    the ESP's shim, GRUB and fallback shim (sha256), the image's payload, and the PCR4
#                  each pair gives (needs /var/tmp/boot-pcr4.py); last line "ESP matches image: yes|no"
set -uo pipefail
ESP_TYPE=c12a7328-f81f-11d2-ba4b-00a0c93ec93b
src="$(findmnt -nvo SOURCE /sysroot 2>/dev/null || findmnt -nvo SOURCE /)"
disk="$(lsblk -nsro PATH,TYPE "$src" | awk '$2 == "disk" { print $1 }' | head -n1)"
esp="$(lsblk -nro PATH,PARTTYPE "$disk" | awk -v t="$ESP_TYPE" 'tolower($2) == t { print $1 }' | head -n1)"
tmp="$(mktemp -d)"
mnt="$(findmnt -nro TARGET -S "$esp" | head -n1)"
if [[ -z "$mnt" ]]; then
    mnt="$(mktemp -d)"
    mount -o ro "$esp" "$mnt" || { echo "can't mount $esp"; exit 1; }
    trap 'umount "$mnt"; rmdir "$mnt"; rm -rf "$tmp"' EXIT
else
    trap 'rm -rf "$tmp"' EXIT
fi
echo "BootCurrent: $(efibootmgr -v 2>/dev/null | awk '/^BootCurrent:/ { c = $2 } c && $0 ~ "^Boot" c { print; exit }')"
sum() { [[ -f "$1" ]] && sha256sum <"$1" | cut -c1-16 || echo "-"; }
echo "ESP $esp on $disk"
for f in EFI/fedora/shimx64.efi EFI/fedora/grubx64.efi EFI/BOOT/BOOTX64.EFI; do echo "  esp   $f $(sum "$mnt/$f")"; done
ls "$mnt/EFI" "$mnt/EFI/fedora" | tr '\n' ' '; echo
shim="$(ls /usr/lib/efi/shim/*/EFI/fedora/shimx64.efi 2>/dev/null | head -n1)"
grub="$(ls /usr/lib/efi/grub2/*/EFI/fedora/grubx64.efi 2>/dev/null | head -n1)"
[[ -n "$shim" ]] || { shim=/usr/lib/bootupd/updates/EFI/fedora/shimx64.efi; grub=/usr/lib/bootupd/updates/EFI/fedora/grubx64.efi; }
echo "  image $shim $(sum "$shim")"
echo "  image $grub $(sum "$grub")"
if [[ -f /var/tmp/boot-pcr4.py ]]; then
    d="$tmp/esp/usr/lib/bootupd/updates/EFI/fedora"; mkdir -p "$d"
    cp "$mnt/EFI/fedora/shimx64.efi" "$mnt/EFI/fedora/grubx64.efi" "$d/"
    echo "PCR4 esp   $(python3 -I /var/tmp/boot-pcr4.py "$tmp/esp" | python3 -c 'import json,sys; print(json.load(sys.stdin)["value"])')"
    echo "PCR4 image $(python3 -I /var/tmp/boot-pcr4.py / | python3 -c 'import json,sys; e=json.load(sys.stdin); print(e["value"], e["id"])')"
fi
if [[ "$(sum "$mnt/EFI/fedora/shimx64.efi")" == "$(sum "$shim")" && "$(sum "$mnt/EFI/fedora/grubx64.efi")" == "$(sum "$grub")" ]]; then
    echo "ESP matches image: yes"
else
    echo "ESP matches image: no"
fi
