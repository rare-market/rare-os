#!/usr/bin/env bash
# Builds the native Rare Launcher release package on Linux x86_64.
#
#   tools/release/build-native.sh [version]
#
# Output in dist/: rare-native-<version>-x86_64.tar.gz (one rare-native/ folder: the launcher, the web game host
# with its Chromium runtime, the UI assets, the store, Discord and frame generation helpers, and the device
# scripts) and its .sha256. The version defaults to `git describe`.
#
# Paths baked into the binaries (panic locations, debug info, C sources) are remapped to neutral prefixes:
# the checkout becomes /src, the cargo home /cargo, the toolchain /rustup, the target dir /target and $HOME
# /home/build. Build from a neutral location (for example /opt/build/rare-os) and check the result with
# `strings` before publishing it.
#
# Needs: rustup (stable), a C toolchain, pkg-config, libudev, libdbus, libxkbcommon, libssl, curl, tar, git.
# Settings: CARGO_TARGET_DIR (default <checkout>/target-release), RARE_CACHE for downloads
# (default ~/.cache/rare-release), CARGO_BUILD_JOBS.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
version=${1:-$(git describe --tags --always 2>/dev/null || echo dev)}
[[ $version =~ ^[A-Za-z0-9._+-]+$ ]] || { echo "bad version: $version" >&2; exit 2; }
[[ $(uname -sm) == "Linux x86_64" ]] || { echo "build on Linux x86_64" >&2; exit 2; }

export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$root/target-release}
cargo_home=${CARGO_HOME:-$HOME/.cargo}
rustup_home=${RUSTUP_HOME:-$HOME/.rustup}
cache=${RARE_CACHE:-$HOME/.cache/rare-release}
mkdir -p "$CARGO_TARGET_DIR" "$cache"

# rustc applies the last matching remap, so the general prefix ($HOME) comes first.
remap="--remap-path-prefix=$HOME=/home/build --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$rustup_home=/rustup"
remap+=" --remap-path-prefix=$CARGO_TARGET_DIR=/target --remap-path-prefix=$root=/src --remap-path-prefix=$cache=/cache"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="${CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS:-} $remap"
cmap="-ffile-prefix-map=$root=/src -ffile-prefix-map=$cargo_home=/cargo -ffile-prefix-map=$CARGO_TARGET_DIR=/target -ffile-prefix-map=$HOME=/home/build"
export CFLAGS="${CFLAGS:-} $cmap" CXXFLAGS="${CXXFLAGS:-} $cmap"
export CARGO_INCREMENTAL=0
# Settings > About shows the version this package was built as.
export RARE_BUILD=$version
export CEF_PATH=$cache/cef

echo "== cargo build ($version)"
cargo build --release --locked --manifest-path launcher/native/Cargo.toml --bin rare-launcher-native
cargo build --release --locked --manifest-path launcher/native/webhost/Cargo.toml --bin rare-webgame-host

echo "== package"
dist=$root/dist
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
out=$stage/rare-native
mkdir -p "$out" "$dist"
cp "$CARGO_TARGET_DIR/release/rare-launcher-native" "$CARGO_TARGET_DIR/release/rare-webgame-host" "$out/"
strip "$out/rare-launcher-native" "$out/rare-webgame-host"
cp -r launcher/public "$out/public"
cp launcher/native/tools/device/*.sh launcher/native/tools/device/*.py "$out/"
cef=$(ls -d "$CEF_PATH"/*/cef_linux_x86_64 | sort -V | tail -1)
tools/render/cef-runtime.sh "$cef" "$out/cef"
tools/render/store-tools.sh "$out/bin" "$cache/store-tools"
tools/render/arrpc-tools.sh "$out/arrpc" "$cache/arrpc-tools"
tools/render/lsfg-vk.sh "$out/lsfg-vk" "$cache/lsfg-vk"
echo "$version" >"$out/VERSION"

name=rare-native-$version-x86_64.tar.gz
# Reproducible-ish archive: fixed owner, sorted names, no user or group names.
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}" \
  -C "$stage" -czf "$dist/$name" rare-native
(cd "$dist" && sha256sum "$name" >"$name.sha256")
ls -la "$dist/$name"
cat "$dist/$name.sha256"
