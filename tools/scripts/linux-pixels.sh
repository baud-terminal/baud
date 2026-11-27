#!/usr/bin/env bash
# Draws the same frames on this Mac and on Linux and compares them pixel
# for pixel, with the Linux app's frames taken from a real compositor.
#
#   tools/scripts/linux-pixels.sh <outdir>
#
# Needs docker with an aarch64 Linux engine (colima's default VM on an
# Apple silicon Mac), and the repository under a directory the engine
# mounts (colima mounts the home directory). Nothing opens a window here:
# the compositors are headless, inside the container.
#
# Three arms, each fixture the same bytes -- a committed recording with
# `CSI ? 25 l` after it, so no cursor is drawn (the Linux app starts
# without the keyboard and would draw the hollow cursor; the bench draws
# none of its own):
#
#   mac/<f>.pam     the `pixels` bench on this Mac: the renderer, set up
#                   as the Linux driver sets it up with the default config
#   linux/<f>.pam   the same bench, same source, same nightly, in the
#                   container
#   shot/<f>.png    the Linux app itself (`baud`, the Wayland driver) under
#                   weston 10's headless backend with the pixman renderer
#                   and desktop-shell, taken by weston-screenshooter once
#                   the frame has settled -- or, for a fixture marked sway,
#                   under sway 1.10.1's headless backend, taken by grim
#
# Each fixture is drawn at a scale of one, and three again at two: weston's
# output at scale 2, which the app reads from wl_output.scale and draws at,
# its buffer twice the window's points with set_buffer_scale 2, and the
# benches at PIXELS_SCALE=2, the 13-point font at 26 pixels. The window is
# 800x600 points at both, so its grid is 100x35 at both: 8x17 cells and
# 16x34.
#
# weston's headless output takes only whole scales, so the fraction is
# sway's: sway 1.10.1 offers wp_fractional_scale_manager_v1, and at an
# output scale of 1.5 the app draws a buffer one and a half times its
# points through a viewport, the benches at PIXELS_SCALE=1.5, 12x26 cells.
# The window floats at the output's origin at 800x608 points, whose grid
# is 100x35 at 1, 1.5 and 2 (608 points are 912 pixels at 1.5, 35 rows of
# 26), and the output is the window's size, its background black (swaybg),
# no pointer device on it. Each fixture runs at 1.5, and less-pager at 1
# and 2 and the control at 1 under sway too. sway runs in Debian trixie, with
# the faces the bench read -- bookworm's DejaVu, sent out of the build
# container -- in place of trixie's own, so both read the same files.
#
# Then the window moves between two outputs of different scales: under
# sway 1.7 (bookworm), which offers no fraction and whose wl_compositor is
# version 4, outputs at 1 and 2, the app drawing at the largest scale of
# the outputs its surface entered; under sway 1.10.1, outputs at 1 and 1.5.
# swaymsg moves the floating window from the first output across the two,
# onto the second, across the two again and back. Across, 500 of its 800
# points are on the first output and 300 on the second, and it gets there
# once from each side, so a scale kept from the step before would differ
# between the two crossings. The frame on each output is
# compared with the bench at that output's scale, and what the app
# attached and told the compositor, from sway's WAYLAND_DEBUG=server log,
# is printed for each step.
#
# and three comparisons: mac against linux (the renderer on two systems),
# linux composited over black against the screenshot (the driver, the
# premultiply and the compositor), and a control fixture -- the screen
# cleared to one colour -- whose screenshot must hold one known value.
# Both benches read the system face the container's default stack finds,
# DejaVu Sans Mono, from the copy the container sends out.
#
# Then the clipboard against the compositor's, on sway's headless backend
# (weston's has no seat): a program's OSC 52 write read back by wl-paste,
# and wl-copy's text pasted with Ctrl+Shift+V typed by wtype, on the
# keyboard that typed before and on a second one.
#
# The app runs with `[font] dpi = 72` in every arm, so its 13 points are
# the benches' 13 pixels (26 at scale 2): the parity is between the same
# size in pixels, and Linux's own point, 96 dpi times the desktop's text
# scaling (`typesize.rs`), is a larger size the benches do not draw.
#
# The app runs with `padding = 0` in every arm, and the grid is cropped
# from the screenshot where the app centres it, half of what the window
# leaves beyond whole cells on each axis (`origin.rs`): 6 px down at
# 800x608 and scale 1, 35 rows of 17 leaving 13.
#
# The app runs with `[window] activity_lights = "off"` in every arm: the
# bench draws the grid alone, and the lights' own line, which the Linux app
# draws at the window's top by default (`config::lights_at`), would take
# a row of the grid the bench has (35 rows of 17 at 800x600 would be 34)
# and draw pixels the bench has no reference for. The pointer checks
# (`linux-pointer.sh`) keep the default, lights and all.
#
# Every result is printed and kept under <outdir>; the script exits 0
# whatever the pixels say, and non-zero only when a step could not run.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
out=${1:?usage: tools/scripts/linux-pixels.sh <outdir>}
mkdir -p "$out"/streams "$out"/mac "$out"/linux "$out"/shot "$out"/diff
out=$(cd "$out" && pwd)
# BAUD_PIXELS_TAG names the image and volumes it makes (by default
# baud-linux-pixels), so two runs on one engine keep apart.
image=${BAUD_PIXELS_TAG:-baud-linux-pixels}

