#!/usr/bin/env bash
# Finishes the installer ISO bootc-image-builder makes (anaconda-iso) for a handheld:
#   - GRUB boots the install after 2 s with no menu on screen (Esc shows it): its stock menu waited
#     60 s for keys a handheld does not have
#   - the entries say "Rare OS", not "Rare OS 44" (the Fedora release the installer comes from)
#   - the install entry boots with the splash (rhgb) instead of a screen of systemd lines
# Both copies of grub.cfg change: the ISO's (BIOS and El Torito) and the one in the EFI partition
# (images/efiboot.img, appended as the ISO's EFI system partition, which USB sticks boot from).
#
#   image/installer/finish-iso.sh IN.iso OUT.iso
# Needs xorriso and mtools.
set -euo pipefail
in="$1" out="$2"
w="$(mktemp -d)"; trap 'rm -rf "$w"' EXIT
xorriso -osirrox on -indev "$in" \
    -extract /EFI/BOOT/grub.cfg "$w/efi.cfg" -extract /boot/grub2/grub.cfg "$w/bios.cfg" \
    -extract /images/efiboot.img "$w/efiboot.img" >/dev/null 2>&1
chmod u+w "$w"/*
for f in "$w/efi.cfg" "$w/bios.cfg"; do
    sed -i -E \
        -e 's/^set timeout=[0-9]+/set timeout_style=hidden\nset timeout=2/' \
        -e "s/Rare OS [0-9]+/Rare OS/g" \
        -e '0,/osbuild.ks quiet$/s//osbuild.ks quiet rhgb/' \
        "$f"
    grep -q 'timeout_style=hidden' "$f" && grep -q 'quiet rhgb' "$f" || { echo "finish-iso: $f did not change as expected" >&2; exit 1; }
done
mcopy -o -i "$w/efiboot.img" "$w/efi.cfg" ::/EFI/BOOT/grub.cfg
rm -f "$out"
xorriso -indev "$in" -outdev "$out" -boot_image any replay \
    -append_partition 2 0xef "$w/efiboot.img" \
    -map "$w/efi.cfg" /EFI/BOOT/grub.cfg -map "$w/bios.cfg" /boot/grub2/grub.cfg -map "$w/efiboot.img" /images/efiboot.img \
    2>&1 | grep -vE '^xorriso : (UPDATE|NOTE)' || true
[[ -s "$out" ]] || { echo "finish-iso: no output" >&2; exit 1; }
ls -l "$out"
