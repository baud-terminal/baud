#!/usr/bin/env bash
# The frame kept for a screen switch's slide, below a top lights line, read
# off drawn pixels (`baud-render`'s `render_hidden`, `crates/baud-app/src/linux.rs`):
#
#   tools/scripts/linux-kept-frame.sh <baud-binary> <outdir>
#
# The binary must be built with the test-only `slide-hold` feature: with
# `BAUD_SLIDE_HOLD=1` the slide stays at its start, where the frame drawn is
# the one kept at the switch, with the cursor taken off it. The run is in a
# headless sway at scale 1, 1600x900, with the font pinned by file to the
# committed JetBrains Mono Regular at 13 points and `padding = 0`, the
# activity lights a line at the top, the slide on and the cursor a steady
# block. The child paints two red bands, A on screen row 2 and B on row 9,
# puts the cursor on blank row 3, right under A, and waits on a FIFO; once
# the screenshot shows both bands and the cursor, the FIFO is written, the
# child brings up the alternate screen, and the held slide is captured.
#
# Every figure is measured in the shot itself, so the veil's tint, the theme
# and the cell's height do not enter. h is band B's height in column 200:
#
#   P1  both bands are in column 200, so the shot is the kept frame and not
#       the alternate screen ("not the kept frame" otherwise: a binary
#       without the hook lands the slide);
#   P2  B's top minus A's top is 7 x h;
#   A   A's height is h: row 3 repainted where it lies leaves row 2 whole;
#   B   on every pixel line of row 3, from A's top + h to A's top + 2h, the
#       pixel at the grid's left edge (the first red pixel on a line of B)
#       equals the one at column 20: no line of row 3 still shows the cursor.
#
# sway and grim come from the nixpkgs revision flake.nix pins, built here;
# sway is nixpkgs' unwrapped one, which starts no session bus. Nothing is
# skipped: a compositor that does not start, a socket that does not appear,
# a screenshot that cannot be read, a wait that runs out or a wrong figure
# fails the script. <outdir>/kept.txt holds the binary's version line and
# sha256, h, |A|, the lines of row 3 still showing the cursor and the exit.
set -euo pipefail
bin=$(readlink -f "$1")
out=$(mkdir -p "$2" && cd "$2" && pwd)
repo=$(cd "$(dirname "$0")/../.." && pwd)
# The polls: a screenshot every 200 ms, for at most 30 s a wait.
poll=0.2
polls=150
# Band B is 7 rows below band A; each band is 40 cells wide; the cursor's
# row is the one under A; column 20 is blank on that row.
rows_apart=7
band_cells=40
probe_col=20
probe_x=200

kept="$out/kept.txt"
fail() {
  echo "linux-kept-frame: $*" >&2
  echo "fail: $*" >>"$kept"
  echo "exit 1" >>"$kept"
  exit 1
}
: >"$kept"
[ -x "$bin" ] || fail "no binary at $1"
"$bin" --version 2>/dev/null | head -1 >>"$kept" || fail "$1 does not run"
echo "binary sha256 $(sha256sum "$bin" | cut -d' ' -f1)" >>"$kept"

rev=$(sed -n 's|.*nixpkgs.url = "github:NixOS/nixpkgs/\([0-9a-f]*\)".*|\1|p' "$repo/flake.nix")
[ -n "$rev" ] || fail "no nixpkgs revision in flake.nix"
sway=$(nix build --no-link --print-out-paths "github:NixOS/nixpkgs/$rev#sway-unwrapped") || fail "sway did not build"
grim=$(nix build --no-link --print-out-paths "github:NixOS/nixpkgs/$rev#grim") || fail "grim did not build"
[ -x "$sway/bin/sway" ] || fail "no sway in $sway"
[ -x "$grim/bin/grim" ] || fail "no grim in $grim"

