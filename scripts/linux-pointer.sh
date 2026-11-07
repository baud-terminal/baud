#!/usr/bin/env bash
# Moves a pointer over the Linux app on a real compositor and checks what
# it does: the bytes a program reads, the selections a drag leaves on the
# compositor's primary selection and clipboard, the command a link click
# would run, and the pointer images the app asks the compositor for.
#
#   scripts/linux-pointer.sh <outdir>
#
# BAUD_POINTER_TAG names the images and volumes it makes (by default
# baud-linux-pointer), so two runs on one engine keep apart.
#
# Needs docker with an aarch64 Linux engine (colima's default VM on an
# Apple silicon Mac), and the repository under a directory the engine
# mounts. Nothing opens a window here: the compositor is headless, inside
# the container.
#
# The app is built once, in Debian bookworm, and run under two sways on
# wlroots' headless backend, each with no input device of its own:
#
#   bookworm  sway 1.7, wlroots 0.15: no wp_cursor_shape_manager_v1, so
#             the app draws its own pointer and sets it with
#             wl_pointer.set_cursor; wl_seat 7, the wheel in axis_discrete
#   trixie    sway 1.10, wlroots 0.18: cursor-shape-v1, so the app names
#             a shape; wl_seat 9, the wheel in axis_value120
#
# The pointer and the keyboard are baud-bench's linux-pointer example, one
# client holding one zwlr_virtual_pointer_v1 and one zwp_virtual_keyboard_v1, so
# a drag is one device's press, moves and release, and every event carries
# a time from the script's own clock. sway runs with WAYLAND_DEBUG=server,
# which logs each request the app sends it; the pointer requests are
# kept, without their timestamps and object numbers, as the trace of what
# the app asked the compositor to show. grim takes the screen with the
# pointer drawn on it.
#
# The window floats at 640x408 at the origin of a 1024x768 output: 80x23
# cells of 8x17 px, so a cell's centre is (8c + 4, 17r + 8), and a point
# past (640, 408) is off the window. The grid is the window less the
# activity lights' own line, which the Linux app draws at the window's
# bottom with the default config (`config::lights_at`) and which these
# checks keep: the 17 points below row 23 are that line, and the inner
# script's ROWS is the grid's height. Each arm runs again with the output
# at scale 2 and at scale 1.5, its mode 2048x1536 and 1536x1152, so the
# layout is 1024x768 logical pixels at every scale; those runs take the
# pointer to the grid's edges and read what the app draws its pointer and
# its window at. The app draws at the scale the compositor prefers: sway
# 1.10 offers fractional-scale-v1, so at 1.5 it draws at 1.5, and sway 1.7
# does not, and its wl_output says 2, so it draws at 2 and sway scales
# the window down. The font is 13 points, so the cell is the embedded
# face's at 13, 19.5 and 26 pixels, 8x17, 12x26 and 16x34 (the pixels
# bench at PIXELS_SCALE 1, 1.5 and 2): 80x23 cells and the lights' line
# are 640x408 points at 1 and 2 and 960x624 pixels, 640x416 points, at 1.5,
# where the window is 640x416 so the grid is 80x23 on both arms. Every check prints "ok" or
# "FAILED" with what was read; the script exits non-zero when one failed
# or a step could not run.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:?usage: scripts/linux-pointer.sh <outdir>}
mkdir -p "$out"
out=$(cd "$out" && pwd)

toolchain=$(rustc -Vv | sed -n 's/^commit-hash: //p')
nightly=${BAUD_NIGHTLY:-nightly-2026-11-06}
tag=${BAUD_POINTER_TAG:-baud-linux-pointer}
docker build -q -t "$tag-build" - >/dev/null <<EOF
FROM rustlang/rust:nightly-bookworm
RUN rustup toolchain install $nightly --profile minimal \
    && rustup default $nightly
EOF
there=$(docker run --rm "$tag-build" rustc -Vv | sed -n 's/^commit-hash: //p')
if [ "$there" != "$toolchain" ]; then
  echo "the container's rustc is $there, this Mac's $toolchain; set BAUD_NIGHTLY" >&2
  exit 1
fi
for arm in bookworm trixie; do
  docker build -q -t "$tag-$arm" - >/dev/null <<EOF
