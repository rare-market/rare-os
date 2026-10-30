#!/usr/bin/env bash
# The web launcher's screenshots for the states that have a native counterpart (the side-by-side
# report): vite dev:ui with the mocks, scripts/shots.mjs in a headless Chrome for Testing shell
# (never a desktop Chrome or its profile), audio muted.
#   webshots.sh OUT [names]     names: comma separated web shot names (default: every "web" in
#                               launcher/native/tests/golden/states/*.json)
# Needs launcher/node_modules (npm ci). The headless shell (Chrome for Testing, Stable) is fetched
# once into ~/.cache/rare-render/chrome.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
OUT=$(mkdir -p "$1" && cd "$1" && pwd)
NAMES=${2:-$(python3 - "$REPO" <<'PY'
import glob, json, sys
names = []
for f in sorted(glob.glob(sys.argv[1] + "/launcher/native/tests/golden/states/*.json")):
    for s in json.load(open(f)).get("states", []):
        if s.get("web") and s["web"] not in names:
            names.append(s["web"])
print(",".join(names))
PY
)}
[ -n "$NAMES" ] || { echo "no native state names a web shot"; exit 0; }
CACHE=${RR_CHROME_DIR:-$HOME/.cache/rare-render/chrome}
CHROME=${CHROME_HEADLESS_SHELL:-$(ls -d "$CACHE"/chrome-headless-shell/*/*/chrome-headless-shell 2>/dev/null | sort | tail -1 || true)}
if [ -z "$CHROME" ]; then
  # Chrome for Testing's headless shell, Stable, unpacked with Python (no unzip on the runner).
  python3 - "$CACHE" <<'PY'
import io, json, os, platform, sys, urllib.request, zipfile
cache = sys.argv[1]
plat = {"Linux": "linux64", "Darwin": "mac-arm64" if platform.machine() == "arm64" else "mac-x64"}[platform.system()]
idx = json.load(urllib.request.urlopen("https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json"))
st = idx["channels"]["Stable"]
url = next(d["url"] for d in st["downloads"]["chrome-headless-shell"] if d["platform"] == plat)
dest = os.path.join(cache, "chrome-headless-shell", st["version"])
z = zipfile.ZipFile(io.BytesIO(urllib.request.urlopen(url).read()))
for m in z.infolist():
    p = z.extract(m, dest)
    mode = m.external_attr >> 16
    if mode:
        os.chmod(p, mode)
print("chrome-headless-shell", st["version"], "in", dest)
PY
  CHROME=$(ls -d "$CACHE"/chrome-headless-shell/*/*/chrome-headless-shell | sort | tail -1)
fi
case "$CHROME" in *chrome-headless-shell) ;; *) echo "refusing $CHROME: only a chrome-headless-shell"; exit 2 ;; esac
# The runner has no unprivileged user namespaces for Chrome's sandbox; it only opens the local
# vite pages, so it runs without one (shots.mjs passes its own flags, the wrapper adds this one).
if [ -n "${RR_CHROME_NO_SANDBOX:-${CI:-}}" ]; then
  printf '#!/bin/sh\nexec "%s" --no-sandbox "$@"\n' "$CHROME" >"$OUT/chrome-headless-shell"
  chmod +x "$OUT/chrome-headless-shell"
  CHROME=$OUT/chrome-headless-shell
fi
cd "$REPO/launcher"
PORT=5180
npx vite --port $PORT --strictPort --logLevel error >"$OUT/vite.log" 2>&1 &
VITE=$!
trap 'kill $VITE 2>/dev/null || true' EXIT
for _ in $(seq 1 120); do curl -sf "http://localhost:$PORT/" >/dev/null && break; sleep 0.5; done
# Exact names: shots.mjs matches ONLY as prefixes, so the run is filtered again afterwards.
ONLY="$NAMES" SIZES=${SIZES:-1920x1200,1280x800} CHROME="$CHROME" URL="http://localhost:$PORT" node scripts/shots.mjs "$OUT" | tail -n +1
python3 - "$OUT" "$NAMES" <<'PY'
import os, re, sys
keep = set(sys.argv[2].split(","))
for f in os.listdir(sys.argv[1]):
    m = re.match(r"(.+)-\d+x\d+\.png$", f)
    if m and m.group(1) not in keep:
        os.unlink(os.path.join(sys.argv[1], f))
PY
