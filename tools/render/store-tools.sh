#!/usr/bin/env bash
# The Epic and GOG tools the native launcher runs (rare-stores): legendary, gogdl (Heroic's GOG
# backend) and umu-run (Proton outside Steam). All three are Python zipapps that need only the
# system python3; each is a separate GPL/MIT program, never linked into Rare.
#
#   tools/render/store-tools.sh <out-dir> [cache-dir]
#
# Puts legendary, gogdl and umu-run in <out-dir> (made executable) and a VERSION file. Downloads are
# pinned and checked against their sha256; with a cache dir, each version is downloaded once.
# Used by tools/release/build-native.sh (~/rare-native/bin on the device) and the image (/usr/libexec/rare/stores).
set -euo pipefail
out=${1:?usage: store-tools.sh <out-dir> [cache-dir]}
cache=${2:-$(mktemp -d)}

LEGENDARY_VER=0.21.1
LEGENDARY_URL=https://github.com/legendary-gl/legendary/releases/download/$LEGENDARY_VER/legendary_linux_x64
LEGENDARY_SHA=dbed33bbe96031e65858233e4badf0a1d401bb6cb0cc995d1237cf3a0c826a45
GOGDL_VER=1.3.1
GOGDL_URL=https://github.com/Heroic-Games-Launcher/heroic-gogdl/releases/download/v$GOGDL_VER/gogdl_linux_x86_64
GOGDL_SHA=7a6f22da4943bdb4c00baf6250fcee7085c1104ba2ac401479e70ec96e722b59
UMU_VER=1.4.4
UMU_URL=https://github.com/Open-Wine-Components/umu-launcher/releases/download/$UMU_VER/umu-launcher-$UMU_VER-zipapp.tar
UMU_SHA=eb590691841f7fad3fc3ad8fd5db4ccb87849fe7948e62b28ece7a4ee48cc851

fetch() { # url sha file
  local f=$cache/$3
  if [[ ! -f $f ]] || ! echo "$2  $f" | sha256sum -c --status; then
    curl -fsSL --retry 3 -o "$f.part" "$1"
    echo "$2  $f.part" | sha256sum -c --status || { echo "checksum mismatch for $1" >&2; rm -f "$f.part"; exit 1; }
    mv "$f.part" "$f"
  fi
}

mkdir -p "$out" "$cache"
fetch "$LEGENDARY_URL" "$LEGENDARY_SHA" "legendary-$LEGENDARY_VER"
fetch "$GOGDL_URL" "$GOGDL_SHA" "gogdl-$GOGDL_VER"
fetch "$UMU_URL" "$UMU_SHA" "umu-$UMU_VER.tar"
install -m0755 "$cache/legendary-$LEGENDARY_VER" "$out/legendary"
install -m0755 "$cache/gogdl-$GOGDL_VER" "$out/gogdl"
tmp=$(mktemp -d); tar xf "$cache/umu-$UMU_VER.tar" -C "$tmp"
install -m0755 "$tmp/umu/umu-run" "$out/umu-run"; rm -rf "$tmp"
echo "legendary $LEGENDARY_VER, gogdl $GOGDL_VER, umu-run $UMU_VER" >"$out/VERSION"