FROM debian:$arm
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
    --no-install-recommends sway grim wl-clipboard libxkbcommon-tools \
    wayland-utils dmz-cursor-theme \
    && rm -rf /var/lib/apt/lists/* && useradd -m baud
EOF
done
docker volume create "$tag-target" >/dev/null
docker volume create "$tag-registry" >/dev/null
docker run --rm -v "$root":/src:ro \
  -v "$tag-target":/target \
  -v "$tag-registry":/usr/local/cargo/registry \
  "$tag-build" bash -c \
  'set -e
   cd /src && export CARGO_TARGET_DIR=/target RUSTFLAGS="-D warnings"
   cargo build -q --release -p baud --bin baud
   cargo build -q --release -p baud-bench --example linux-pointer \
     --example linux-pointer-trace --example linux-pixels'

# The build above stops at its first failure (`set -e`), and the script
# with it: a build that failed is a step that could not run, not every
# check failed against an app that is not there.

# One arm, as the user baud (sway will not run as root). Results go to
# /tmp/out, which leaves the container as tar on stdout.
inner=$(
  cat <<'INNER'
set -u
mkdir -p /tmp/out && D=$(mktemp -d) && cd "$D"
O=/tmp/out
# The grid's rows: the window's 24 less the activity lights' own line at
# its bottom, the default on Linux (the header).
ROWS=23
export XDG_RUNTIME_DIR=$D/xdg && mkdir -m 0700 "$XDG_RUNTIME_DIR"
W=$(awk "BEGIN { print 1024 * $SCALE }") H=$(awk "BEGIN { print 768 * $SCALE }")
cat > sway.conf <<C
output HEADLESS-1 resolution ${W}x${H} position 0 0 scale $SCALE
default_border none
default_floating_border none
seat seat0 xcursor_theme DMZ-Black 24
for_window [app_id="org.baud.terminal"] floating enable, resize set $WIN, move position 0 0
C
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
  WAYLAND_DEBUG=server sway -c sway.conf > sway.log 2>&1 &
SWAY=$!
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1
export SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock)
{
  sway -v
  wayland-info | grep -E "'(wl_compositor|wl_output|wp_fractional_scale_manager_v1|wp_viewporter|wl_seat|wp_cursor_shape_manager_v1|zwlr_virtual_pointer_manager_v1|zwp_virtual_keyboard_manager_v1|zwp_primary_selection_device_manager_v1)'" |
    sed 's/, name: *[0-9]*//; s/  */ /g'
} > $O/compositor.txt
xkbcli compile-keymap --layout us > keymap.xkb
printf '[launch]\nsplash = "off"\n' > baud.toml
# xdg-open, first on the path: it writes down its arguments and opens
# nothing.
mkdir bin
printf '#!/bin/sh\nprintf "%%s\\n" "$@" >> %s/opened\n' "$D" > bin/xdg-open
printf '#!/bin/sh\nif [ "${2:-}" = plain ]; then grim %s/"$1".png; else grim -c %s/"$1".png; fi\n' "$O" "$O" > bin/shot
chmod +x bin/xdg-open bin/shot
export PATH=$D/bin:$PATH D O
FAILED=0
check() { # name, file, printf format of the bytes wanted
  local got want
  got=$(cat "$2" 2>/dev/null | od -An -c | tr -s ' \n' ' ')
  want=$(printf "$3" | od -An -c | tr -s ' \n' ' ')
  if [ "$got" = "$want" ]; then
    echo "ok      $1: [$(cat "$2" | cat -v)]"
  else
    echo "FAILED  $1: [$(cat "$2" 2>/dev/null | cat -v)], wanted [$(printf "$3" | cat -v)]"
    FAILED=1
  fi
}
# One session: the child script (with $S its folder), then the pointer
# script on stdin. The child waits for the window's grid, 80 columns and
# ROWS rows, before it writes: the pty is spawned at 80x24.
session() {
  S=$D/$1 && mkdir -p "$S" && : > "$S/got"
  printf 'until [ "$(stty size)" = "%s 80" ]; do sleep 0.05; done\nstty raw -echo\n%s\n' "$ROWS" "$2" > "$S/child.sh"
  # Each session starts with the pointer off the window.
  echo "move 900 700" | /target/release/examples/linux-pointer keymap.xkb
  from=$(wc -l < sway.log)
  BAUD_LOG=debug BAUD_CONFIG=baud.toml S=$S \
    BAUD_CMD="bash $S/child.sh" /target/release/baud > "$S/baud.log" 2>&1 &
  BAUD=$!
  for _ in $(seq 100); do [ -e "$S/ready" ] && break; sleep 0.1; done
  [ -e "$S/ready" ] || { echo "FAILED  $1: the child never started"; FAILED=1; }
  sleep 0.5
  S=$S /target/release/examples/linux-pointer keymap.xkb
  sleep 0.3
  kill $BAUD; wait $BAUD 2>/dev/null
  # The app's pointer requests and the pointer events it was sent, in
  # order, without timestamps, serials or object numbers.
  tail -n +$((from + 1)) sway.log | /target/release/examples/linux-pointer-trace > $O/trace-$1.txt
  cp "$S/baud.log" $O/baud-$1.log
}