# name, recording under tools/tests/recordings (or "control"), grid, scale, and
# the compositor the app's frame is taken under
fixtures="
less-pager ascii/less-pager.raw 100x35 1 weston
cat-cjk utf8/cat-cjk.raw 100x35 1 weston
vim-unicode utf8/vim-unicode.raw 100x35 1 weston
btop utf8/btop.raw 100x35 1 weston
swirl sixel/swirl.raw 100x35 1 weston
control control 100x35 1 weston
less-pager-x2 ascii/less-pager.raw 100x35 2 weston
cat-cjk-x2 utf8/cat-cjk.raw 100x35 2 weston
control-x2 control 100x35 2 weston
less-pager-sway ascii/less-pager.raw 100x35 1 sway
control-sway control 100x35 1 sway
less-pager-sway-x2 ascii/less-pager.raw 100x35 2 sway
less-pager-x1.5 ascii/less-pager.raw 100x35 1.5 sway
cat-cjk-x1.5 utf8/cat-cjk.raw 100x35 1.5 sway
vim-unicode-x1.5 utf8/vim-unicode.raw 100x35 1.5 sway
btop-x1.5 utf8/btop.raw 100x35 1.5 sway
swirl-x1.5 sixel/swirl.raw 100x35 1.5 sway
control-x1.5 control 100x35 1.5 sway
"

# The control: the page cleared to rgb(185, 37, 105), which the app
# dims as it dims every page (`[glass] app_backgrounds = "dimmed"`); each
# channel's premultiplied value is a rounding up, so a truncation would
# read one less on all three.
control=$'\033[48;2;185;37;105m\033[2J\033[?25l'

echo "$fixtures" | while read -r name rec ws scale comp; do
  [ -n "$name" ] || continue
  echo "$scale" >"$out/streams/$name.scale"
  echo "$comp" >"$out/streams/$name.comp"
  if [ "$rec" = control ]; then
    printf '%s' "$control" >"$out/streams/$name.bin"
  else
    {
      cat "$root/tools/tests/recordings/$rec"
      printf '\033[?25l'
    } >"$out/streams/$name.bin"
  fi
  echo "$ws" >"$out/streams/$name.ws"
done

toolchain=$(rustc -Vv | sed -n 's/^commit-hash: //p')

