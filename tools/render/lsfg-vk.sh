#!/usr/bin/env bash
# lsfg-vk, the Vulkan layer that runs Lossless Scaling's frame generation on Linux (lsfg-vk.dev),
# as Rare uses it (launcher/crates/rare-system/src/gamefx.rs): the layer libraries (64- and 32-bit)
# and their manifests in the release archive's layout, plus lsfg-vk-cli. Its own manifests are NOT
# installed where the Vulkan loader looks: Rare writes one per game that has frame generation on.
#
#   tools/render/lsfg-vk.sh <out-dir> [cache-dir]
#
# <out-dir> gets lib/, share/vulkan/implicit_layer.d/, bin/lsfg-vk-cli and VERSION. The archive is
# pinned and checked against its sha256; with a cache dir it is downloaded once. Used by
# tools/release/build-native.sh (~/rare-native/lsfg-vk on the device) and the image (/usr/lib/rare/lsfg-vk).
# lsfg-vk 2 is CC BY-NC-ND 4.0: shipped unmodified. The frame generation itself loads from the
# player's own Lossless Scaling (Steam app 993090, "lsfg-vk" beta branch).
set -euo pipefail
out=${1:?usage: lsfg-vk.sh <out-dir> [cache-dir]}
cache=${2:-$(mktemp -d)}

VER=2.0.0
URL=https://builds.lsfg-vk.dev/lsfg-vk-$VER.tar.xz
SHA=08bdbdf373a111022df87dac7aa87e3b564bb841f961552e3ca85fea12b5aa74

mkdir -p "$out" "$cache"
f=$cache/lsfg-vk-$VER.tar.xz
if [[ ! -f $f ]] || ! echo "$SHA  $f" | sha256sum -c --status; then
  curl -fsSL --retry 3 -o "$f.part" "$URL"
  echo "$SHA  $f.part" | sha256sum -c --status || { echo "checksum mismatch for $URL" >&2; rm -f "$f.part"; exit 1; }
  mv "$f.part" "$f"
fi
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
tar -xf "$f" -C "$tmp"
rm -rf "$out/lib" "$out/share" "$out/bin"
mkdir -p "$out/lib" "$out/share/vulkan/implicit_layer.d" "$out/bin"
install -m0755 "$tmp"/lib/liblsfg-vk-layer.so "$tmp"/lib/liblsfg-vk-layer.x86.so "$out/lib/"
install -m0644 "$tmp"/share/vulkan/implicit_layer.d/*.json "$out/share/vulkan/implicit_layer.d/"
install -m0755 "$tmp"/bin/lsfg-vk-cli "$out/bin/"
echo "lsfg-vk $VER" > "$out/VERSION"