# The sessions at scale 1; at another scale only the edges, below.
if [ "$SCALE" = 1 ]; then
# A stream drag, Ctrl+Shift+C, an Alt drag for a block, a double and a
# triple click, two clicks too far apart to be a double, and the pointer
# leaving and coming back. Each selection is read back from the
# compositor's primary selection.
session select '
printf "alpha beta gamma\r\none two three four\r\nblock 0123456789\r\nblock abcdefghij\r\nblock ABCDEFGHIJ\r\n"
touch $S/ready; exec cat > $S/got' <<'P'
move 52 8
sleep 200
press left
move 60 25
move 52 25
release left
sleep 300
sh shot stream
sh timeout 3 wl-paste -p -n > $S/stream
mods ctrl+shift
key 46
mods none
sleep 200
sh timeout 3 wl-paste -n > $S/copied
mods alt
move 52 42
press left
move 70 59
move 76 76
release left
mods none
sleep 300
sh shot block
sh timeout 3 wl-paste -p -n > $S/block
move 108 8
tick 1000
click left
click left
sleep 300
sh timeout 3 wl-paste -p -n > $S/double
move 20 25
tick 1000
click left
click left
click left
sleep 300
sh timeout 3 wl-paste -p -n > $S/triple
sh wl-copy -p sentinel 2>/dev/null
sleep 200
move 60 8
tick 1000
click left
tick 300
click left
sleep 300
sh timeout 3 wl-paste -p -n > $S/slow
tick 1000
click left
click left
sleep 300
sh timeout 3 wl-paste -p -n > $S/fast
mods shift
move 20 25
tick 1000
click left
mods none
sleep 300
sh timeout 3 wl-paste -p -n > $S/extended
P
check "a stream drag, the primary selection" $D/select/stream 'beta gamma\none two'
check "Ctrl+Shift+C, the clipboard" $D/select/copied 'beta gamma\none two'
check "an Alt drag, a block" $D/select/block '0123\nabcd\nABCD'
check "a double click, the word" $D/select/double 'gamma'
check "a triple click, the line" $D/select/triple 'one two three four'
check "two clicks 300 ms apart select nothing" $D/select/slow 'sentinel'
check "the same two clicks 10 ms apart, the word" $D/select/fast 'beta'
check "Shift and a click extend it, by words" $D/select/extended 'beta gamma\none'

# The wheel over the history: 100 lines, three detents up, the top row
# read back by a triple click, then three down. The screen is taken
# before and after the wheel with the pointer at the right edge, outside
# the columns compared (sway draws it into the output whether or not
# grim asks): what the window draws three lines back is the live
# screen's rows three rows lower.
session wheel '
for i in $(seq 1 100); do printf "line %d\r\n" $i; done
touch $S/ready; exec cat > $S/got' <<'P'
move 620 200
sleep 200
sh shot wheel-live plain
wheel -3
sleep 300
sh shot wheel-back plain
move 20 8
tick 1000
click left
click left
click left
sleep 300
sh timeout 3 wl-paste -p -n > $S/top
wheel 3
sleep 300
move 20 8
tick 1000
click left
click left
click left
sleep 300
sh timeout 3 wl-paste -p -n > $S/live
move 620 200
swipe -51
sleep 300
move 20 8
tick 1000
click left
click left
click left
sleep 300
sh timeout 3 wl-paste -p -n > $S/swiped
swipe 51
sleep 300
move 620 200
wheel -3
sleep 300
move 634 350
press left
move 634 200
move 634 5
release left
sleep 1600
sh shot thumb-top plain
move 20 8
tick 1000
click left
click left
click left
sleep 300
sh timeout 3 wl-paste -p -n > $S/thumb
move 620 200
wheel 100
sleep 300
wheel -100
sleep 1600
sh shot wheel-top plain
P
# 100 lines and the cursor's blank line: the live screen's top is line
# 102 - ROWS, and three lines back 99 - ROWS.
check "three detents up, the top row" $D/wheel/top "line $((99 - ROWS))"
check "three down again, the top row" $D/wheel/live "line $((102 - ROWS))"
check "a touchpad's 51 px up, three lines of 17, the top row" $D/wheel/swiped "line $((99 - ROWS))"
check "the thumb dragged to the top, the top row" $D/wheel/thumb 'line 1'
check "the wheel on the primary screen sends the program nothing" $D/wheel/got ''
# Rows 3 to ROWS - 1 three lines back against rows 0 to ROWS - 4 live,
# short of the scrollbar's column and above the lights' line: 600 pixels
# wide and ROWS - 3 rows of 17 tall each.
px=/target/release/examples/linux-pixels
h=$(((ROWS - 3) * 17))
$px crop $O/wheel-back.png 0 51 600 $h $D/wheel/back.pam
$px crop $O/wheel-live.png 0 0 600 $h $D/wheel/live.pam
$px diff $D/wheel/back.pam $D/wheel/live.pam | head -1 > $D/wheel/drawn
check "three lines back, the window draws the history" $D/wheel/drawn \
  "0 of $((600 * h)) pixels differ, largest channel difference 0\n"
# The thumb's frame at the top is the wheel's at the top below the first
# row, which the triple click between them leaves selected, and not the
# live screen's rows: the grid's rows 1 to ROWS - 1. Both are taken once
# the copy's words are gone (`copied.rs`, MESSAGE_FOR), which a copy just
# before each puts over row 1 for 1.5 s, with a different count in each.
g=$(((ROWS - 1) * 17))
$px crop $O/thumb-top.png 0 17 600 $g $D/wheel/thumb.pam
$px crop $O/wheel-top.png 0 17 600 $g $D/wheel/top.pam
$px diff $D/wheel/thumb.pam $D/wheel/top.pam | head -1 > $D/wheel/thumb-drawn
check "the thumb at the top draws what the wheel at the top draws" $D/wheel/thumb-drawn \
  "0 of $((600 * g)) pixels differ, largest channel difference 0\n"
