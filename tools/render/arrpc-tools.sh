#!/usr/bin/env bash
# arRPC, the open source Discord RPC server (OpenAsar, MIT), for Discord Rich Presence with Discord
# on the web (rare-discord runs it; it is a separate program, never linked into Rare). It needs
# Node.js: the official build is bundled for dev installs; the image uses Fedora's nodejs instead
# (--no-node), which rare-discord finds on PATH.
#
#   tools/render/arrpc-tools.sh [--no-node] <out-dir> [cache-dir]
#
# Puts arrpc/ (arRPC with its one dependency, ws), bin/node (unless --no-node) and VERSION in
# <out-dir>. Downloads are pinned and checked against their sha256; with a cache dir, each version
# is downloaded once. Used by tools/release/build-native.sh (~/rare-native/arrpc on the device) and the image
# (/usr/libexec/rare/arrpc).
set -euo pipefail
node=1
if [[ ${1:-} == --no-node ]]; then node=0; shift; fi
out=${1:?usage: arrpc-tools.sh [--no-node] <out-dir> [cache-dir]}
cache=${2:-$(mktemp -d)}

ARRPC_VER=3.7.0
ARRPC_URL=https://github.com/OpenAsar/arrpc/archive/refs/tags/$ARRPC_VER.tar.gz
ARRPC_SHA=1fe4441800c960cd1d8561805c5cdde52c05875e42721e1dd650379a0ca0e06b
WS_VER=8.18.3
WS_URL=https://registry.npmjs.org/ws/-/ws-$WS_VER.tgz
WS_SHA=424be604c8e7926fc29a1f067bf2dac256af3bcea62fe30395018bbaf8a9be2a
NODE_VER=22.23.4
NODE_URL=https://nodejs.org/dist/v$NODE_VER/node-v$NODE_VER-linux-x64.tar.xz
NODE_SHA=1805bcd35e6815559bd2eb97db438d3fdb0eb3a5e6a977b7c1badca76337e9ce

fetch() { # url sha file
  local f=$cache/$3
  if [[ ! -f $f ]] || ! echo "$2  $f" | sha256sum -c --status; then
    curl -fsSL --retry 3 -o "$f.part" "$1"
    echo "$2  $f.part" | sha256sum -c --status || { echo "checksum mismatch for $1" >&2; rm -f "$f.part"; exit 1; }
    mv "$f.part" "$f"
  fi
}

mkdir -p "$cache"
fetch "$ARRPC_URL" "$ARRPC_SHA" "arrpc-$ARRPC_VER.tar.gz"
fetch "$WS_URL" "$WS_SHA" "ws-$WS_VER.tgz"
rm -rf "$out/arrpc" "$out/bin"
mkdir -p "$out/arrpc/node_modules/ws"
tar xzf "$cache/arrpc-$ARRPC_VER.tar.gz" -C "$out/arrpc" --strip-components=1 "arrpc-$ARRPC_VER/src" "arrpc-$ARRPC_VER/package.json" "arrpc-$ARRPC_VER/LICENSE"
tar xzf "$cache/ws-$WS_VER.tgz" -C "$out/arrpc/node_modules/ws" --strip-components=1
# The bridge listens on every interface upstream; on this device only the page in Rare's window
# reads it, so it answers this device only (what you play stays off the network).
sed -i "s/new WebSocketServer({ port })/new WebSocketServer({ port, host: '127.0.0.1' })/" "$out/arrpc/src/bridge.js"
grep -q "host: '127.0.0.1'" "$out/arrpc/src/bridge.js" || { echo "arrpc: could not bind the bridge to 127.0.0.1" >&2; exit 1; }
version="arrpc $ARRPC_VER, ws $WS_VER"
if (( node )); then
  fetch "$NODE_URL" "$NODE_SHA" "node-v$NODE_VER-linux-x64.tar.xz"
  mkdir -p "$out/bin"
  tar xJf "$cache/node-v$NODE_VER-linux-x64.tar.xz" -C "$out/bin" --strip-components=2 "node-v$NODE_VER-linux-x64/bin/node"
  tar xJf "$cache/node-v$NODE_VER-linux-x64.tar.xz" -C "$out" --strip-components=1 "node-v$NODE_VER-linux-x64/LICENSE"
  mv "$out/LICENSE" "$out/bin/NODE-LICENSE"
  version="$version, node $NODE_VER"
fi
echo "$version" >"$out/VERSION"
