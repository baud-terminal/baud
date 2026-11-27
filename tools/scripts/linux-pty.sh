#!/usr/bin/env bash
# Whole-app throughput through a pty on Linux, six terminals. This tree's
# baud, built here, against the other five as nixpkgs ships them
# (`linux-terminals.sh`), each in a 200x50 window in DejaVu Sans Mono 13 on
# a private headless compositor (`vt-compare-linux.sh`). Four recordings,
# each replayed to about 40 MB a run, 5 rounds with the terminals' order
# turned each round; the reader drops round 1.
#
#   tools/scripts/linux-pty.sh <outdir>
#
# Writes <outdir>/meta.txt (the commit, every binary, the host) and
# <outdir>/results.txt (vt-compare-linux.sh's lines). Timed: run it on a
# machine with nothing else running.
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
out=${1:?usage: tools/scripts/linux-pty.sh <outdir>}
mkdir -p "$out"
out=$(cd "$out" && pwd)
eval "$("$root/tools/scripts/linux-terminals.sh")"
cargo build -q --release --manifest-path "$root/Cargo.toml" -p baud --bin baud
cargo build -q --release --manifest-path "$root/Cargo.toml" -p baud-bench --example vt-reply
target=${CARGO_TARGET_DIR:-$root/target}
export BAUD=$target/release/baud VT_REPLY=$target/release/examples/vt-reply
R=$root/tools/tests/recordings
{
  echo "baud $(git -C "$root" rev-parse HEAD)$(git -C "$root" diff --quiet HEAD || echo ' (dirty)')"
  echo "baud binary $(sha256sum "$BAUD" | cut -c1-16)"
  echo "nixpkgs $NIXPKGS_REV"
  for b in "$SWAY" "$FOOT" "$ALACRITTY" "$KITTY" "$GHOSTTY" "$WEZTERM"; do echo "tool $b"; done
  echo "font $FONT_DIR/DejaVuSansMono.ttf size 13"
  echo "host $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2) $(uname -r)"
  echo "start $(date +%T) load1 $(cut -d' ' -f1 /proc/loadavg)"
} >"$out/meta.txt"
bash "$root/tools/scripts/vt-compare-linux.sh" "$out/results.txt" 5 1 \
  "$R/ascii/cat-scroll.raw:89" "$R/utf8/cat-cjk.raw:122" \
  "$R/ascii/less-pager.raw:1326" "$R/ascii/git-log.raw:79" >"$out/rounds.log" 2>&1
echo "end $(date +%T) load1 $(cut -d' ' -f1 /proc/loadavg)" >>"$out/meta.txt"
echo "$out"