$px crop $O/thumb-top.png 0 0 600 $h $D/wheel/thumb-rows.pam
if $px diff $D/wheel/thumb-rows.pam $D/wheel/live.pam > /dev/null; then
  echo "the live screen" > $D/wheel/thumb-live
else
  echo "the history" > $D/wheel/thumb-live
fi
check "the thumb at the top does not draw the live screen" $D/wheel/thumb-live 'the history\n'

# The alternate screen keeps no history: the wheel is the cursor keys,
# ?1007 being set from power-on.
session alt '
printf "\033[?1049h"; touch $S/ready; exec cat > $S/got' <<'P'
move 300 200
wheel -3
wheel 2
sleep 300
P
check "the wheel on the alternate screen, Up three times, Down twice" $D/alt/got \
  '\033[A\033[A\033[A\033[B\033[B'

# Reports under ?1000 and ?1006 from the cell at column 11, row 3: each
# button, the wheel both ways and sideways, Ctrl and Alt, and Shift's
# click, which selects and reports nothing; a move with no button
# reports nothing either.
session sgr '
printf "\033[?1000h\033[?1006h"; touch $S/ready; exec cat > $S/got' <<'P'
move 84 42
click left
click middle
click right
wheel -1
wheel 1
across -1
across 1
mods ctrl
click left
mods alt
click left
mods shift
click left
mods none
move 92 42
move 100 59
sleep 300
P
check "?1000 ?1006: buttons, wheel, Ctrl, Alt" $D/sgr/got \
  '\033[<0;11;3M\033[<0;11;3m\033[<1;11;3M\033[<1;11;3m\033[<2;11;3M\033[<2;11;3m\033[<64;11;3M\033[<65;11;3M\033[<66;11;3M\033[<67;11;3M\033[<16;11;3M\033[<16;11;3m\033[<8;11;3M\033[<8;11;3m'

# ?1002: motion only with a button held, once a cell; ?1003: every move.
session drag '
printf "\033[?1002h\033[?1006h"; touch $S/ready; exec cat > $S/got' <<'P'
move 84 42
move 92 42
press left
move 94 42
move 100 42
move 108 59
release left
move 116 59
sleep 300
P
check "?1002: a drag, once a cell" $D/drag/got \
  '\033[<0;12;3M\033[<32;13;3M\033[<32;14;4M\033[<0;14;4m'
session any '
printf "\033[?1003h\033[?1006h"; touch $S/ready; exec cat > $S/got' <<'P'
move 84 42
move 86 42
move 92 42
move 92 59
sleep 300
P
check "?1003: every move into another cell" $D/any/got \
  '\033[<35;11;3M\033[<35;12;3M\033[<35;12;4M'
# The original encoding: ?1000 alone, each value plus 32 in a byte.
session x10 '
printf "\033[?1000h"; touch $S/ready; exec cat > $S/got' <<'P'
move 84 42
click left
sleep 300
P
check "?1000 alone: X10's bytes" $D/x10/got '\033[M +#\033[M#+#'

# The pointer's shape: the I-beam where no program asked; a key hides it;
# OSC 22 names a hand, an arrow and a watch, each shown at once; an empty
# OSC 22 is the I-beam again; the pointer leaves and comes back with its
# shape; the seat loses its pointer and gets a new one.
session shape '
printf "baud\r\n"; touch $S/ready
head -c 1 > /dev/null; printf "\033]22;hand2\033\\\\"; touch $S/1
head -c 1 > /dev/null; printf "\033]22;left_ptr\033\\\\"; touch $S/2
head -c 1 > /dev/null; printf "\033]22;watch\033\\\\"; touch $S/3
head -c 1 > /dev/null; printf "\033]22;\033\\\\"; touch $S/4
exec cat > $S/got' <<'P'
move 300 200
sleep 300
sh shot shape-text
key 30
waitfor [ -e $S/1 ]
sleep 300
sh shot shape-hand2
key 30
waitfor [ -e $S/2 ]
sleep 300
key 30
waitfor [ -e $S/3 ]
sleep 300
sh shot shape-watch
move 310 200
key 30
waitfor [ -e $S/4 ]
sleep 300
move 900 700
sleep 200
move 300 200
sleep 200
again
move 320 210
sleep 300
P
# What the app asked the compositor to show, a request a line and repeats
# folded: on sway 1.7 its own three images, on sway 1.10 the shapes by name.
grep -E '^(enter|leave|set_|capabilities)' $O/trace-shape.txt | sed 's/ [0-9]* [0-9]*$//' |
  uniq | tr '\n' ';' > $D/shape/seq
if grep -q wp_cursor_shape_manager_v1 $O/compositor.txt; then
  check "the shapes, named: I-beam, hidden, hand, hidden, arrow, hidden, watch, hidden, I-beam" $D/shape/seq \
    'enter;set_shape text;set_cursor hidden;set_shape pointer;set_cursor hidden;set_shape default;set_cursor hidden;set_shape wait;set_cursor hidden;set_shape text;leave;enter;set_shape text;leave;capabilities keyboard;capabilities none;capabilities pointer;capabilities pointer keyboard;enter;set_shape text;leave;capabilities keyboard;capabilities none;'
