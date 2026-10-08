#!/usr/bin/env bash
# Assemble target/baud.app: a real macOS application bundle, so baud launches
# from Finder without a Terminal window (the "two windows" bug), takes focus
# like an app, and carries its own Dock icon. The icon is generated from
# assets/logo/baud-icon-dock.svg; the binary is the release build.
#
#   scripts/bundle-macos.sh [--no-build]
#
# Needs iconutil (macOS) and rsvg-convert (via `nix shell nixpkgs#librsvg`).
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT=$(pwd)
VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')

if [ "${1:-}" != "--no-build" ]; then
  cargo build --release -p baud-app
fi
BIN="$ROOT/target/release/baud"
[ -x "$BIN" ] || {
  echo "no release binary at $BIN (run without --no-build)"
  exit 1
}

APP="$ROOT/target/baud.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

# Info.plist with the version substituted in.
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist >"$APP/Contents/Info.plist"

# The icon: render the padded dock squircle to every iconset size, then pack.
# The sizes come in pairs (Nx and its @2x) per Apple's iconset layout.
ICONSET=$(mktemp -d)/baud.iconset
mkdir -p "$ICONSET"
SVG="$ROOT/assets/logo/baud-icon-dock.svg"
# shellcheck disable=SC2016  # $1/$2 expand in the inner `sh -c`, not out here
nix shell nixpkgs#librsvg --command sh -c '
  svg="$1"; out="$2"; shift 2
  for pair in 16:icon_16x16 32:icon_16x16@2x 32:icon_32x32 64:icon_32x32@2x \
              128:icon_128x128 256:icon_128x128@2x 256:icon_256x256 \
              512:icon_256x256@2x 512:icon_512x512 1024:icon_512x512@2x; do
    rsvg-convert -w "${pair%%:*}" -h "${pair%%:*}" "$svg" -o "$out/${pair#*:}.png"
  done
' sh "$SVG" "$ICONSET"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/baud.icns"

# The executable.
cp "$BIN" "$APP/Contents/MacOS/baud"

# The embedded fonts' licences travel with the binary that carries them, as
# the upstream archives ship them: JetBrains Mono's OFL and AUTHORS, and the
# Nerd Fonts icon face's MIT licence and its icon-set licence table, and STIX
# Two Math's OFL. The built Regular and Bold -- JetBrains Mono with baud's
# Hebrew -- carry their own OFL, FONTLOG, AUTHORS and MANIFEST, and the
# Hebrew's source, Cascadia Mono, its OFL and SOURCE.
for DIR in jetbrains-mono symbols-nerd-font stix-two-math built glyphs/languages/hebrew; do
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
cp "$ROOT/baud-app/fonts/jetbrains-mono/OFL.txt" \
  "$ROOT/baud-app/fonts/jetbrains-mono/AUTHORS.txt" \
  "$ROOT/baud-app/fonts/jetbrains-mono/SOURCE" \
  "$APP/Contents/Resources/fonts/jetbrains-mono/"
cp "$ROOT/baud-app/fonts/symbols-nerd-font/LICENSE" \
  "$ROOT/baud-app/fonts/symbols-nerd-font/README.md" \
  "$ROOT/baud-app/fonts/symbols-nerd-font/SOURCE" \
  "$APP/Contents/Resources/fonts/symbols-nerd-font/"
cp "$ROOT/baud-app/fonts/stix-two-math/OFL.txt" \
  "$ROOT/baud-app/fonts/stix-two-math/SOURCE" \
  "$APP/Contents/Resources/fonts/stix-two-math/"

# Ad-hoc sign, so the bundle has a stable identity for TCC/accessibility and
# runs without a quarantine prompt after a local build.
codesign --force --sign - "$APP" >/dev/null 2>&1 || echo "note: codesign failed (bundle still runs)"

echo "built $APP  (baud $VERSION)"
