#!/usr/bin/env bash
# Assemble target/baud.app: a real macOS application bundle, so baud launches
# from Finder without a Terminal window (the "two windows" bug), takes focus
# like an app, and carries its own Dock icon. The icon is packed from
# assets/logo/baud-icon-dock-<N>.png, baud-icon-dock.svg drawn at each
# iconset size by tools/baud-pictures and packed by iconutil into
# packaging/macos/baud.icns, which is committed; the binary is the release
# build.
#
#   scripts/bundle-macos.sh [--no-build | --info-plist | --icns OUT |
#                            --assemble BIN APP]
#
# --info-plist prints the Info.plist it would write and stops, before any
# build; baud-app/tests/version.rs reads the version keys from it. --icns
# packs the PNGs into OUT with iconutil (macOS) and stops: run with
# packaging/macos/baud.icns after the PNGs change, and baud-app/tests/
# install.rs fails while the two differ. --assemble writes the bundle for
# the binary BIN at APP and stops, with no build and no iconutil, which is
# how the Nix package makes its bundle. The release build and the bundle go
# under $CARGO_TARGET_DIR when it is set, else target/.
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
# The version scheme is X.Y.Z-beta, then X.Y.Z-beta.N, then X.Y.Z.
# CFBundleShortVersionString is "three period-separated integers ... only
# numeric characters (0-9) and periods" (Apple,
# [[source: apple-cfbundleshortversionstring]]), so it takes the version less
# its pre-release part; any other shape stops the bundle here rather than
# writing a plist Apple refuses.
if [[ $VERSION =~ ^([0-9]+\.[0-9]+\.[0-9]+)(-beta(\.(0|[1-9][0-9]*))?)?$ ]]; then
  SHORT_VERSION=${BASH_REMATCH[1]}
else
  echo "version $VERSION is not X.Y.Z, X.Y.Z-beta or X.Y.Z-beta.N" >&2
  exit 1
fi
# CFBundleVersion is the counter committed beside the plist, raised by hand
# for each release, so a build with no .git -- nix, a source tarball --
# writes the same number. Apple allows "one to three period-separated
# integers" ([[source: apple-cfbundleversion]]); anything else stops the
# bundle here.
BUILD_NUMBER=$(cat packaging/macos/build-number)
if ! [[ $BUILD_NUMBER =~ ^(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*)){0,2}$ ]]; then
  echo "packaging/macos/build-number \"$BUILD_NUMBER\" is not one to three integers" >&2
  exit 1
fi
plist() {
  sed -e "s/__SHORT_VERSION__/$SHORT_VERSION/g" -e "s/__BUILD_NUMBER__/$BUILD_NUMBER/g" \
    packaging/macos/Info.plist
}
if [ "${1:-}" = "--info-plist" ]; then
  plist
  exit 0
fi

# The icon: the padded dock squircle at every iconset size, each drawn by
# the brand generator's pinned resvg and committed, then packed. The sizes
# come in pairs (Nx and its @2x) per Apple's iconset layout. iconutil packs
# the same PNGs into the same bytes each time, so the result is committed
# and the bundle copies it: a build with no iconutil -- Nix's -- carries
# the same icon.
if [ "${1:-}" = "--icns" ]; then
  OUT=${2:?--icns OUT}
  ICONSET=$(mktemp -d)/baud.iconset
  mkdir -p "$ICONSET"
  for pair in 16:icon_16x16 32:icon_16x16@2x 32:icon_32x32 64:icon_32x32@2x \
    128:icon_128x128 256:icon_128x128@2x 256:icon_256x256 \
    512:icon_256x256@2x 512:icon_512x512 1024:icon_512x512@2x; do
    cp "$ROOT/assets/logo/baud-icon-dock-${pair%%:*}.png" "$ICONSET/${pair#*:}.png"
  done
  iconutil -c icns "$ICONSET" -o "$OUT"
  rm -rf "$(dirname "$ICONSET")"
  exit 0
fi

if [ "${1:-}" = "--assemble" ]; then
  BIN=${2:?--assemble BIN APP}
  APP=${3:?--assemble BIN APP}