else
  check "the shapes, drawn: the I-beam, the hand for hand2, the arrow for arrow and watch" $D/shape/seq \
    'enter;set_cursor the I-beam;set_cursor hidden;set_cursor the hand;set_cursor hidden;set_cursor the arrow;set_cursor hidden;set_cursor the arrow;set_cursor hidden;set_cursor the I-beam;leave;enter;set_cursor the I-beam;leave;capabilities keyboard;capabilities none;capabilities pointer;capabilities pointer keyboard;enter;set_cursor the I-beam;leave;capabilities keyboard;capabilities none;'
fi

# The middle button pastes the primary selection; Shift+Insert too; with
# ?2004 the paste is bracketed.
session paste '
printf "\033[?2004h"; touch $S/ready; exec cat > $S/got' <<'P'
sh wl-copy -p "from the primary" 2>/dev/null
sleep 300
move 84 42
click middle
waitfor grep -q primary $S/got
mods shift
key 110
mods none
sleep 300
P
check "the middle button and Shift+Insert paste the primary" $D/paste/got \
  '\033[200~from the primary\033[201~\033[200~from the primary\033[201~'

# Links: Ctrl held over an OSC 8 link previews it and shows the pointing
# hand, a Ctrl click opens it with xdg-open, and a Ctrl click on a URI in
# the text opens that; over a link that will not open, an ftp one, the
# preview says why and the arrow takes the hand's place, since nothing
# about it may look openable (`link::Hover::pointer`); the pointer leaving takes the
# preview away. Under ?1000 ?1006 the Ctrl click on a link is still the
# link's, with the hand, and elsewhere the program's.
session link '
printf "see \033]8;;https://example.org/p14\033\\\\this link\033]8;;\033\\\\ or https://example.com/plain\r\n"
printf "\033]8;;ftp://example.org/refused\033\\\\refused\033]8;;\033\\\\\r\n"
touch $S/ready; head -c 1 > /dev/null; printf "\033[?1000h\033[?1006h"; touch $S/tracking
exec cat > $S/got' <<'P'
move 52 8
mods ctrl
sleep 300
sh shot link-preview
click left
waitfor [ -s $D/opened ]
move 196 8
tick 1000
click left
waitfor [ $(wc -l < $D/opened) = 2 ]
move 20 25
sleep 300
sh shot link-refused
move 900 700
sleep 300
sh shot link-left
move 300 200
mods none
key 30
waitfor [ -e $S/tracking ]
move 52 8
mods ctrl
tick 1000
click left
waitfor [ $(wc -l < $D/opened) = 3 ]
move 300 59
tick 1000
click left
mods none
sleep 300
P
check "Ctrl clicks run xdg-open with each link" $D/opened \
  'https://example.org/p14\nhttps://example.com/plain\nhttps://example.org/p14\n'
check "under tracking a Ctrl click on a link is the link's, elsewhere the program's" $D/link/got \
  '\033[<16;38;4M\033[<16;38;4m'
# The hand while Ctrl is held over a link that opens, under tracking too;
# the arrow over the refused link (`link::Hover::pointer`); and the I-beam
# again off the link: on sway 1.10 the shapes by name, on sway 1.7 the
# app's images.
grep -E '^(enter|leave|set_)' $O/trace-link.txt | sed 's/ [0-9]* [0-9]*$//' |
  uniq | tr '\n' ';' > $D/link/seq
if grep -q wp_cursor_shape_manager_v1 $O/compositor.txt; then
  check "Ctrl over a link that opens: the pointer shape, named" $D/link/seq \
    'enter;set_shape text;set_shape pointer;set_shape default;leave;enter;set_shape text;set_cursor hidden;set_shape text;set_shape pointer;set_shape text;leave;'
else
  check "Ctrl over a link that opens: the hand, drawn" $D/link/seq \
    'enter;set_cursor the I-beam;set_cursor the hand;set_cursor the arrow;leave;enter;set_cursor the I-beam;set_cursor hidden;set_cursor the I-beam;set_cursor the hand;set_cursor the I-beam;leave;'
fi

# ?2001: a click on the cursor's line moves the shell's point with the
# cursor keys.
session readline '
printf "\033[?2001h> abcdefgh"; touch $S/ready; exec cat > $S/got' <<'P'
move 34 8
click left
sleep 300
P
check "?2001: a click six columns back is six Lefts" $D/readline/got '\033[D\033[D\033[D\033[D\033[D\033[D'

