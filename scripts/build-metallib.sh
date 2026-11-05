#!/bin/sh
# Compile baud's Metal shaders to a committed `.metallib`, and record the
# digest of the source it was built from.
#
# A cold Metal shader cache costs +287 ms to the first present, of which the source-to-AIR
# front end is 277. A `.metallib` holds AIR, so shipping one skips that
# front end on every machine; the per-GPU pipeline build still happens,
# because AIR is portable and GPU binaries are not -- and measured, it is
# 73.6 ms cold from this library against 45.1 from a source-built one,
# which is unexplained and is why the net is 253 rather than 277.
#
# WHY THE ARTEFACT IS COMMITTED
#     The machine that builds baud is usually not the machine that has
#     the Metal toolchain -- it is a separate Xcode component, absent by
#     default, and absent entirely on Linux and on CI. A generated file
#     in the tree is how this repo already ships every table it cannot
#     rebuild everywhere, and `the_library_was_built_from_the_shaders_beside_it`
#     in `baud-present-macos/tests/metallib.rs` is the gate that stops it
#     drifting from the shaders.
#
# WHY NOT `xcrun`
#     `xcode-select` on this box points at a nix `apple-sdk-14.4`, so
#     `xcrun metal` answers "tool 'metal' not found"; and Xcode's own
#     `metal` is a stub that answers "missing Metal Toolchain" even when
#     the toolchain is installed, because the real compiler lives in a
#     cryptex whose mount path carries a random suffix. The one stable
#     route is to ask `xcodebuild` where the component is and take the
#     compiler from there, which is what this does.
#
# Usage: scripts/build-metallib.sh [xcode-developer-dir]

set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
src="$here/baud-present-macos/src/shaders.metal"
out="$here/baud-present-macos/src/baud.metallib"
digest="$here/baud-present-macos/src/shaders.metal.fnv"
libdigest="$here/baud-present-macos/src/baud.metallib.fnv"

dev=${1:-}
if [ -z "$dev" ]; then
  for c in /Applications/Xcode.app/Contents/Developer \
    /Applications/Xcode-beta.app/Contents/Developer; do
    [ -x "$c/usr/bin/xcodebuild" ] && dev=$c && break
  done
fi
if [ -z "$dev" ] || [ ! -x "$dev/usr/bin/xcodebuild" ]; then
  echo "no Xcode with xcodebuild found; pass its Developer dir as the argument" >&2
  echo "(xcode-select points at $(xcode-select -p 2>/dev/null), which is why" >&2
  echo " xcrun cannot help here)" >&2
  exit 2
fi

info=$(DEVELOPER_DIR="$dev" "$dev/usr/bin/xcodebuild" -showComponent MetalToolchain 2>/dev/null || true)
status=$(printf '%s\n' "$info" | sed -n 's/^Status: //p')
search=$(printf '%s\n' "$info" | sed -n 's/^Toolchain Search Path: //p')
if [ "$status" != "installed" ] || [ -z "$search" ]; then
  echo "the Metal toolchain is not installed: xcodebuild -downloadComponent MetalToolchain" >&2
  echo "(status: ${status:-unknown})" >&2
  exit 3
fi

metal="$search/Metal.xctoolchain/usr/bin/metal"
[ -x "$metal" ] || {
  echo "no compiler at $metal" >&2
  exit 4
}

# The positive control: a compiler that cannot answer its own version is
# a stub, and every failure mode here has produced one.
"$metal" --version >/dev/null 2>&1 || {
  echo "the compiler at $metal will not run:" >&2
  "$metal" --version >&2 || true
  exit 5
}

# **The library's floor is the app's: macOS 26**. Without the flag the
# compiler takes its target from the environment --
# `MACOSX_DEPLOYMENT_TARGET`, 14.0 under devenv, else the SDK's version,
# 14.4 from the nix SDKROOT -- so the same source built two libraries.
# `baud-app/tests/macos_floor.rs` holds this release to Info.plist's.
"$metal" -mmacosx-version-min=26.0 -o "$out" "$src"
[ -s "$out" ] || {
  echo "the compiler wrote nothing to $out" >&2
  exit 6
}

# Both digests, written after the compile: the source's says the shaders
# have not moved since, and the library's says *this* library is the one
# this run produced from them. The source digest alone cannot say that --
# it relates the shaders to themselves -- and a library swapped for one
# built from different text with the same entry points would pass every
# other gate here.
(cd "$here" && cargo xtask fnv1a "$src" "$out" "$digest" "$libdigest")

echo "$(basename "$out"): $(wc -c <"$out" | tr -d ' ') bytes, from $("$metal" --version 2>&1 | head -1)"
