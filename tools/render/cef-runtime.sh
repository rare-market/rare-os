#!/usr/bin/env bash
# Lays out the CEF runtime rare-webgame-host runs on, from the CEF distribution the `cef` crate
# downloaded (CEF_PATH/<version>/cef_linux_x86_64): libcef.so stripped, the resources, en-US only,
# SwiftShader's Vulkan as the fallback. About 320 MB, once per CEF version.
#
#   tools/render/cef-runtime.sh <cef distribution dir> <out dir>
#
# The out dir gets a VERSION file; the host finds the runtime in `cef/` next to itself (the native
# preview, ~/rare-native/cef) or in /usr/lib/rare/cef (the image).
set -euo pipefail
src=${1:?cef distribution dir}
out=${2:?out dir}
[[ -f $src/libcef.so && -f $src/icudtl.dat ]] || { echo "not a CEF distribution: $src"; exit 1; }
ver=$(sed -n 's/.*"name": *"cef_binary_\([^+]*\).*/\1/p' "$src/archive.json" 2>/dev/null | head -1)
ver=${ver:-$(basename "$(dirname "$src")")}
if [[ -f $out/VERSION && $(cat "$out/VERSION") == "$ver" ]]; then
  echo "cef runtime $ver already in $out"
  exit 0
fi
tmp=$out.tmp.$$
rm -rf "$tmp"; mkdir -p "$tmp/locales"
for f in libcef.so libvk_swiftshader.so libvulkan.so.1 vk_swiftshader_icd.json icudtl.dat v8_context_snapshot.bin resources.pak chrome_100_percent.pak chrome_200_percent.pak; do
  [[ -f $src/$f ]] && cp "$src/$f" "$tmp/"
done
cp "$src/locales/en-US.pak" "$tmp/locales/"
strip --strip-unneeded "$tmp/libcef.so" "$tmp/libvk_swiftshader.so" 2>/dev/null || true
echo "$ver" >"$tmp/VERSION"
rm -rf "$out"; mv "$tmp" "$out"
echo "cef runtime $ver in $out ($(du -sh "$out" | cut -f1))"