# The side and extra buttons under ?1000 ?1006: buttons 8 to 11, "adding
# 128 (for buttons 8 through 11)" (ctlseqs), pressed and released at
# column 11, row 3; BTN_TASK, X's twelfth, which no encoding names, sends
# nothing; a drag under ?1002 adds the motion 32; ?1000 alone writes X10's
# bytes, whose release names no button.
session side '
printf "\033[?1002h\033[?1006h"; touch $S/ready
until [ -e $S/go1 ]; do sleep 0.05; done; printf "\033[?1006l"; touch $S/did1
exec cat > $S/got' <<'P'
move 84 42
click side
click extra
click forward
click back
click task
press side
move 92 42
release side
sh touch $S/go1
waitfor [ -e $S/did1 ]
sleep 200
click side
sleep 300
P
check "the side buttons: 128 to 131, past them nothing, a drag 160, X10" $D/side/got \
  '\033[<128;11;3M\033[<128;11;3m\033[<129;11;3M\033[<129;11;3m\033[<130;11;3M\033[<130;11;3m\033[<131;11;3M\033[<131;11;3m\033[<128;11;3M\033[<160;12;3M\033[<128;12;3m\033[M\240,#\033[M#,#'

# DEC's locator (DEC STD 070-13): the program asks and the pointer answers.
# The child reads its stdin in the background and writes each request
# when the pointer script says go, so a request lands between the
# pointer's events. DSR ?55 and ?56 answer a locator ready and a mouse;
# DECELR 1;2 reports in cells and DECSLE 1;3 asks for presses and
# releases: DECLRP Pe;Pb;Pr;Pc, the left button 2 and 3 with 4 held, the
# side button the fourth, 8 and 9 with 8 held, the extra button not one of
# the four, and Shift's click the selection's. DECEFR around the pointer
# reports event 10 once the pointer leaves it, and not again; DECRQLP
# answers event 1 where it is, in cells and under DECELR 1;1 in pixels
# from 1:1; DECELR 2 sends one report and stops; DECEFR with no corners is
# the pointer's own cell, and the pointer leaving the window reports it
# with the position omitted; DECELR 0 and DECRQLP answer 0, "not enabled".
session locator '
exec 3<&0; cat <&3 > $S/got & touch $S/ready
go() { until [ -e $S/go$1 ]; do sleep 0.05; done; printf "$2"; touch $S/did$1; }
go 1 "\033[?55n\033[?56n\033[1;2\047z\033[1;3\047{"
go 2 "\033[2;10;4;14\047w"
go 3 "\033[\047|"
go 4 "\033[1;1\047z\033[1\047|"
go 5 "\033[2;2\047z"
go 6 "\033[1;2\047z\033[\047w"
go 7 "\033[0\047z\033[\047|"
wait' <<'P'
move 84 42
sh touch $S/go1
waitfor [ -e $S/did1 ]
sleep 200
click left
click side
click extra
mods shift
tick 1000
click left
mods none
sh touch $S/go2
waitfor [ -e $S/did2 ]
sleep 200
move 108 42
move 124 42
move 140 42
sh touch $S/go3
waitfor [ -e $S/did3 ]
sh touch $S/go4
waitfor [ -e $S/did4 ]
sleep 200
tick 1000
click left
sh touch $S/go5
waitfor [ -e $S/did5 ]
sleep 200
tick 1000
click left
tick 1000
click left
sh touch $S/go6
waitfor [ -e $S/did6 ]
sleep 200
move 900 700
sleep 200
move 140 42
sh touch $S/go7
waitfor [ -e $S/did7 ]
sleep 300
P
check "DEC's locator: DSR, presses, the fourth, a filter, requests, pixels, one-shot, leaving, off" $D/locator/got \
  '\033[?50n\033[?57;1n\033[2;4;3;11&w\033[3;0;3;11&w\033[8;8;3;11&w\033[9;0;3;11&w\033[10;0;3;16&w\033[1;0;3;18&w\033[1;0;43;141&w\033[2;4;43;141&w\033[3;0;43;141&w\033[2;4;3;18&w\033[10;0&w\033[0&w'

# The OSC 133 prompt's click (the semantic-prompts proposal): with cl=line
# a click on the line being typed moves the cursor by characters, a wide
# one counted once, a click in the left prompt counted as the input's
# first character and one past the input as just after it, and a click on
# another row is not the prompt's; it takes the click over ?2001, which
# would count columns. The input is "ab", a wide character and "cd" after
# a two-column prompt, so the cursor is five characters in. Then, each
# after a command that ran and ended (C, D), a prompt with kitty's
# click_events=1 is sent a left press at the screen's row, and one with
# click_events=2 at its row counted from the prompt's top line, both
# 1-based.
session prompt '
printf "\033[?2001h\033]133;A;cl=line\033\\\\$ \033]133;B\033\\\\ab\344\270\255cd"
exec 3<&0; cat <&3 > $S/got & touch $S/ready
until [ -e $S/go1 ]; do sleep 0.05; done
printf "\r\n\033]133;C\033\\\\\033]133;D;0\033\\\\\033]133;A;click_events=1\033\\\\> \033]133;B\033\\\\xy"; touch $S/did1
until [ -e $S/go2 ]; do sleep 0.05; done
printf "\r\n\033]133;C\033\\\\\033]133;D;0\033\\\\\033]133;A;click_events=2\033\\\\> \033]133;B\033\\\\xy"; touch $S/did2
wait' <<'P'
move 28 8
click left
move 44 8
tick 1000
click left
move 4 8
tick 1000
click left
move 164 8
tick 1000
click left
move 28 42
tick 1000
click left
sh touch $S/go1
waitfor [ -e $S/did1 ]
sleep 200
move 28 25
tick 1000
click left
sh touch $S/go2
waitfor [ -e $S/did2 ]
sleep 200
move 28 42
tick 1000
click left
sleep 300
P
check "the prompt's click: characters, a wide one once, the prompt, past the input, another row; click_events 1 and 2" $D/prompt/got \
  '\033[D\033[D\033[D\033[D\033[D\033[D\033[D\033[D\033[D\033[D\033[D\033[D\033[<0;4;2M\033[<0;4;1M'
