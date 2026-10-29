#!/usr/bin/env bash
# Moves a pointer over the Linux app on a real compositor and checks what
# it does: the bytes a program reads, the selections a drag leaves on the
# compositor's primary selection and clipboard, the command a link click
# would run, and the pointer images the app asks the compositor for.
#
#   scripts/linux-pointer.sh <outdir>
#
# Needs docker with an aarch64 Linux engine (colima's default VM on an
# Apple silicon Mac), and the repository under a directory the engine
# mounts. Nothing opens a window here: the compositor is headless, inside
# the container. The research note docs/research/notes/be-linux-pixels.md
# holds a run's results.
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
# The pointer and the keyboard are scripts/linux-pointer.py, one client
# holding one zwlr_virtual_pointer_v1 and one zwp_virtual_keyboard_v1, so
# a drag is one device's press, moves and release, and every event carries
# a time from the script's own clock. sway runs with WAYLAND_DEBUG=server,
# which logs each request the app sends it; the pointer requests are
# kept, without their timestamps and object numbers, as the trace of what
# the app asked the compositor to show. grim takes the screen with the
# pointer drawn on it.
#
# The window floats at 640x408 at the origin of a 1024x768 output: 80x24
# cells of 8x17 px, so a cell's centre is (8c + 4, 17r + 8), and a point
# past (640, 408) is off the window. Every check prints "ok" or "FAILED"
# with what was read; the script exits non-zero when one failed or a step
# could not run.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:?usage: scripts/linux-pointer.sh <outdir>}
mkdir -p "$out"
out=$(cd "$out" && pwd)

toolchain=$(rustc -Vv | sed -n 's/^commit-hash: //p')
nightly=${BAUD_NIGHTLY:-nightly-2026-04-11}
docker build -q -t baud-linux-pointer-build - >/dev/null <<EOF
FROM rustlang/rust:nightly-bookworm
RUN rustup toolchain install $nightly --profile minimal \
    && rustup default $nightly
EOF
there=$(docker run --rm baud-linux-pointer-build rustc -Vv | sed -n 's/^commit-hash: //p')
if [ "$there" != "$toolchain" ]; then
  echo "the container's rustc is $there, this Mac's $toolchain; set BAUD_NIGHTLY" >&2
  exit 1
fi
for arm in bookworm trixie; do
  docker build -q -t baud-linux-pointer-$arm - >/dev/null <<EOF
