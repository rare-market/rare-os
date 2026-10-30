#!/usr/bin/bash
# Rare OS (greenboot red.d): a health check failed on this start. Record the running version so
# the updater (os-update) never stages it again once greenboot has gone back to the previous one,
# and so Settings can say what happened. Only on the first starts of a new version (boot_counter
# set); a normal start that fails a check is not about an update.
set -uo pipefail
STATE_DIR=${RARE_UPDATE_STATE_DIR:-/var/lib/rare}
grub2-editenv - list 2>/dev/null | grep -q '^boot_counter=' || exit 0
json="$(rpm-ostree status --json 2>/dev/null)" || exit 0
d="$(jq -c '[.deployments[] | select(.booted == true)][0] | {digest: (.["container-image-reference-digest"] // null), version: (.version // null), checksum: (.checksum // null)}' <<<"$json")" || exit 0
mkdir -p "$STATE_DIR"
jq --argjson at "$(date +%s)" '. + {reason: "failed", at: $at}' <<<"$d" >"$STATE_DIR/update-skip.json.new" &&
    chmod 0644 "$STATE_DIR/update-skip.json.new" && mv -f "$STATE_DIR/update-skip.json.new" "$STATE_DIR/update-skip.json"
echo "rare: recorded $(jq -r .version <<<"$d") as a version that did not start"