font="$repo/crates/baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"
[ -f "$font" ] || fail "no font at $font"
cat >"$out/baud.toml" <<EOF
fonts = ["$font"]
font_fallback = false
font_size = 13
padding = 0
[window]
activity_lights = "top_line"
activity_lights_glow = 0
[screen]
transition = "on"
exit_status = "off"
[cursor]
blink = false
trail = "off"
unfocused_hollow = "off"
[launch]
splash = "off"
EOF
rm -f "$out/go"
mkfifo "$out/go"
cat >"$out/child.sh" <<EOF
#!/bin/sh
printf '\\033[2J\\033[H\\033[2 q'
printf '\\033[3;1H\\033[48;2;200;0;0m%${band_cells}s\\033[m' ''
printf '\\033[10;1H\\033[48;2;200;0;0m%${band_cells}s\\033[m' ''
printf '\\033[4;1H\\033[?25h'
read _ < "$out/go"
printf '\\033[?1049h'
exec sleep 3600
EOF
chmod +x "$out/child.sh"
cat >"$out/sway.conf" <<EOF
output HEADLESS-1 resolution 1600x900 scale 1
default_border none
gaps inner 0
exec env BAUD_MOTION=full BAUD_SLIDE_HOLD=1 BAUD_CONFIG=$out/baud.toml $bin -e $out/child.sh
EOF

# The compositor's runtime directory, under TMPDIR; a socket's path is held
# to 108 bytes (sun_path).
tmp=${TMPDIR:?TMPDIR names where the runtime directory goes}
rt=$(mktemp -d "$tmp/kf.XXXX")
chmod 700 "$rt"
[ "${#rt}" -lt 90 ] || fail "the runtime directory's path is too long for a socket: $rt"
setsid env -i HOME="$HOME" PATH="$PATH" XDG_RUNTIME_DIR="$rt" WLR_BACKENDS=headless \
  WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
  "$sway/bin/sway" -c "$out/sway.conf" >"$out/sway.log" 2>&1 &
