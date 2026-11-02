#!/usr/bin/env bash
# Draws the same frames on this Mac and on Linux and compares them pixel
# for pixel, with the Linux app's frames taken from a real compositor.
#
#   scripts/linux-pixels.sh <outdir>
#
# Needs docker with an aarch64 Linux engine (colima's default VM on an
# Apple silicon Mac), and the repository under a directory the engine
# mounts (colima mounts the home directory). Nothing opens a window here:
# the compositors are headless, inside the container. The research note
# docs/research/notes/be-linux-pixels.md holds a run's results.
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
#                   the frame has settled
#
# Each fixture is drawn at a scale of one, and three again at two: weston's
# output at scale 2, which the app reads from wl_output.scale and draws at,
# its buffer twice the window's points with set_buffer_scale 2, and the
# benches at PIXELS_SCALE=2, the 13-point font at 26 pixels. The window is
# 800x600 points at both, so its grid is 100x35 at both: 8x17 cells and
# 16x34.
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
# Every result is printed and kept under <outdir>; the script exits 0
# whatever the pixels say, and non-zero only when a step could not run.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:?usage: scripts/linux-pixels.sh <outdir>}
mkdir -p "$out"/streams "$out"/mac "$out"/linux "$out"/shot "$out"/diff
out=$(cd "$out" && pwd)
# BAUD_PIXELS_TAG names the image and volumes it makes (by default
# baud-linux-pixels), so two runs on one engine keep apart.
image=${BAUD_PIXELS_TAG:-baud-linux-pixels}
py="python3 $root/scripts/linux-pixels.py"

# name, recording under tests/recordings (or "control"), grid, scale
fixtures="
less-pager ascii/less-pager.raw 100x35 1
cat-cjk utf8/cat-cjk.raw 100x35 1
vim-unicode utf8/vim-unicode.raw 100x35 1
btop utf8/btop.raw 100x35 1
swirl sixel/swirl.raw 100x35 1
control control 100x35 1
less-pager-x2 ascii/less-pager.raw 100x35 2
cat-cjk-x2 utf8/cat-cjk.raw 100x35 2
control-x2 control 100x35 2
"

# The control: the page cleared to rgb(185, 37, 105), which the app
# dims as it dims every page (`[glass] app_backgrounds = "dimmed"`); each
# channel's premultiplied value is a rounding up, so a truncation would
# read one less on all three.
control=$'\033[48;2;185;37;105m\033[2J\033[?25l'

echo "$fixtures" | while read -r name rec ws scale; do
  [ -n "$name" ] || continue
  echo "$scale" >"$out/streams/$name.scale"
  if [ "$rec" = control ]; then
    printf '%s' "$control" >"$out/streams/$name.bin"
  else
    {
      cat "$root/tests/recordings/$rec"
      printf '\033[?25l'
    } >"$out/streams/$name.bin"
  fi
  echo "$ws" >"$out/streams/$name.ws"
done

toolchain=$(rustc -Vv | sed -n 's/^commit-hash: //p')

