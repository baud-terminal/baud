#!/usr/bin/env bash
# Where the frame sits in the Linux surface, read off drawn pixels
# (`baud-app/src/origin.rs`):
#
#   scripts/linux-padding.sh <baud-binary> <outdir>
#
# Runs the binary in its own headless sway at scale 1 on a 1605x900 output,
# a private runtime directory and no session bus, so a point is 96 dots to
# the inch, with the font pinned by file to the committed JetBrains Mono
# Regular at 13 points (10 px cells), the activity lights off and the
# exit-status strip off, so the frame is the grid alone. It prints a run
# of 50 full blocks at the grid's first cell, takes the compositor's output
# with grim, and reads where the run starts and how tall a block is.
#
# Twice: `padding = 8`, a gutter of 8 x 96/72 = 10.67, drawn as 10 px, and
# `padding = 0`. The grid is the whole cells that fit inside the gutter,
# and what is left beyond them is split between opposite sides, the odd
# pixel right and bottom: at 8, 1605 less 20 holds 158 cells, 1580 px, and
# the 5 px left put the first cell at 10 + 2 = 12; at 0, 160 cells leave 5
# and the first cell is at 2. The rows the same way, from the block's
# measured height. A frame drawn from the surface's corner starts at 0 on
# both axes and fails.
#
# sway and grim come from the nixpkgs revision flake.nix pins, as in
# scripts/linux-points.sh. Nothing is skipped: a compositor that does not
# start, a socket that does not appear, or a screenshot that cannot be read
# fails the script, as a misplaced frame does. The result is written to
# <outdir>/padding.txt: the binary's version line, each arm's measurement
# and the verdict.
set -euo pipefail
bin=$(readlink -f "$1")
out=$(mkdir -p "$2" && cd "$2" && pwd)
repo=$(cd "$(dirname "$0")/.." && pwd)
W=1605
H=900
cells=50
want_cell=10

die() {
  echo "linux-padding: $*" | tee -a "$out/padding.txt" >&2
  exit 1
}
: >"$out/padding.txt"
[ -x "$bin" ] || die "no binary at $1"
"$bin" --version 2>/dev/null | head -1 >>"$out/padding.txt" || die "$1 does not run"

rev=$(sed -n 's|.*nixpkgs.url = "github:NixOS/nixpkgs/\([0-9a-f]*\)".*|\1|p' "$repo/flake.nix")
[ -n "$rev" ] || die "no nixpkgs revision in flake.nix"
sway=$(nix build --no-link --print-out-paths "github:NixOS/nixpkgs/$rev#sway-unwrapped") || die "sway did not build"
grim=$(nix build --no-link --print-out-paths "github:NixOS/nixpkgs/$rev#grim") || die "grim did not build"

font="$repo/baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"
[ -f "$font" ] || die "no font at $font"
cat >"$out/ruler.sh" <<EOF
#!/bin/sh
printf '\\033[2J\\033[H'
i=0
while [ \$i -lt $cells ]; do printf '\\342\\226\\210'; i=\$((i+1)); done
sleep 30
EOF
chmod +x "$out/ruler.sh"