else
  TARGET=${CARGO_TARGET_DIR:-$ROOT/target}
  if [ "${1:-}" != "--no-build" ]; then
    cargo build --release -p baud
  fi
  BIN="$TARGET/release/baud"
  APP="$TARGET/baud.app"
fi
[ -x "$BIN" ] || {
  echo "no release binary at $BIN (run without --no-build)"
  exit 1
}

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# Info.plist with the version substituted in.
plist >"$APP/Contents/Info.plist"

cp "$ROOT/packaging/macos/baud.icns" "$APP/Contents/Resources/baud.icns"
# The About window's film, the README's logo, and the author's picture;
# `baud-app/src/about.rs` reads them from here, and a binary with no bundle
# shows the lockup still and no picture.
cp "$ROOT/assets/readme/logo-dark.gif" "$ROOT/assets/readme/logo-light.gif" \
  "$ROOT/assets/readme/author.jpg" "$APP/Contents/Resources/"

# The executable.
cp "$BIN" "$APP/Contents/MacOS/baud"

# The embedded fonts' licences travel with the binary that carries them, as
# the upstream archives ship them: JetBrains Mono's OFL and AUTHORS, and the
# Nerd Fonts icon face's MIT licence, its icon-set licence table and the
# icon sets' own licence texts (icon-licences/), and STIX Two Math's OFL.
# The four built faces -- JetBrains Mono with baud's Hebrew and Noto Sans
# Devanagari -- carry their own OFL, FONTLOG, AUTHORS and MANIFEST, the
# Hebrew's source, Cascadia Mono, its OFL and SOURCE, and the
# Devanagari's SOURCE (its licence is the built OFL, under Noto's line).
for DIR in jetbrains-mono symbols-nerd-font stix-two-math built glyphs/languages/hebrew glyphs/languages/devanagari; do
  mkdir -p "$APP/Contents/Resources/fonts/$DIR"
done
cp "$ROOT/baud-app/fonts/built/OFL.txt" \
  "$ROOT/baud-app/fonts/built/FONTLOG.txt" \
  "$ROOT/baud-app/fonts/built/AUTHORS.txt" \
  "$ROOT/baud-app/fonts/built/MANIFEST" \
  "$APP/Contents/Resources/fonts/built/"
cp "$ROOT/baud-app/fonts/glyphs/languages/hebrew/OFL.txt" \
  "$ROOT/baud-app/fonts/glyphs/languages/hebrew/SOURCE" \
  "$APP/Contents/Resources/fonts/glyphs/languages/hebrew/"
cp "$ROOT/baud-app/fonts/glyphs/languages/devanagari/SOURCE" \
  "$APP/Contents/Resources/fonts/glyphs/languages/devanagari/"
cp "$ROOT/baud-app/fonts/jetbrains-mono/OFL.txt" \
  "$ROOT/baud-app/fonts/jetbrains-mono/AUTHORS.txt" \
  "$ROOT/baud-app/fonts/jetbrains-mono/SOURCE" \
  "$APP/Contents/Resources/fonts/jetbrains-mono/"
cp "$ROOT/baud-app/fonts/symbols-nerd-font/LICENSE" \
  "$ROOT/baud-app/fonts/symbols-nerd-font/README.md" \
  "$ROOT/baud-app/fonts/symbols-nerd-font/SOURCE" \
  "$APP/Contents/Resources/fonts/symbols-nerd-font/"
cp -R "$ROOT/baud-app/fonts/symbols-nerd-font/icon-licences" \
  "$APP/Contents/Resources/fonts/symbols-nerd-font/"
cp "$ROOT/baud-app/fonts/stix-two-math/OFL.txt" \
  "$ROOT/baud-app/fonts/stix-two-math/SOURCE" \
  "$APP/Contents/Resources/fonts/stix-two-math/"

# Ad-hoc sign, so the bundle has a stable identity for TCC/accessibility and
# runs without a quarantine prompt after a local build. --assemble leaves
# the signing to its caller: the Nix package signs with a signer nixpkgs
# builds, as no Xcode tool is reachable from a Nix build (flake.nix).
if [ "${1:-}" != "--assemble" ]; then
  codesign --force --sign - "$APP" >/dev/null 2>&1 || echo "note: codesign failed (bundle still runs)"
fi

echo "built $APP  (baud $VERSION)"
