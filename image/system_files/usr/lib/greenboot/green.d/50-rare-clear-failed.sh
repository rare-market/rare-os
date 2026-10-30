#!/usr/bin/bash
# Rare OS (greenboot green.d): this start passed its checks. If red.d had recorded this very
# version as one that did not start (an earlier try of it failed, a later one worked), forget it.
set -uo pipefail
f=${RARE_UPDATE_STATE_DIR:-/var/lib/rare}/update-skip.json
[[ -r "$f" ]] || exit 0
[[ "$(jq -r '.reason // empty' "$f")" == failed ]] || exit 0
booted="$(rpm-ostree status --json 2>/dev/null | jq -r '[.deployments[] | select(.booted == true)][0].checksum // empty')"
[[ -n "$booted" && "$(jq -r '.checksum // empty' "$f")" == "$booted" ]] || exit 0
rm -f "$f"
echo "rare: this version started fine after all; no longer skipped"