fi

# The grid's edges at every scale, under ?1000 ?1006 on the alternate
# screen, where no history puts the scrollbar's thumb beside the last
# column: the first cell; the last column and row from the window's last
# point, on the lights' line below the grid, and from the first point in
# the last cell; the cell before it; and
# the same two from half points, the extent doubled. Then the pixel
# report, ?1016, from the first and the last point: the frame's pixel each
# is in, which at a scale of 2 or 1.5 is not the point. The points and the
# pixel are the run's (the table at the end of this script).
session edges '
printf "\033[?1049h\033[?1000h\033[?1006h"; touch $S/ready
until [ -e $S/go1 ]; do sleep 0.05; done; printf "\033[?1016h"; touch $S/did1
exec cat > $S/got' <<P
move 0 0
click left
move $LAST
tick 1000
click left
move $FIRST
tick 1000
click left
move $BEFORE
tick 1000
click left
extent 2048 1536
move $HALF_LAST
tick 1000
click left
move $HALF_BEFORE
tick 1000
click left
extent 1024 768
sh touch \$S/go1
waitfor [ -e \$S/did1 ]
sleep 200
move 0 0
tick 1000
click left
move $LAST
tick 1000
click left
sleep 300
P
l="80;$ROWS" b="79;$((ROWS - 1))"
check "at scale $SCALE, the edges: 1;1, the last cell three ways, the one before twice, pixels" $D/edges/got \
  '\033[<0;1;1M\033[<0;1;1m\033[<0;'"$l"'M\033[<0;'"$l"'m\033[<0;'"$l"'M\033[<0;'"$l"'m\033[<0;'"$b"'M\033[<0;'"$b"'m\033[<0;'"$l"'M\033[<0;'"$l"'m\033[<0;'"$b"'M\033[<0;'"$b"'m\033[<0;1;1M\033[<0;1;1m\033[<0;'"$PIXEL"'M\033[<0;'"$PIXEL"'m'
# What the app drew its surfaces at: the window's buffer, any buffer scale
# and viewport destination it set, the scales the compositor sent it, and
# the pointer's image, drawn here on sway 1.7 only, its buffer and scale.
grep '^surface' $O/trace-edges.txt | awk '!seen[$0]++' > $D/edges/surfaces
cat $D/edges/surfaces | sed 's/^/        /'
# The window at the scale the app draws at here, $APP: its last buffer the
# window's points at it, rounded halfway away from zero; with
# fractional-scale-v1 -- which sway 1.10 sends at 1 too, as 120 -- the
# buffer scale left at one and the viewport's destination the window's
# points, else the buffer scale the integer and no destination, and at 1
# neither. Every buffer it attached, in order: the first drawn before the
# compositor said a scale, then one for each size and scale, none drawn
# twice, and on the arm that draws a fraction none at the integer scale
# sway sends beside it, which would be a frame laid out for a scale the
# window is not drawn at.
grep '^surface window attach' $O/trace-edges.txt | tail -1 > $D/edges/buffer
check "at scale $SCALE, the window drawn at $APP: its buffer" $D/edges/buffer "surface window attach $BUFFER\n"
grep '^surface window buffer_scale' $O/trace-edges.txt | tail -1 | grep -v ' 1$' > $D/edges/scaled || true
grep '^surface window destination' $O/trace-edges.txt | tail -1 > $D/edges/mapped || true
if grep -q wp_fractional_scale_manager_v1 $O/compositor.txt; then
  check "at scale $SCALE, the buffer scale left at one" $D/edges/scaled ''
  check "at scale $SCALE, the buffer through the viewport" $D/edges/mapped "surface window destination ${WIN% *}x${WIN#* }\n"
elif [ "$APP" = 1 ]; then
  cat $D/edges/scaled $D/edges/mapped > $D/edges/told
  check "at scale $SCALE, nothing of a scale told" $D/edges/told ''
else
  check "at scale $SCALE, the buffer scale $APP" $D/edges/scaled "surface window buffer_scale $APP\n"
  check "at scale $SCALE, no viewport" $D/edges/mapped ''
fi
grep '^surface window attach' $O/trace-edges.txt | sed 's/^surface window attach //' > $D/edges/buffers
echo "        the window's buffers: $(tr '\n' ' ' < $D/edges/buffers)"
sort $D/edges/buffers | uniq -d > $D/edges/twice
check "at scale $SCALE, no buffer size drawn twice" $D/edges/twice ''
whole=$(awk "BEGIN { s = $APP; print (s == int(s)) ? 0 : int(s) + 1 }")
if [ "$whole" != 0 ]; then
  grep -x "$((${WIN% *} * whole))x$((${WIN#* } * whole))" $D/edges/buffers > $D/edges/at-whole || true
  check "at scale $SCALE, no frame at the integer scale $whole" $D/edges/at-whole ''
