#!/usr/bin/env bash
# Puts an OCI layout (the image the installer installs) into an installer ISO as its /container,
# in place of the one bootc-image-builder wrote. The installer's kickstart installs from that
# directory (ostreecontainer --url=/run/install/repo/container --transport=oci).
#
# Why: bootc-image-builder takes the installer's packages from the payload's /etc/yum.repos.d, and
# Fedora 45 moved its repo files to /usr/share/dnf5/repos.d, so the ISO is built from a copy of the
# image with the repo files added there; this puts the published image back, byte for byte (copied
# from the registry with its digests, so the installed system is exactly the published image).
#
#   image/installer/set-payload.sh IN.iso OCI_DIR OUT.iso
# OCI_DIR holds one image (skopeo copy --preserve-digests docker://...@sha256:... oci:OCI_DIR:<ref>).
# Needs xorriso and jq.
set -euo pipefail
in="$1" oci="$2" out="$3"
w="$(mktemp -d)"; trap 'rm -rf "$w"' EXIT
xorriso -osirrox on -indev "$in" -extract /container/index.json "$w/index.json" >/dev/null 2>&1 \
    || { echo "set-payload: $in has no /container/index.json" >&2; exit 1; }
[[ "$(jq '.manifests | length' "$oci/index.json")" == 1 ]] || { echo "set-payload: $oci must hold one image" >&2; exit 1; }
# Keep the reference name the kickstart may ask for.
old_ref="$(jq -r '.manifests[0].annotations["org.opencontainers.image.ref.name"] // empty' "$w/index.json")"
new_ref="$(jq -r '.manifests[0].annotations["org.opencontainers.image.ref.name"] // empty' "$oci/index.json")"
if [[ "$old_ref" != "$new_ref" ]]; then
    # index.json is not content-addressed: the image (manifest, config, layers) stays as it is.
    echo "set-payload: ref name '${new_ref}' -> '${old_ref}' (as the ISO had it)"
    jq --arg r "$old_ref" 'if $r == "" then del(.manifests[0].annotations["org.opencontainers.image.ref.name"])
        else .manifests[0].annotations["org.opencontainers.image.ref.name"] = $r end' "$oci/index.json" > "$w/new-index.json"
    cp "$w/new-index.json" "$oci/index.json"
fi
echo "set-payload: the ISO had $(jq -r '.manifests[0].digest' "$w/index.json"), now $(jq -r '.manifests[0].digest' "$oci/index.json")"
rm -f "$out"
xorriso -indev "$in" -outdev "$out" -boot_image any replay -rm_r /container -- -map "$oci" /container \
    2>&1 | grep -vE '^xorriso : (UPDATE|NOTE)' || true
[[ -s "$out" ]] || { echo "set-payload: no output" >&2; exit 1; }
ls -l "$out"