FROM debian:$arm
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
    --no-install-recommends sway grim wl-clipboard libxkbcommon-tools \
    wayland-utils dmz-cursor-theme python3 \
    && rm -rf /var/lib/apt/lists/* && useradd -m baud
EOF
done
docker volume create baud-linux-pointer-target >/dev/null
docker volume create baud-linux-pointer-registry >/dev/null
docker run --rm -v "$root":/src:ro \
  -v baud-linux-pointer-target:/target \
  -v baud-linux-pointer-registry:/usr/local/cargo/registry \
  baud-linux-pointer-build bash -c \
  'cd /src && CARGO_TARGET_DIR=/target RUSTFLAGS="-D warnings" cargo build -q --release -p baud --bin baud'

# One arm, as the user baud (sway will not run as root). Results go to
# /tmp/out, which leaves the container as tar on stdout.
inner=$(
  cat <<'INNER'
set -u
mkdir -p /tmp/out && D=$(mktemp -d) && cd "$D"
O=/tmp/out
export XDG_RUNTIME_DIR=$D/xdg && mkdir -m 0700 "$XDG_RUNTIME_DIR"
cat > sway.conf <<C
output HEADLESS-1 resolution 1024x768 position 0 0
default_border none
default_floating_border none
seat seat0 xcursor_theme DMZ-Black 24
for_window [app_id="org.baud.terminal"] floating enable, resize set 640 408, move position 0 0
C
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
  WAYLAND_DEBUG=server sway -c sway.conf > sway.log 2>&1 &
SWAY=$!
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1
export SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock)
{
  sway -v
  wayland-info | grep -E "'(wl_seat|wp_cursor_shape_manager_v1|zwlr_virtual_pointer_manager_v1|zwp_virtual_keyboard_manager_v1|zwp_primary_selection_device_manager_v1)'" |
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
# script on stdin. The child waits for the window's 80x24 before it writes.
session() {
  S=$D/$1 && mkdir -p "$S" && : > "$S/got"
  printf 'until [ "$(stty size)" = "24 80" ]; do sleep 0.05; done\nstty raw -echo\n%s\n' "$2" > "$S/child.sh"
  # Each session starts with the pointer off the window.
  echo "move 900 700" | python3 /src/scripts/linux-pointer.py keymap.xkb
  from=$(wc -l < sway.log)
  BAUD_LOG=debug BAUD_CONFIG=baud.toml S=$S \
    BAUD_CMD="bash $S/child.sh" /target/release/baud > "$S/baud.log" 2>&1 &
  BAUD=$!
  for _ in $(seq 100); do [ -e "$S/ready" ] && break; sleep 0.1; done
  [ -e "$S/ready" ] || { echo "FAILED  $1: the child never started"; FAILED=1; }
  sleep 0.5
  S=$S python3 /src/scripts/linux-pointer.py keymap.xkb
  sleep 0.3
  kill $BAUD; wait $BAUD 2>/dev/null
  # The app's pointer requests and the pointer events it was sent, in
  # order, without timestamps, serials or object numbers.
  tail -n +$((from + 1)) sway.log | python3 /src/scripts/linux-pointer-trace.py > $O/trace-$1.txt
  cp "$S/baud.log" $O/baud-$1.log
}

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
sleep 300
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
sleep 300
sh shot wheel-top plain
P
check "three detents up, the top row" $D/wheel/top 'line 75'
check "three down again, the top row" $D/wheel/live 'line 78'
check "a touchpad's 51 px up, three lines of 17, the top row" $D/wheel/swiped 'line 75'
check "the thumb dragged to the top, the top row" $D/wheel/thumb 'line 1'
check "the wheel on the primary screen sends the program nothing" $D/wheel/got ''
# Rows 3 to 23 three lines back against rows 0 to 20 live, short of the
# scrollbar's column: 600x357 pixels each.
px="python3 /src/scripts/linux-pixels.py"
$px crop $O/wheel-back.png 0 51 600 357 $D/wheel/back.pam
$px crop $O/wheel-live.png 0 0 600 357 $D/wheel/live.pam
$px diff $D/wheel/back.pam $D/wheel/live.pam | head -1 > $D/wheel/drawn
check "three lines back, the window draws the history" $D/wheel/drawn \
  '0 of 214200 pixels differ, largest channel difference 0\n'
# The thumb's frame at the top is the wheel's at the top below the first
# row, which the triple click between them leaves selected, and not the
# live screen's rows.
$px crop $O/thumb-top.png 0 17 600 391 $D/wheel/thumb.pam
$px crop $O/wheel-top.png 0 17 600 391 $D/wheel/top.pam
$px diff $D/wheel/thumb.pam $D/wheel/top.pam | head -1 > $D/wheel/thumb-drawn
check "the thumb at the top draws what the wheel at the top draws" $D/wheel/thumb-drawn \
  '0 of 234600 pixels differ, largest channel difference 0\n'
$px crop $O/thumb-top.png 0 0 600 357 $D/wheel/thumb-rows.pam
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
# folded: on sway 1.7 its own two images, on sway 1.10 the shapes by name.
grep -E '^(enter|leave|set_|capabilities)' $O/trace-shape.txt | sed 's/ [0-9]* [0-9]*$//' |
  uniq | tr '\n' ';' > $D/shape/seq
if grep -q wp_cursor_shape_manager_v1 $O/compositor.txt; then
  check "the shapes, named: I-beam, hidden, hand, hidden, arrow, hidden, watch, hidden, I-beam" $D/shape/seq \
    'enter;set_shape text;set_cursor hidden;set_shape pointer;set_cursor hidden;set_shape default;set_cursor hidden;set_shape wait;set_cursor hidden;set_shape text;leave;enter;set_shape text;leave;capabilities keyboard;capabilities none;capabilities pointer;capabilities pointer keyboard;enter;set_shape text;leave;capabilities keyboard;capabilities none;'
else
  check "the shapes, drawn: the I-beam, and the arrow for hand, arrow and watch" $D/shape/seq \
    'enter;set_cursor the I-beam;set_cursor hidden;set_cursor the arrow;set_cursor hidden;set_cursor the arrow;set_cursor hidden;set_cursor the arrow;set_cursor hidden;set_cursor the I-beam;leave;enter;set_cursor the I-beam;leave;capabilities keyboard;capabilities none;capabilities pointer;capabilities pointer keyboard;enter;set_cursor the I-beam;leave;capabilities keyboard;capabilities none;'
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

# Links: Ctrl held over an OSC 8 link previews it, a Ctrl click opens it
# with xdg-open, and a Ctrl click on a URI in the text opens that; the
# pointer leaving takes the preview away. Under ?1000 ?1006 the Ctrl
# click on a link is still the link's, and elsewhere the program's.
session link '
printf "see \033]8;;https://example.org/p14\033\\\\this link\033]8;;\033\\\\ or https://example.com/plain\r\n"
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

# ?2001: a click on the cursor's line moves the shell's point with the
# cursor keys.
session readline '
printf "\033[?2001h> abcdefgh"; touch $S/ready; exec cat > $S/got' <<'P'
move 34 8
click left
sleep 300
P
check "?2001: a click six columns back is six Lefts" $D/readline/got '\033[D\033[D\033[D\033[D\033[D\033[D'

# Every shape and hide named the serial of the enter before it.
cat $O/trace-*.txt | grep -c "NOT the last" > $D/stale
check "every set_cursor and set_shape carries the last enter's serial" $D/stale '0\n'

kill $SWAY; wait $SWAY 2>/dev/null
cp sway.conf sway.log $O/
exit $FAILED
INNER
)

status=0
for arm in bookworm trixie; do
  mkdir -p "$out/$arm"
  echo "== $arm"
  docker run --rm -i -v "$root":/src:ro -v baud-linux-pointer-target:/target:ro \
    baud-linux-pointer-$arm bash -c \
    'cat > /tmp/arm.sh && chmod 755 /tmp/arm.sh && runuser -u baud -- bash /tmp/arm.sh > /tmp/results.txt 2>&1; s=$?; mkdir -p /tmp/out; cp /tmp/results.txt /tmp/out/; tar -C /tmp/out -c .; exit $s' \
    <<<"$inner" 2>"$out/$arm/container.log" | tar -C "$out/$arm" -x || status=1
  cat "$out/$arm/results.txt"
done
exit $status
