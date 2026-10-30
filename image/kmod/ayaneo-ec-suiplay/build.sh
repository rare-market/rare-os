#!/usr/bin/bash
# Builds ayaneo_ec_suiplay.ko for the base image's exact kernel and puts it in $OUT
# (/usr/lib/modules/<kver>/extra/ayaneo-ec-suiplay.ko, depmod runs in the final stage).
#
# Why: the SuiPlay0X1 is an AYANEO 2S whose DMI says "Mysten Labs, Inc." / "SuiPlay0X1", so the
# kernel's ayaneo-ec (AYANEO board vendor only) never binds and there is no fan hwmon for
# rare-fand. Source: drivers/platform/x86/ayaneo-ec.c from the OGC kernel tag v7.2.11-ogc4, with
# suiplay.patch (SuiPlay-only DMI table, fan quirk, as in Bazzite's kernel; charge_control=1
# adds the 2S charge quirk, BAT0 charge_behaviour, off by default).
#
# kernel-devel for the OGC kernel is not in any dnf repo; Universal Blue's akmods image for the
# same kernel (the one Bazzite installs its kernel from) carries it in /kernel-rpms.
#
# Never fails the image build: without the module the fan stays on the EC's curve, as before.
# Runs in a throwaway stage FROM the base image. On an installed device it also runs in a
# fedora:<release> container with KVER=$(uname -r).
set -uo pipefail

OUT="${1:-/out}"
SRC="$(cd "$(dirname "$0")" && pwd)"
WORK=/var/tmp/ayaneo-ec-suiplay
mkdir -p "$OUT" "$WORK"

skip() {
    echo "::warning::ayaneo-ec-suiplay not built: $*" >&2
    exit 0
}

KVER="${KVER:-$(ls /usr/lib/modules | sort -V | tail -n1)}"
FEDORA="$(rpm -E %fedora)"
echo "kernel $KVER, Fedora $FEDORA"
case "$KVER" in
    *ogc*) FLAVOR=ogc ;;
    *) skip "kernel $KVER is not an OGC kernel" ;;
esac
# The stock module must exist (same source tree) and the vendored copy must still be its driver.
if [[ -d "/usr/lib/modules/$KVER/kernel" ]]; then
    modinfo -k "$KVER" ayaneo_ec >/dev/null 2>&1 || skip "kernel $KVER has no ayaneo_ec module"
fi

IMG="ghcr.io/ublue-os/akmods:${FLAVOR}-${FEDORA}-${KVER}"
command -v skopeo >/dev/null || dnf5 -y install --setopt=install_weak_deps=False skopeo || skip "no skopeo"
skopeo copy --retry-times 3 "docker://${IMG}" "dir:${WORK}/akmods" || skip "can't pull ${IMG}"
RPM="kernel-rpms/kernel-devel-${KVER}.rpm"
for blob in "$WORK"/akmods/*; do
    # grep -q would exit early, tar would die of SIGPIPE and pipefail would fail the test.
    if tar -tf "$blob" 2>/dev/null | grep -x "$RPM" >/dev/null; then
        tar -xf "$blob" -C "$WORK" "$RPM"
        break
    fi
done
rm -rf "$WORK/akmods"
[[ -f "$WORK/$RPM" ]] || skip "no ${RPM} in ${IMG}"
dnf5 -y install --setopt=install_weak_deps=False "$WORK/$RPM" gcc make patch kmod || skip "installing kernel-devel failed"
KDIR="/usr/src/kernels/${KVER}"
[[ -f "$KDIR/Module.symvers" ]] || skip "kernel-devel has no Module.symvers"

cp "$SRC/ayaneo-ec.c" "$SRC/Makefile" "$WORK/"
patch -d "$WORK" -p1 < "$SRC/suiplay.patch" || skip "suiplay.patch does not apply"
make -C "$KDIR" M="$WORK" modules || skip "build failed against ${KVER}"

KO="$WORK/ayaneo-ec-suiplay.ko"
[[ "$(modinfo -F vermagic "$KO" | cut -d' ' -f1)" == "$KVER" ]] || skip "vermagic is not ${KVER}"
modinfo -F alias "$KO" | grep -q 'svn.*MystenLabs,Inc\..*pn.*SuiPlay0X1' || skip "module has no SuiPlay0X1 alias"
modinfo -F alias "$KO" | grep -q 'AYANEO' && skip "module would also match AYANEO boards"
modinfo -F parm "$KO" | grep -q '^charge_control:' || skip "module has no charge_control parameter"
install -Dm0644 "$KO" "$OUT/usr/lib/modules/${KVER}/extra/ayaneo-ec-suiplay.ko"
modinfo "$OUT/usr/lib/modules/${KVER}/extra/ayaneo-ec-suiplay.ko"
