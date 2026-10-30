#!/usr/bin/env bash
# Host side of the golden run: builds (once per Containerfile hash) the Fedora 44 lavapipe image
# and runs `golden.py ci` in it, with no network, the repo read-only and OUT writable.
#   run.sh OUT [WEB]     OUT holds bin/<program> (golden.py build); WEB the web shots (optional)
#   run.sh --test        the tooling selftest (test_golden.py) in the same image
# Extra golden.py arguments come in GOLDEN_ARGS (e.g. "--only poc-home --res 1920x1200").
#
# Rootless podman with its own store: the rare-build runner's user has no subuid range, so the
# store is vfs with chown errors ignored and the container maps only that user (root inside is
# rarebuild outside, so OUT stays theirs). Nothing here needs root or touches the default store.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
STORE=${RARE_GOLDEN_STORE:-${XDG_DATA_HOME:-$HOME/.local/share}/rare-golden/storage}
RUN=${RARE_GOLDEN_RUNROOT:-${TMPDIR:-/tmp}/rare-golden-run-$(id -u)}
mkdir -p "$STORE" "$RUN"
export XDG_RUNTIME_DIR=$RUN
# The runner's unit has ProtectKernelTunables and ProtectControlGroups: no default sysctls (crun
# would write /proc/sys/net/ipv4/ping_group_range) and no cgroups.
cat >"$RUN/containers.conf" <<'CONF'
[containers]
default_sysctls = []
cgroups = "disabled"
[engine]
cgroup_manager = "cgroupfs"
events_logger = "file"
CONF
export CONTAINERS_CONF_OVERRIDE=$RUN/containers.conf
P=(podman --root "$STORE" --runroot "$RUN/storage" --storage-driver vfs --storage-opt vfs.ignore_chown_errors=true
   --cgroup-manager cgroupfs --events-backend file)
sum=$( (sha256sum "$HERE/Containerfile" 2>/dev/null || shasum -a 256 "$HERE/Containerfile") | cut -c1-12)
TAG=localhost/rare-golden:$sum
if ! "${P[@]}" image exists "$TAG"; then
  echo "building $TAG"
  "${P[@]}" build --pull=newer -t "$TAG" -f "$HERE/Containerfile" "$HERE"
fi
args=(--rm --network=none -v "$REPO:/src:ro")
for v in GITHUB_RUN_ID GITHUB_SHA; do
  [ -n "${!v:-}" ] && args+=(-e "$v=${!v}")
done
if [ "${1:-}" = --test ]; then
  exec "${P[@]}" run "${args[@]}" "$TAG" python3 /src/tools/render/golden/test_golden.py
fi
OUT=$1
WEB=${2:-}
mkdir -p "$OUT"
args+=(-v "$OUT:/out")
web=()
if [ -n "$WEB" ] && [ -d "$WEB" ]; then
  args+=(-v "$WEB:/web:ro")
  web=(--web /web)
fi
# shellcheck disable=SC2086
exec "${P[@]}" run "${args[@]}" "$TAG" python3 /src/tools/render/golden/golden.py ci --out /out "${web[@]}" ${GOLDEN_ARGS:-}