# The image: weston 10.0.1 and wl-clipboard from Debian bookworm, and the
# nightly whose commit is this Mac's, checked below.
nightly=${BAUD_NIGHTLY:-nightly-2026-11-06}
docker build -q -t "$image" - >/dev/null <<EOF
FROM rustlang/rust:nightly-bookworm
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
    --no-install-recommends weston sway wtype wl-clipboard \
    && rm -rf /var/lib/apt/lists/* && useradd -m baud
RUN rustup toolchain install $nightly --profile minimal \
    && rustup default $nightly
EOF
there=$(docker run --rm "$image" rustc -Vv | sed -n 's/^commit-hash: //p')
if [ "$there" != "$toolchain" ]; then
  echo "the container's rustc is $there, this Mac's $toolchain; set BAUD_NIGHTLY" >&2
  exit 1
fi

docker volume inspect "$image-target" >/dev/null 2>&1 || docker volume create "$image-target" >/dev/null
docker volume inspect "$image-registry" >/dev/null 2>&1 || docker volume create "$image-registry" >/dev/null

# Inside: build, then each fixture through the bench and through the app
# under a weston of its own. weston's desktop-shell leaves a window's size
# to the client, and the Linux window then opens at 800x600, so the output
# is 800x600: the window is placed at the origin and fills it, and its
# grid at 13 px is 100x35, 800x595, the five rows below it the page.
# kiosk-shell would size the window itself, but weston 10's shows nothing
# to its screenshooter, not even weston-simple-shm, where desktop-shell
# does. Streams in and results out as tar on stdin and
# stdout; the log on stderr.
inner=$(
  cat <<'INNER'
set -euo pipefail
exec 3>&1 1>&2
mkdir -p /io && tar -C /io -x
mkdir -p /io/linux /io/shot /io/clip /io/faces/system
cd /src
export CARGO_TARGET_DIR=/target RUSTFLAGS="-D warnings"
cargo build -q --release -p baud --bin baud
cargo build -q --release -p baud-bench --example linux-pointer-trace --example linux-pixels
cargo bench -q -p baud-bench --bench pixels --no-run
# The system face the default stack finds here, which the bench is told
# of and which goes out for the Mac arm to read.
mono=/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf
cp "$mono" /io/faces/
cp /usr/share/fonts/truetype/dejavu/*.ttf /io/faces/system/
export XDG_RUNTIME_DIR=/tmp/xdg && mkdir -p -m 0700 $XDG_RUNTIME_DIR
printf 'padding = 0\n[launch]\nsplash = "off"\n[window]\nactivity_lights = "off"\n[link]\npaths = "off"\n[font]\ndpi = 72\n' > /tmp/baud.toml
printf '[shell]\npanel-position=none\nbackground-color=0xff000000\nlocking=false\nanimation=none\nstartup-animation=none\n' > /tmp/weston.ini
# The headless backend's width and height are the output's points: at
# --scale=2 an 800x600 output is 1600x1200 pixels, and its screenshot too.
start_weston() { # width and height in points, scale
  rm -f $XDG_RUNTIME_DIR/wayland-1*
  weston --config=/tmp/weston.ini --backend=headless-backend.so --use-pixman \
    --width="$1" --height="$2" --scale="${3:-1}" \
    --shell=desktop-shell.so --debug \
    --socket=wayland-1 --idle-time=0 >/tmp/weston.log 2>&1 &
  WESTON=$!
  for _ in $(seq 100); do [ -S $XDG_RUNTIME_DIR/wayland-1 ] && break; sleep 0.1; done
  export WAYLAND_DISPLAY=wayland-1
  sleep 1
}
shoot() {
  local dir; dir=$(mktemp -d)
  (cd "$dir" && weston-screenshooter >/dev/null 2>&1)
  mv "$dir"/*.png "$1"
  rmdir "$dir"
}
for s in /io/streams/*.bin; do
  n=$(basename "$s" .bin)
  ws=$(cat /io/streams/$n.ws)
  sc=$(cat /io/streams/$n.scale)
  PIXELS_WS=$ws PIXELS_FACES=$mono PIXELS_SCALE=$sc \
    cargo bench -q -p baud-bench --bench pixels -- "$s" /io/linux/$n.pam
  [ "$(cat /io/streams/$n.comp)" = weston ] || continue
  start_weston 800 600 "$sc"
  # The child waits for the window's grid, so no byte is drawn at the
  # 80x24 the pty is spawned at and then reflowed; then raw, so the tty
  # passes the bytes as they are: no LF made CR LF, no answer to a query
  # echoed onto the screen. Written, it leaves <name>.fed beside the
  # screenshot: a grid that never came leaves the page empty, and the
  # comparison names that rather than the pixels it costs.
  BAUD_LOG=debug BAUD_CONFIG=/tmp/baud.toml \
    BAUD_CMD="until [ \"\$(stty size)\" = '35 100' ]; do sleep 0.05; done; stty raw -echo; cat $s; : >/io/shot/$n.fed; exec sleep 600" \
    /target/release/baud >/io/shot/$n.log 2>&1 &
  BAUD=$!
  sleep 4
  shoot /io/shot/$n.png
  sleep 1
  shoot /io/shot/$n.again.png
  kill $BAUD; wait $BAUD 2>/dev/null || true
  kill $WESTON; wait $WESTON 2>/dev/null || true
done
# weston's headless backend has no seat, so neither wl-paste nor the app
# has a data device there.
start_weston 640 408
echo "weston: wl-copy: $(timeout 5 wl-copy weston 2>&1); wl-paste: $(timeout 5 wl-paste -n 2>&1)" > /io/clip/result.txt
kill $WESTON; wait $WESTON 2>/dev/null || true
# The clipboard on sway's headless backend, which has a seat and the
# virtual-keyboard protocol wtype types through; sway will not run as
# root. Keys give the app focus and the serial a selection is set with.
cat > /tmp/clip.sh <<'CLIP'
set -u
D=$(mktemp -d) && cd "$D"
export XDG_RUNTIME_DIR=$D/xdg && mkdir -m 0700 "$XDG_RUNTIME_DIR"
printf 'output HEADLESS-1 resolution 640x408\nbar {\n mode invisible\n}\n' > sway.conf
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
  sway -c sway.conf > sway.log 2>&1 &
SWAY=$!
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1
export SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock)
wl-copy before
echo "sway control: wl-copy then wl-paste: [$(timeout 5 wl-paste -n 2>&1)]"
printf 'padding = 0\n[launch]\nsplash = "off"\n[window]\nactivity_lights = "off"\n[link]\npaths = "off"\n[font]\ndpi = 72\n' > baud.toml
# The child: one key, then a program's OSC 52 write, then whatever
# arrives -- a paste -- kept in a file.
cat > child.sh <<C
stty raw -echo
touch $D/ready
head -c 1 > $D/key
printf '\033]52;c;%s\a' "\$(printf 'from baud' | base64)"
touch $D/copied
exec cat > $D/pasted
C
: > pasted
BAUD_CONFIG=baud.toml BAUD_CMD="bash $D/child.sh" /target/release/baud > baud.log 2>&1 &
BAUD=$!
# Each step waits, up to five seconds, for what the one before it does.
wait_for() { for _ in $(seq 50); do eval "$1" && return 0; sleep 0.1; done; return 1; }
wait_for '[ -e ready ] && swaymsg -t get_tree | grep -q org.baud.terminal' ||
  echo "the window or the child did not come up"
# One virtual keyboard for a key and the paste chord: wtype's sleeps keep
# it alive, so the app has bound it before the first key, and the copy and
# wl-copy's offer land in the gap.
wtype -s 1000 x -s 3000 -M ctrl -M shift v -m shift -m ctrl &
WTYPE=$!
wait_for '[ -e copied ]'
echo "the key reached the child: [$(cat key 2>/dev/null)]"
sleep 0.3
echo "wl-paste after the program's OSC 52: [$(timeout 2 wl-paste -n 2>&1)]"
wl-copy "one"
wait $WTYPE
wait_for '[ -s pasted ]'
echo "Ctrl+Shift+V pasted, the same keyboard: [$(cat pasted)]"
# A second keyboard: the first was removed when wtype left, so the seat
# lost and regained the keyboard capability.
wl-copy "two"
wtype -s 1000 -M ctrl -M shift v -m shift -m ctrl
wait_for 'grep -q two pasted'
echo "Ctrl+Shift+V pasted, a second keyboard: [$(cat pasted)]"
kill $BAUD; kill $SWAY; wait $SWAY 2>/dev/null
CLIP
runuser -u baud -- bash /tmp/clip.sh >> /io/clip/result.txt 2>&1 || true
tar -C /io -c linux shot clip faces >&3
INNER
)
tar -C "$out" -c streams |
  docker run --rm -i -v "$root":/src:ro \
    -v "$image-target":/target \
    -v "$image-registry":/usr/local/cargo/registry \
    "$image" bash -c "$inner" 2>"$out/container.log" |
  tar -C "$out" -x

# sway: the fixtures marked sway under sway 1.10.1 (trixie) at their scale,
# then the window moved between two outputs under sway 1.7 (bookworm) and
# sway 1.10.1. The app is the one the build container built, from the
# target volume; sway will not run as root, so each stage runs as the user
# baud. Streams and faces in, results out, as tar.
for arm in bookworm trixie; do
  docker build -q -t "$image-$arm" - >/dev/null <<EOF
FROM debian:$arm
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
    --no-install-recommends sway grim wl-clipboard libxkbcommon-tools \
    wayland-utils dmz-cursor-theme \
    && rm -rf /var/lib/apt/lists/* && useradd -m baud
EOF
done
stage=$(
  cat <<'STAGE'
set -u
O=/io/out
D=$(mktemp -d) && cd "$D"
export XDG_RUNTIME_DIR=$D/xdg && mkdir -m 0700 "$XDG_RUNTIME_DIR"
printf 'padding = 0\n[launch]\nsplash = "off"\n[window]\nactivity_lights = "off"\n[link]\npaths = "off"\n[font]\ndpi = 72\n' > baud.toml
# Outputs as sway.conf lines, one per output, each with a black
# background; the window floats at the layout's origin, 800x608 points.
# Each sway gets a runtime directory of its own.
run=0
start_sway() {
  run=$((run + 1))
  export XDG_RUNTIME_DIR=$D/xdg$run && mkdir -m 0700 "$XDG_RUNTIME_DIR"
  # sway binds its IPC socket where SWAYSOCK says, when it is set: the
  # last sway's would be this one's.
  unset SWAYSOCK
  {
    for o in "$@"; do echo "output $o bg #000000 solid_color"; done
    echo 'default_border none'
    echo 'default_floating_border none'
    echo 'for_window [app_id="org.baud.terminal"] floating enable, resize set 800 608, move absolute position 0 0'
  } > sway.conf
  WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=$# WLR_RENDERER=pixman \
    WLR_LIBINPUT_NO_DEVICES=1 WAYLAND_DEBUG=server sway -c sway.conf > sway.log 2>&1 &
  SWAY=$!
  for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
  export WAYLAND_DISPLAY=wayland-1
  for _ in $(seq 100); do ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock > /dev/null 2>&1 && break; sleep 0.1; done
  export SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock)
  sleep 1
}
# The app, its child waiting for the grid before it writes the stream,
# then leaving the log's name with .fed for .log.
start_baud() { # stream, rows, cols, log
  BAUD_LOG=debug BAUD_CONFIG=baud.toml \
    BAUD_CMD="until [ \"\$(stty size)\" = '$2 $3' ]; do sleep 0.05; done; stty raw -echo; cat $1; : >'${4%.log}.fed'; exec sleep 600" \
    /target/release/baud > "$4" 2>&1 &
  BAUD=$!
}
# What the app attached to its window and told of the scale, from the
# log so far: the last buffer size, buffer scale, destination and the
# scales sway sent.
told() {
  /target/release/examples/linux-pointer-trace < sway.log | grep '^surface window' > told.all
  for k in attach buffer_scale destination preferred_buffer_scale fractional_scale; do
    grep "^surface window $k " told.all | tail -1
  done
}
stop() {
  kill $BAUD; wait $BAUD 2>/dev/null
  kill $SWAY; wait $SWAY 2>/dev/null
}
if [ "$ARM" = trixie ]; then
  mkdir -p $O/shot
  for s in /io/streams/*.bin; do
    n=$(basename "$s" .bin)
    [ "$(cat /io/streams/$n.comp)" = sway ] || continue
    sc=$(cat /io/streams/$n.scale) ws=$(cat /io/streams/$n.ws)
    start_sway "HEADLESS-1 resolution $(awk "BEGIN { print 800 * $sc }")x$(awk "BEGIN { print 608 * $sc }") position 0 0 scale $sc"
    start_baud "$s" "${ws#*x}" "${ws%x*}" $O/shot/$n.log
    sleep 4
    grim -o HEADLESS-1 $O/shot/$n.png
    sleep 1
    grim -o HEADLESS-1 $O/shot/$n.again.png
    told > $O/shot/$n.told
    stop
  done
fi
# The moves: less-pager's grid, 100x35 at every scale here, so a move
# changes the scale and not the grid. Each step waits two seconds for the
# frame at the new scale, then takes the outputs the window is on; the
# frame is compared only where the window is on one output.
if [ "$ARM" = trixie ]; then S2=1.5; else S2=2; fi
M=$O/moves && mkdir -p $M
start_sway "HEADLESS-1 resolution 800x608 position 0 0 scale 1" \
  "HEADLESS-2 resolution $(awk "BEGIN { print 800 * $S2 }")x$(awk "BEGIN { print 608 * $S2 }") position 800 0 scale $S2"
swaymsg -t get_outputs | grep -E '"(name|scale)"' | tr -d ' ,' | paste - - > $M/outputs.txt
start_baud /io/streams/less-pager.bin 35 100 $M/baud.log
sleep 4
step() { # name, the absolute position or "here", the outputs to take
  local name=$1 at=$2
  shift 2
  if [ "$at" != here ]; then
    swaymsg "[app_id=\"org.baud.terminal\"] move absolute position $at" > /dev/null
    sleep 2
  fi
  swaymsg -t get_tree | /target/release/examples/linux-pixels window-rect > $M/$name.rect
  for o in "$@"; do grim -o $o $M/$name-$o.png; done
  told > $M/$name.told
}
step first here HEADLESS-1
step across-from-1 "300 0" HEADLESS-1 HEADLESS-2
step moved "800 0" HEADLESS-2
step across-from-2 "300 0" HEADLESS-1 HEADLESS-2
step back "0 0" HEADLESS-1
/target/release/examples/linux-pointer-trace < sway.log | grep '^surface window' > $M/told.all
stop
STAGE
)
for arm in bookworm trixie; do
  mkdir -p "$out/$arm"
  tar -C "$out" -c streams faces |
    docker run --rm -i -e ARM="$arm" -e STAGE="$stage" -v "$root":/src:ro \
      -v "$image-target":/target:ro "$image-$arm" bash -c '
set -eu
exec 3>&1 1>&2
mkdir -p /io && tar -C /io -x
# The faces the bench read, in place of this system'"'"'s own.
find /usr/share/fonts/truetype/dejavu -name "*.ttf" -delete
cp /io/faces/system/*.ttf /usr/share/fonts/truetype/dejavu/
mkdir -p /io/out && chown baud /io/out
printf "%s\n" "$STAGE" > /tmp/stage.sh
runuser -u baud -- env ARM="$ARM" bash /tmp/stage.sh
tar -C /io/out -c . >&3' 2>"$out/$arm/container.log" |
    tar -C "$out/$arm" -x
done
cp "$out"/trixie/shot/* "$out/shot/"

# The Mac arm, on the same faces: the system face the container's stack
# found is read from the copy it sent out.
echo "$fixtures" | while read -r name rec ws scale comp; do
  [ -n "$name" ] || continue
  PIXELS_WS=$ws PIXELS_SCALE=$scale PIXELS_FACES="$out/faces/DejaVuSansMono.ttf" \
    cargo bench -q -p baud-bench --bench pixels -- \
    "$out/streams/$name.bin" "$out/mac/$name.pam"
done

# The comparisons, by `linux-pixels-compare.sh`, which reads the fixtures
# from the folder.
echo "$fixtures" >"$out/fixtures.txt"
exec "$root/tools/scripts/linux-pixels-compare.sh" "$out"
