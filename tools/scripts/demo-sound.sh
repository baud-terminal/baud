#!/usr/bin/env bash
# demo-sound.sh: the tour's two versions, silent and with keyboard clicks.
#
#   tools/scripts/demo-sound.sh [--unpinned] TOUR.mp4 TOUR.timeline.tsv PACK/
#
# TOUR.mp4 and TOUR.timeline.tsv are one take of the demo's `--record
# TOUR.mp4` (crates/baud-app/examples/demo/record.rs, timeline.rs). PACK/ is a
# Mechvibes sound pack's directory, its config.json and its audio file:
# the pack docs/assets/demo/LICENSES.md names under "Keyboard sounds", fetched
# by hand and never committed; another is refused unless `--unpinned`
# says the clicks version's credit does not cover it.
#
# It leaves TOUR.mp4 as it was and writes beside it, each with TOUR.mp4's
# video copied:
#   - TOUR.silent.mp4, an AAC track of silence as long as the video;
#   - TOUR.clicks.mp4, the pack's sound for each `key` and `enter` row
#     at the row's time, over silence, and nothing else.
# Which key's sound a row takes, a key the pack lacks, and why a Command
# chord or a modifier alone makes none, are `demo --sound`'s
# (crates/baud-app/examples/demo/sound.rs), which this runs with the demo's
# ffmpeg: the same timeline and pack make the same bytes.
#
# The ffmpeg is the demo's, from the nixpkgs revision in NIXPKGS below,
# which must be run.rs's (tools/scripts/demo-sound-test.sh holds the two equal).
# DEMO_SOUND_FFMPEG_BIN names another bin directory of that version.
set -euo pipefail

NIXPKGS="github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a"
FFMPEG_VERSION="ffmpeg version 9.0.1"

usage() {
  echo "usage: tools/scripts/demo-sound.sh [--unpinned] TOUR.mp4 TOUR.timeline.tsv PACK/" >&2
  exit 2
}

unpinned=()
while [ "$#" -gt 0 ]; do
  case $1 in
  --unpinned)
    unpinned=(--unpinned)
    shift
    ;;
  --)
    shift
    break
    ;;
  -*) usage ;;
  *) break ;;
  esac
done
[ "$#" -eq 3 ] || usage
tour=$1
timeline=$2
pack=${3%/}
for f in "$tour" "$timeline" "$pack/config.json"; do
  [ -f "$f" ] || {
    echo "demo-sound: no file $f" >&2
    exit 2
  }
done

if [ -n "${DEMO_SOUND_FFMPEG_BIN:-}" ]; then
  bin=$DEMO_SOUND_FFMPEG_BIN
else
  bin="$(nix build --no-link --print-out-paths "$NIXPKGS#ffmpeg-headless.bin")/bin"
fi
"$bin/ffmpeg" -hide_banner -version | head -n 1 | grep -q "^$FFMPEG_VERSION" || {
  echo "demo-sound: $bin/ffmpeg is not $FFMPEG_VERSION, the pinned one" >&2
  exit 2
}

root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cargo run --quiet --release --manifest-path "$root/Cargo.toml" -p baud --example demo -- \
  --sound "$bin" "$tour" "$timeline" "$pack" ${unpinned[@]+"${unpinned[@]}"}