# One arm: the app at `padding = $1`, the screenshot read. Prints
# "<x> <y> <run> <block height>": the run's first bright pixel on a row
# through the blocks, its length, and the block's first bright row and
# height on the column through its first pixel.
arm() {
  local pad=$1 dir="$out/padding-$1"
  mkdir -p "$dir"
  cat >"$dir/baud.toml" <<EOF
fonts = ["$font"]
font_fallback = false
font_size = 13
padding = $pad
[window]
activity_lights = "off"
[screen]
exit_status = "off"
[launch]
splash = "off"
EOF
  cat >"$dir/sway.conf" <<EOF
output HEADLESS-1 resolution ${W}x${H} scale 1
default_border none
gaps inner 0
exec env BAUD_CONFIG=$dir/baud.toml $bin -e $out/ruler.sh
EOF
  # The compositor's runtime directory, short: a socket's path is held to
  # 108 bytes (sun_path), which a deep <outdir> would overrun.
  local rt
  rt=$(mktemp -d /tmp/baud-padding.XXXXXX)
  chmod 700 "$rt"
  env -i HOME="$HOME" PATH="$PATH" XDG_RUNTIME_DIR="$rt" WLR_BACKENDS=headless \
    WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
    "$sway/bin/sway" -c "$dir/sway.conf" >"$dir/sway.log" 2>&1 &
  local sway_pid=$!
  local sock=
  for _ in $(seq 50); do
    sock=$(find "$rt" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' | head -1)
    [ -n "$sock" ] && break
    sleep 0.1
  done
  if [ -z "$sock" ]; then
    kill "$sway_pid" 2>/dev/null || true
    rm -rf -- "$rt"
    die "sway made no socket (see $dir/sway.log)"
  fi
  sleep 3
  local shot=ok
  env -i XDG_RUNTIME_DIR="$rt" WAYLAND_DISPLAY="$(basename "$sock")" \
    "$grim/bin/grim" -t ppm "$dir/shot.ppm" || shot=failed
  kill "$sway_pid" 2>/dev/null || true
  wait "$sway_pid" 2>/dev/null || true
  rm -rf -- "$rt"
  [ "$shot" = ok ] || die "grim took no screenshot (padding $pad)"
  local magic
  magic=$(head -c 2 "$dir/shot.ppm")
  [ "$magic" = "P6" ] || die "not a P6 screenshot (padding $pad)"
  local skip
  skip=$(head -c 64 "$dir/shot.ppm" | awk 'BEGIN{RS="\n"} {n += length($0) + 1} NR == 3 {print n; exit}')
  # Every pixel as 1 (bright: each channel past 128) or 0, one row a line.
  od -An -v -tu1 -j "$skip" -N $((W * H * 3)) "$dir/shot.ppm" |
    tr -s ' \n' '\n' | grep -v '^$' |
    awk -v w="$W" '{v[++n] = $1; if (n == 3) {printf "%d", (v[1] > 128 && v[2] > 128 && v[3] > 128); n = 0; if (++x == w) {printf "\n"; x = 0}}}' \
      >"$dir/bright.txt"
  # The first row with a bright pixel, then that row's run and where it
  # starts; then the column through the run's start, from the top.
  awk -v w="$W" '
    found == 0 && index($0, "1") {top = NR; row = $0; found = 1}
    {col[NR] = $0}
    END {
      if (!found) {print "none"; exit}
      x = index(row, "1"); run = 0
      for (i = x; i <= w && substr(row, i, 1) == "1"; i++) run++
      h = 0
      for (r = top; r in col && substr(col[r], x, 1) == "1"; r++) h++
      print x - 1, top - 1, run, h
    }' "$dir/bright.txt"
}

check() {
  local pad=$1 gutter=$2
  local got
  got=$(arm "$pad")
  [ "$got" != none ] || die "padding $pad: nothing bright on the screen"
  read -r x y run h <<<"$got"
  local cell=$((run / cells))
  [ "$cell" -eq "$want_cell" ] || die "padding $pad: $cells cells drew $run px, not $((want_cell * cells))"
  local room_w=$((W - 2 * gutter)) room_h=$((H - 2 * gutter))
  local want_x=$((gutter + (room_w - room_w / cell * cell) / 2))
  local want_y=$((gutter + (room_h - room_h / h * h) / 2))
  echo "padding $pad: first cell at ($x, $y), cell ${cell}x$h, want ($want_x, $want_y)" >>"$out/padding.txt"
  [ "$x" -eq "$want_x" ] && [ "$y" -eq "$want_y" ] ||
    die "padding $pad: the first cell is at ($x, $y), not ($want_x, $want_y)"
}

check 8 10
check 0 0
echo "ok" >>"$out/padding.txt"
cat "$out/padding.txt"
