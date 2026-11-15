#!/usr/bin/env bash
# What a point is on Linux, read off drawn pixels (`baud-app/src/typesize.rs`):
#
#   scripts/linux-points.sh <baud-binary> <outdir>
#
# Runs the binary in its own headless sway at scale 1 -- a private runtime
# directory and no session bus, so no settings portal answers and the point
# is the untold desktop's, 96 dots to the inch --
# with the font pinned by file to the committed JetBrains Mono Regular at
# 13 points and `padding = 0` -- 900 px hold 39 rows of 23, and the 3 px
# left put the grid 1 px down, so screen row 8 crosses its first row
# (scripts/linux-padding.sh reads where the frame sits) -- prints a run
# of 50 full blocks, takes the compositor's output with grim, and
# measures the run's width in the screenshot's pixels.
#
# 13 points at 96 dpi is 17.33 px, and JetBrains Mono's advance of 600 units
# to the em makes a cell of 10.4, drawn as 10 px: the run is 500 px. A build
# that sizes a point at 72 dpi, as macOS does, draws 13 px and 8 px cells, a
# run of 400, and fails.
#
# sway and grim come from the nixpkgs revision flake.nix pins, built here,
# not from whatever the host has installed; sway is nixpkgs' unwrapped one,
# which starts no session bus and so no portals (the wrapped sway does, and
# its document portal mounts into the runtime directory). Nothing is skipped: a compositor
# that does not start, a socket that does not appear, or a screenshot that
# cannot be read fails the script, as a wrong width does. The result is
# written to <outdir>/points.txt: the binary's version line, the width, the
# cell and the verdict.
set -euo pipefail
bin=$(readlink -f "$1")
out=$(mkdir -p "$2" && cd "$2" && pwd)
repo=$(cd "$(dirname "$0")/.." && pwd)
# The run the desktop's point draws. 13 pt x 96/72 = 17.33 px to the em;
# JetBrains Mono's advance is 600/1000 em, so 17.33 x 0.6 = 10.4 px, a 10 px
# cell; 50 cells are 500 px. A 72-dpi point is 13 px to the em, 13 x 0.6 =
# 7.8 px, an 8 px cell and a 400 px run, which fails.
cells=50
want_cell=10
want_run=$((want_cell * cells))

die() {
  echo "linux-points: $*" | tee -a "$out/points.txt" >&2
  exit 1
}
: >"$out/points.txt"
[ -x "$bin" ] || die "no binary at $1"
"$bin" --version 2>/dev/null | head -1 >>"$out/points.txt" || die "$1 does not run"

rev=$(sed -n 's|.*nixpkgs.url = "github:NixOS/nixpkgs/\([0-9a-f]*\)".*|\1|p' "$repo/flake.nix")
[ -n "$rev" ] || die "no nixpkgs revision in flake.nix"
sway=$(nix build --no-link --print-out-paths "github:NixOS/nixpkgs/$rev#sway-unwrapped") || die "sway did not build"
grim=$(nix build --no-link --print-out-paths "github:NixOS/nixpkgs/$rev#grim") || die "grim did not build"

font="$repo/baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"
[ -f "$font" ] || die "no font at $font"
cat >"$out/baud.toml" <<EOF
fonts = ["$font"]
font_fallback = false
font_size = 13
padding = 0
[window]
activity_lights = "off"
[launch]
splash = "off"
EOF
cat >"$out/ruler.sh" <<EOF
#!/bin/sh
printf '\\033[2J\\033[H'
i=0
while [ \$i -lt $cells ]; do printf '\\342\\226\\210'; i=\$((i+1)); done
sleep 30
EOF
chmod +x "$out/ruler.sh"
cat >"$out/sway.conf" <<EOF
output HEADLESS-1 resolution 1600x900 scale 1
default_border none
gaps inner 0
exec env BAUD_CONFIG=$out/baud.toml $bin -e $out/ruler.sh
EOF

# The compositor's runtime directory, short: a socket's path is held to
# 108 bytes (sun_path), which a deep <outdir> would overrun.
rt=$(mktemp -d /tmp/baud-points.XXXXXX)
chmod 700 "$rt"
env -i HOME="$HOME" PATH="$PATH" XDG_RUNTIME_DIR="$rt" WLR_BACKENDS=headless \
  WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
  "$sway/bin/sway" -c "$out/sway.conf" >"$out/sway.log" 2>&1 &
sway_pid=$!
trap 'kill "$sway_pid" 2>/dev/null || true; rm -rf -- "$rt"' EXIT
sock=
for _ in $(seq 50); do
  sock=$(find "$rt" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' | head -1)
  [ -n "$sock" ] && break
  sleep 0.1
done
[ -n "$sock" ] || die "sway made no socket (see $out/sway.log)"
sleep 3
env -i XDG_RUNTIME_DIR="$rt" WAYLAND_DISPLAY="$(basename "$sock")" \
  "$grim/bin/grim" -t ppm "$out/shot.ppm" || die "grim took no screenshot"

# The run's width: the bright pixels from the left on a row through the
# blocks' middle, in the binary PPM grim wrote (`P6 <w> <h> 255`, then RGB).
magic=$(head -c 2 "$out/shot.ppm")
[ "$magic" = "P6" ] || die "not a P6 screenshot"
header=$(head -c 64 "$out/shot.ppm" | tr '\n' ' ')
read -r _ width _ _ <<<"$header"
skip=$(head -c 64 "$out/shot.ppm" | awk 'BEGIN{RS="\n"} {n += length($0) + 1} NR == 3 {print n; exit}')
row=8
run=$(od -An -v -tu1 -j $((skip + row * width * 3)) -N $((width * 3)) "$out/shot.ppm" |
  tr -s ' \n' '\n' | grep -v '^$' |
  awk '{v[NR] = $1} END {n = 0; for (i = 1; i + 2 <= NR; i += 3) {if (v[i] > 128 && v[i+1] > 128 && v[i+2] > 128) n++; else if (n > 0) break}; print n}')
cell=$((run / cells))
echo "run $run px, cell $cell px, want $want_cell" >>"$out/points.txt"
if [ "$run" -ne "$want_run" ]; then
  die "a point is not the desktop's 96 dpi: $cells cells drew $run px, not $want_run"
fi
echo "ok" >>"$out/points.txt"
cat "$out/points.txt"