# The image: weston 10.0.1 and wl-clipboard from Debian bookworm, and the
# nightly whose commit is this Mac's, checked below.
nightly=${BAUD_NIGHTLY:-nightly-2026-04-07}
docker build -q -t "$image" - >/dev/null <<EOF
FROM rustlang/rust:nightly-bookworm
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
    --no-install-recommends weston sway wtype wl-clipboard python3 \
    && rm -rf /var/lib/apt/lists/* && useradd -m baud
RUN rustup toolchain install $nightly --profile minimal \
    && rustup default $nightly
EOF
there=$(docker run --rm "$image" rustc -Vv | sed -n 's/^commit-hash: //p')
if [ "$there" != "$toolchain" ]; then
  echo "the container's rustc is $there, this Mac's $toolchain; set BAUD_NIGHTLY" >&2
  exit 1
fi

docker volume create "$image-target" >/dev/null
docker volume create "$image-registry" >/dev/null

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
mkdir -p /io/linux /io/shot /io/clip /io/faces
cd /src
export CARGO_TARGET_DIR=/target RUSTFLAGS="-D warnings"
cargo build -q --release -p baud --bin baud
cargo bench -q -p baud-bench --bench pixels --no-run
# The system face the default stack finds here, which the bench is told
# of and which goes out for the Mac arm to read.
mono=/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf
cp "$mono" /io/faces/
export XDG_RUNTIME_DIR=/tmp/xdg && mkdir -p -m 0700 $XDG_RUNTIME_DIR
printf '[launch]\nsplash = "off"\n' > /tmp/baud.toml
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
  start_weston 800 600 "$sc"
  # The child waits for the window's grid, so no byte is drawn at the
  # 80x24 the pty is spawned at and then reflowed; then raw, so the tty
  # passes the bytes as they are: no LF made CR LF, no answer to a query
  # echoed onto the screen.
  BAUD_LOG=debug BAUD_CONFIG=/tmp/baud.toml \
    BAUD_CMD="until [ \"\$(stty size)\" = '35 100' ]; do sleep 0.05; done; stty raw -echo; cat $s; exec sleep 600" \
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
printf '[launch]\nsplash = "off"\n' > baud.toml
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

# The Mac arm, on the same faces: the system face the container's stack
# found is read from the copy it sent out.
echo "$fixtures" | while read -r name rec ws scale; do
  [ -n "$name" ] || continue
  PIXELS_WS=$ws PIXELS_SCALE=$scale PIXELS_FACES="$out/faces/DejaVuSansMono.ttf" \
    cargo bench -q -p baud-bench --bench pixels -- \
    "$out/streams/$name.bin" "$out/mac/$name.pam"
done

# The comparisons.
echo "$fixtures" | while read -r name rec ws scale; do
  [ -n "$name" ] || continue
  echo "== $name ($ws, scale $scale)"
  printf 'mac against linux, the bench: '
  if cmp -s "$out/mac/$name.pam" "$out/linux/$name.pam"; then
    echo identical
  else
    $py diff "$out/mac/$name.pam" "$out/linux/$name.pam" "$out/diff/$name-os.png" || true
  fi
  printf 'the screenshot settled: '
  $py diff "$out/shot/$name.png" "$out/shot/$name.again.png" >/dev/null &&
    echo yes || echo "no, it changed in the second after"
  $py over-black "$out/linux/$name.pam" "$out/linux/$name.over.pam"
  w=$(sed -n 's/^WIDTH //p' "$out/linux/$name.pam" | head -1)
  h=$(sed -n 's/^HEIGHT //p' "$out/linux/$name.pam" | head -1)
  $py crop "$out/shot/$name.png" 0 0 "$w" "$h" "$out/shot/$name.grid.pam"
  printf 'linux over black against the screenshot: '
  $py diff "$out/linux/$name.over.pam" "$out/shot/$name.grid.pam" "$out/diff/$name-app.png" || true
done
# The control's one value, computed here and not read from either arm:
# rgb(185, 37, 105) at the dark dim's alpha, round(0.70 * 255) = 179,
# premultiplied over black and rounded.
for c in control control-x2; do
  printf 'the %s holds one value, (130, 26, 74), everywhere: ' "$c"
  python3 - "$out/shot/$c.grid.pam" "$root/scripts/linux-pixels.py" <<'EOF'
import importlib.util, sys
spec = importlib.util.spec_from_file_location("lp", sys.argv[2])
lp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lp)
w, h, p = lp.read(sys.argv[1])
want = bytes([(c * 179 + 127) // 255 for c in (185, 37, 105)] + [255])
truncated = bytes([c * 179 // 255 for c in (185, 37, 105)] + [255])
assert want == bytes([130, 26, 74, 255]) and truncated == bytes([129, 25, 73, 255])
print("yes" if p == bytearray(want * (w * h)) else "no")
EOF
done
echo "== clipboard"
cat "$out/clip/result.txt"