sway_pid=$!
# sway leads a session of its own, so the window and its child go with it;
# a writer the child never read would hold the FIFO open, and goes too.
go_pid=
trap 'kill -- -"$sway_pid" ${go_pid:+"$go_pid"} 2>/dev/null || true; rm -rf -- "$rt"' EXIT
sock=
for _ in $(seq 50); do
  sock=$(find "$rt" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' | head -1)
  [ -n "$sock" ] && break
  sleep 0.1
done
[ -n "$sock" ] || fail "sway made no socket (see $out/sway.log)"
[ "${#sock}" -lt 108 ] || fail "the socket's path is ${#sock} bytes, over sun_path's 108"
kill -0 "$sway_pid" 2>/dev/null || fail "sway ended (see $out/sway.log)"

shoot() {
  env -i XDG_RUNTIME_DIR="$rt" WAYLAND_DISPLAY="$(basename "$sock")" \
    "$grim/bin/grim" -t ppm "$1" 2>>"$out/grim.log"
}

# A shot's pixels, one line of decimal R G B triples per pixel line, in
# <shot>.rows; its width in `width`. grim writes `P6\n<w> <h>\n255\n`.
width=0
rows() {
  local magic header skip
  magic=$(head -c 2 "$1")
  [ "$magic" = "P6" ] || return 1
  header=$(head -c 64 "$1" | tr '\n' ' ')
  read -r _ width _ _ <<<"$header"
  skip=$(head -c 64 "$1" | awk 'BEGIN{RS="\n"} {n += length($0) + 1} NR == 3 {print n; exit}')
  od -An -v -tu1 -w$((width * 3)) -j "$skip" "$1" >"$1.rows"
}

# Red, under the veil or not: red well above green and blue.
RED='function red(r, g, b) { return r >= 80 && r - g >= 50 && r - b >= 50 }'

# The red runs in column x of a shot's rows, top to bottom: "top height".
runs_in_column() {
  awk -v x="$2" "$RED"'
    { i = 3 * x + 1; on = red($i, $(i + 1), $(i + 2)) }
    on && !was { top = NR - 1 }
    !on && was { print top, NR - 1 - top }
    { was = on }
    END { if (was) print top, NR - top }' "$1"
}

# The first and last red x on pixel line y.
red_extent() {
  awk -v y="$2" "$RED"'
    NR == y + 1 {
      first = -1
      for (i = 1; i + 2 <= NF; i += 3)
        if (red($i, $(i + 1), $(i + 2))) { if (first < 0) first = (i - 1) / 3; last = (i - 1) / 3 }
      print first, last
      exit
    }' "$1"
}

# The pixel lines from y0 up to y1 on which the pixel at x0 differs from
# the one at x1.
differing_lines() {
  awk -v x0="$2" -v x1="$3" -v y0="$4" -v y1="$5" '
    NR - 1 >= y0 && NR - 1 < y1 {
      a = 3 * x0 + 1; b = 3 * x1 + 1
      if ($a != $b || $(a + 1) != $(b + 1) || $(a + 2) != $(b + 2)) n++
    }
    END { print n + 0 }' "$1"
}

# The grid's measures in a shot with both bands: the runs A and B, the
# height h, the left edge and the cell's width. Fails the shot with a reason
# in `why`.
measure() {
  why=""
  rows "$1" || {
    why="not a P6 screenshot"
    return 1
  }
  local runs
  runs=$(runs_in_column "$1.rows" "$probe_x")
  if [ "$(printf '%s\n' "$runs" | grep -c .)" -ne 2 ]; then
    why="not the kept frame: column $probe_x holds $(printf '%s\n' "$runs" | grep -c .) red runs, not 2 [$(printf '%s' "$runs" | tr '\n' ';')]"
    return 1
  fi
  read -r a_top a_h <<<"$(printf '%s\n' "$runs" | sed -n 1p)"
  read -r b_top b_h <<<"$(printf '%s\n' "$runs" | sed -n 2p)"
  h=$b_h
  read -r left right <<<"$(red_extent "$1.rows" $((b_top + h / 2)))"
  [ "$left" -ge 0 ] || {
    why="no red on band B's middle line"
    return 1
  }
  cell_w=$(((right - left + 1) / band_cells))
  [ "$cell_w" -gt 0 ] || {
    why="band B is $((right - left + 1)) px wide"
    return 1
  }
  probe=$((left + probe_col * cell_w + cell_w / 2))
  cursor_top=$((a_top + h))
}

# (a) Both bands and the cursor drawn: on row 3 the pixel at the grid's
# left edge differs from the one at column 20.
ready=
for _ in $(seq "$polls"); do
  if shoot "$out/pre.ppm" && measure "$out/pre.ppm" && [ "$a_h" -eq "$h" ] &&
    [ "$(differing_lines "$out/pre.ppm.rows" "$left" "$probe" $((cursor_top + h / 2)) $((cursor_top + h / 2 + 1)))" -eq 1 ]; then
    ready=1
    break
  fi
  kill -0 "$sway_pid" 2>/dev/null || fail "sway ended (see $out/sway.log)"
  sleep "$poll"
done
[ -n "$ready" ] || fail "the bands and the cursor never showed in 30 s${why:+ (last: $why)}"
echo "pre: A $a_top+$a_h B $b_top+$b_h left $left cell $cell_w" >"$out/pre.txt"

# (b) The switch.
printf 'go\n' | timeout 30 tee "$out/go" >/dev/null &
go_pid=$!

# (c) The held slide: a shot that differs from pre.ppm, and the next shot
# the same bytes.
held=
last=
for i in $(seq "$polls"); do
  shot="$out/poll-$((i % 2)).ppm"
  shoot "$shot" || fail "grim took no screenshot (see $out/grim.log)"
  if [ -n "$last" ] && ! cmp -s "$last" "$out/pre.ppm" && cmp -s "$last" "$shot"; then
    cp "$shot" "$out/held.ppm"
    held=1
    break
  fi
  last=$shot
  kill -0 "$sway_pid" 2>/dev/null || fail "sway ended (see $out/sway.log)"
  sleep "$poll"
done
rm -f "$out"/poll-*.ppm
wait "$go_pid" || fail "the child never read the FIFO"
go_pid=
[ -n "$held" ] || fail "no switch held still in 30 s"

# The assertions, on held.ppm.
measure "$out/held.ppm" || fail "$why"
rm -f "$out/pre.ppm.rows"
lit=$(differing_lines "$out/held.ppm.rows" "$left" "$probe" "$cursor_top" $((cursor_top + h)))
rm -f "$out/held.ppm.rows"
{
  echo "h $h"
  echo "A $a_h"
  echo "cursor lines in row 3 $lit"
} >>"$kept"
[ $((b_top - a_top)) -eq $((rows_apart * h)) ] ||
  fail "P2: band B's top is $((b_top - a_top)) px below A's, not $rows_apart x $h"
[ "$a_h" -eq "$h" ] || fail "A: band A is $a_h px tall in the kept frame, not $h"
[ "$lit" -eq 0 ] || fail "B: $lit of row 3's $h lines still show the cursor"
echo "exit 0" >>"$kept"
cat "$kept"
