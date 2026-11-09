#!/usr/bin/bash
# Checks a pushed image the way a device does before an update: containers/image with the device's
# policy (sigstoreSigned, keyPath image/keys/rare-os.pub, matchRepository) and registries.d
# (use-sigstore-attachments), as set in image/build/build.sh. Devices read only legacy cosign
# attachments (the sha256-<digest>.sig tag); `cosign verify` also accepts cosign's newer bundle
# format, so it can pass while every device rejects the update.
#
#   tools/check-device-signature.sh <repo> <sha256:digest> [authfile]
#
# Exits non-zero if the .sig tag is missing or the policy rejects the image.
set -euo pipefail

repo="${1:?image repo}"
digest="${2:?sha256:digest}"
authfile="${3:-}"
key="$(cd "$(dirname "$0")/.." && pwd)/image/keys/rare-os.pub"
[ -s "$key" ] || { echo "missing $key" >&2; exit 1; }

if ! command -v skopeo >/dev/null; then
  sudo apt-get update -qq && sudo apt-get install -y -qq --no-install-recommends skopeo >/dev/null
fi

work="$(mktemp -d "${RUNNER_TEMP:-/tmp}/devsig.XXXXXX")"
trap 'rm -rf "$work"' EXIT
auth=()
[ -n "$authfile" ] && auth=(--authfile "$authfile")

# 1. The legacy attachment tag exists (registry API, no policy involved).
sigtag="${digest/:/-}.sig"
if ! skopeo inspect --raw --retry-times 3 "${auth[@]}" "docker://$repo:$sigtag" >/dev/null; then
  echo "no $repo:$sigtag: the image is not signed in the format devices read" >&2
  exit 1
fi
echo "found $repo:$sigtag"

# 2. The device's policy accepts the image. skopeo inspect does not apply policy; copy does, before
# any layer is fetched, so a rejected image fails here with the error a device shows.
mkdir -p "$work/registries.d"
printf 'docker:\n  %s:\n    use-sigstore-attachments: true\n' "$repo" > "$work/registries.d/rare-os.yaml"
printf '{"default":[{"type":"reject"}],"transports":{"docker":{"%s":[{"type":"sigstoreSigned","keyPath":"%s","signedIdentity":{"type":"matchRepository"}}]}}}\n' \
  "$repo" "$key" > "$work/policy.json"

skopeo --policy "$work/policy.json" --registries.d "$work/registries.d" \
  copy --retry-times 3 "${auth[@]}" "docker://$repo@$digest" "dir:$work/image" >/dev/null
echo "device policy accepts $repo@$digest (image/keys/rare-os.pub)"