fi
if grep -q wp_cursor_shape_manager_v1 $O/compositor.txt; then
  grep '^surface pointer' $D/edges/surfaces > $D/edges/drawn || true
  check "at scale $SCALE, the pointer is named, not drawn" $D/edges/drawn ''
else
  want=$(awk "BEGIN { s = $SCALE; c = (s == int(s)) ? s : int(s) + 1; print c }")
  printf 'surface pointer attach %sx%s\nsurface pointer buffer_scale %s\n' \
    $((24 * want)) $((24 * want)) $want > $D/edges/drawn-want
  grep '^surface pointer' $D/edges/surfaces > $D/edges/drawn || true
  check "at scale $SCALE, the pointer drawn at scale $want" $D/edges/drawn \
    "$(cat $D/edges/drawn-want)\n"
fi

# Every shape and hide named the serial of the enter before it.
cat $O/trace-*.txt | grep -c "NOT the last" > $D/stale
check "every set_cursor and set_shape carries the last enter's serial" $D/stale '0\n'

kill $SWAY; wait $SWAY 2>/dev/null
cp sway.conf sway.log $O/
exit $FAILED
INNER
)

# Each run: the arm, the output's scale, the scale the app draws at there,
# the window in points, the edges' points -- the window's last, the first
# in the last cell, the one before it, and the last and the one before in
# half points -- the pixel report at the last point, from one, and the
# window's buffer. A point is the scale's pixels, the cell the pixel is
# in, and a point on the lights' line below the grid, or a pixel past the
# grid's last, is the grid's last row. The grid is 80x23 at every scale
# (the header), its last row 22 counted from 0:
#
#   app 1    8x17 cells, 640x408 points, the grid 391 pixels tall: the
#            last cell's first point (632, 374); the one before (631,
#            373); (639, 407) is on the lights' line, reported 640;391
#   app 2    16x34 cells, the grid 782 pixels tall; (632, 374) is (1264,
#            748); (639, 407) is (1278, 814) and at 640x416 (639, 415) is
#            (1278, 830), both past the grid, 1279;782
#   app 1.5  12x26 cells, 640x416 points, the grid 598 pixels tall: the
#            last cell's first point (632, 382), (948, 573); the one before
#            (631, 381), (946.5, 571.5), and (631.5, 381) in half points;
#            (639, 415) is (958.5, 622.5), past the grid, 959;598
runs="
bookworm 1   1   640_408 639_407 632_374 631_373 1279_815 1263_747 640;391  640x408
trixie   1   1   640_408 639_407 632_374 631_373 1279_815 1263_747 640;391  640x408
bookworm 2   2   640_408 639_407 632_374 631_373 1279_815 1263_747 1279;782 1280x816
trixie   2   2   640_408 639_407 632_374 631_373 1279_815 1263_747 1279;782 1280x816
bookworm 1.5 2   640_416 639_415 632_374 631_373 1279_831 1263_747 1279;782 1280x832
trixie   1.5 1.5 640_416 639_415 632_382 631_381 1279_831 1263_762 959;598  960x624
"
status=0
while read -r arm scale app win last first before half_last half_before pixel buffer; do
  [ -n "$arm" ] || continue
  dir=$arm && [ "$scale" = 1 ] || dir=$arm-x$scale
  mkdir -p "$out/$dir"
  echo "== $arm, scale $scale"
  sp() { echo "${1/_/ }"; }
  docker run --rm -i -e SCALE="$scale" -e APP="$app" -e WIN="$(sp "$win")" \
    -e LAST="$(sp "$last")" -e FIRST="$(sp "$first")" -e BEFORE="$(sp "$before")" \
    -e HALF_LAST="$(sp "$half_last")" -e HALF_BEFORE="$(sp "$half_before")" \
    -e PIXEL="$pixel" -e BUFFER="$buffer" \
    -v "$root":/src:ro -v "$tag-target":/target:ro \
    "$tag-$arm" bash -c \
    'cat > /tmp/arm.sh && chmod 755 /tmp/arm.sh && runuser -u baud -- env SCALE="$SCALE" APP="$APP" WIN="$WIN" LAST="$LAST" FIRST="$FIRST" BEFORE="$BEFORE" HALF_LAST="$HALF_LAST" HALF_BEFORE="$HALF_BEFORE" PIXEL="$PIXEL" BUFFER="$BUFFER" bash /tmp/arm.sh > /tmp/results.txt 2>&1; s=$?; mkdir -p /tmp/out; cp /tmp/results.txt /tmp/out/; tar -C /tmp/out -c .; exit $s' \
    <<<"$inner" 2>"$out/$dir/container.log" | tar -C "$out/$dir" -x || status=1
  cat "$out/$dir/results.txt"
done <<<"$runs"
exit $status
