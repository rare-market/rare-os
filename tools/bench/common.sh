# Shared helpers for the Rare OS bench tools. Source, do not run.
# Env: BENCH_HOST (default suiplay), BENCH_OUT (default <repo>/bench/runs)
BENCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HOST="${BENCH_HOST:-suiplay}"
OUTROOT="${BENCH_OUT:-$BENCH_DIR/../../bench/runs}"
RSSH=(ssh -o ConnectTimeout=10 -o ServerAliveInterval=15 "$HOST")
die() { echo "error: $*" >&2; exit 1; }
rrun() { "${RSSH[@]}" "$@"; }
# remote_start <run-id> <sampler args...>: pushes sampler.py to /tmp/rare-bench and detaches it
# (setsid nohup, so it survives ssh drops). Writes nothing outside /tmp/rare-bench.
remote_start() {
  local id="$1"; shift
  rrun "mkdir -p /tmp/rare-bench/$id && cat > /tmp/rare-bench/sampler.py" < "$BENCH_DIR/sampler.py"
  rrun "cd /tmp/rare-bench && setsid nohup python3 sampler.py --out /tmp/rare-bench/$id $* >/tmp/rare-bench/$id/stderr.log 2>&1 < /dev/null & echo started \$!"
}
# remote_wait <run-id> <max-seconds>: polls (reconnecting if ssh drops) until the done file exists.
remote_wait() {
  local id="$1" max="$2" t=0
  while (( t <= max )); do
    if rrun "test -f /tmp/rare-bench/$id/done" 2>/dev/null; then return 0; fi
    sleep 10; t=$((t+10))
  done
  return 1
}
# remote_pull <run-id> <local-dir>
remote_pull() {
  mkdir -p "$2"
  rrun "tar -C /tmp/rare-bench/$1 -cf - ." | tar -C "$2" -xf -
}
slug() { printf '%s' "$1" | tr -c 'A-Za-z0-9._-' '_'; }
