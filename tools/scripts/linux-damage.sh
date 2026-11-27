#!/usr/bin/env bash
# Shows frames on a real compositor through the Linux driver's window and
# measures what row damage changes: the time Window::present takes, the
# rows it copies into the buffer and damages, the compositor's CPU time,
# and the screen the compositor then shows, frame by frame.
#
#   tools/scripts/linux-damage.sh <outdir> [<patch>]
#
# A patch, applied to the tree before it is built, makes a perturbation
# arm: the same runs on code made wrong on purpose, to show the checks
# can fail. BAUD_DAMAGE_TAG names the images and volumes it makes (by
# default baud-linux-damage). BAUD_DAMAGE_ROUNDS (5) and
# BAUD_DAMAGE_FRAMES (300) size the timed runs, BAUD_DAMAGE_SHOTS (30)
# the frames a workload is checked against the screen on.
#
# Needs docker with an aarch64 Linux engine (colima's default VM on an
# Apple silicon Mac), and the repository under a directory the engine
# mounts. Nothing opens a window here: the compositor is headless, inside
# the container.
#
# The `present` bench (crates/baud-bench/benches/present.rs) is built once, in
# Debian bookworm, and run under two sways on wlroots' headless backend
# with the pixman renderer: bookworm's sway 1.7 and trixie's 1.10.1. The
# output is 800x600 and the bench's window, tiled alone with no border,
# fills it; its grid is 100x35 cells of 8x17 px, 800x595.
#
# Each sway runs the bench twice a round, BAUD_DAMAGE_ROUNDS rounds, the
# arms in turn and the order swapped each round: `whole`, every frame
# told it changed everything, as the driver did before it sent its rows,
# and `rows`, the bands the renderer painted. Then, under a second sway
# that logs each request and event (WAYLAND_DEBUG=server), each arm twice
# more with grim taking the screen after each of the first
# BAUD_DAMAGE_SHOTS frames: once with the page opaque, where the screen
# must be the frame itself, and once at the default dim, where the two
# arms' screens must hash the same. The log's frame traffic is kept,
# without timestamps, as trace-<page>-<arm>.txt.
# baud-bench's linux-damage example, built by this Mac's cargo, reads it all
# and prints the summary; the script exits non-zero when a screen check
# fails or a step could not run.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
out=${1:?usage: tools/scripts/linux-damage.sh <outdir> [<patch>]}
patch=${2:-}
mkdir -p "$out"
out=$(cd "$out" && pwd)
label=tree
if [ -n "$patch" ]; then
  label=$(basename "$patch" .patch)
fi

toolchain=$(rustc -Vv | sed -n 's/^commit-hash: //p')
nightly=${BAUD_NIGHTLY:-nightly-2026-11-06}
tag=${BAUD_DAMAGE_TAG:-baud-linux-damage}
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
docker volume inspect "$tag-target" >/dev/null 2>&1 || docker volume create "$tag-target" >/dev/null
docker volume inspect "$tag-registry" >/dev/null 2>&1 || docker volume create "$tag-registry" >/dev/null
# The tree, copied so a patch can be applied, and built into a target of
# the arm's own, so no arm's build is another's left in place. The patch
# comes in on stdin.
{
  [ -n "$patch" ] && cat "$patch"
  true
} |
  docker run --rm -i -v "$root":/src:ro \
    -v "$tag-target":/target \
    -v "$tag-registry":/usr/local/cargo/registry \
    -e LABEL="$label" "$tag-build" bash -c '
set -euo pipefail
cat > /tmp/patch
rm -rf /build && mkdir /build
tar -C /src --exclude=./target -c . | tar -C /build -x
if [ -s /tmp/patch ]; then patch -d /build -p1 < /tmp/patch; fi
cd /build
export CARGO_TARGET_DIR=/target/$LABEL RUSTFLAGS="-D warnings"
bin=$(cargo bench -q -p baud-bench --bench present --no-run --message-format=json 2>/dev/null |
  grep -o "\"executable\":\"[^\"]*present[^\"]*\"" | tail -1 | cut -d\" -f4)
cp "$bin" /target/$LABEL/present-bench
'

# One compositor, as the user baud (sway will not run as root). Results go
# to /tmp/out, which leaves the container as tar on stdout.
inner=$(
  cat <<'INNER'
set -u
mkdir -p /tmp/out && D=$(mktemp -d) && cd "$D"
O=/tmp/out
export XDG_RUNTIME_DIR=$D/xdg && mkdir -m 0700 "$XDG_RUNTIME_DIR"
cat > sway.conf <<C
output HEADLESS-1 resolution 800x600 position 0 0 scale 1
default_border none
C
start() { # the log's file, and WAYLAND_DEBUG's value
  rm -f "$XDG_RUNTIME_DIR"/wayland-1*
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
    WAYLAND_DEBUG=$2 sway -c sway.conf > "$1" 2>&1 &
  SWAY=$!
  for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
  export WAYLAND_DISPLAY=wayland-1
}
stop() { kill $SWAY; wait $SWAY 2>/dev/null; }
B=/target/$LABEL/present-bench
start sway.log 0
sway -v > $O/compositor.txt
for round in $(seq "$ROUNDS"); do
  order="whole rows"
  [ $((round % 2)) = 0 ] && order="rows whole"
  for arm in $order; do
    PRESENT_COMPOSITOR=$SWAY $B $arm "$FRAMES" > $O/time-$round-$arm.txt 2>&1
  done
done
stop
start traced.log server
for alpha in 255 dim; do
  for arm in whole rows; do
    from=$(wc -l < traced.log)
    if [ $alpha = dim ]; then
      PRESENT_SHOT="grim -t ppm -" $B $arm "$SHOTS" "$SHOTS" > $O/shot-$alpha-$arm.txt 2>&1
    else
      PRESENT_SHOT="grim -t ppm -" PRESENT_ALPHA=$alpha $B $arm "$SHOTS" "$SHOTS" > $O/shot-$alpha-$arm.txt 2>&1
    fi
    sleep 0.2
    # The frame traffic: each attach, damage, commit, release and frame
    # callback, without the time each was logged; libwayland 1.21 writes
    # an object's id after "@", 1.23 after "#".
    tail -n +$((from + 1)) traced.log |
      grep -E 'wl_surface[#@][0-9]+\.(attach|damage_buffer|commit)|wl_buffer[#@][0-9]+\.release|wl_callback[#@][0-9]+\.done' |
      sed 's/^\[[ 0-9.]*\] *//; s/@/#/g' > $O/trace-$alpha-$arm.txt
  done
done
stop
INNER
)
for arm in bookworm trixie; do
  mkdir -p "$out/$label/$arm"
  echo "== $label, $arm"
  docker run --rm -i -v "$root":/src:ro -v "$root":/build:ro \
    -v "$tag-target":/target:ro \
    -e LABEL="$label" -e ROUNDS="${BAUD_DAMAGE_ROUNDS:-5}" \
    -e FRAMES="${BAUD_DAMAGE_FRAMES:-300}" -e SHOTS="${BAUD_DAMAGE_SHOTS:-30}" \
    "$tag-$arm" bash -c \
    'cat > /tmp/arm.sh && runuser -u baud -- env LABEL="$LABEL" ROUNDS="$ROUNDS" FRAMES="$FRAMES" SHOTS="$SHOTS" bash /tmp/arm.sh >&2; tar -C /tmp/out -c .' \
    <<<"$inner" 2>"$out/$label/$arm/container.log" | tar -C "$out/$label/$arm" -x
done
cargo run -q --manifest-path "$root/Cargo.toml" -p baud-bench --example linux-damage -- "$out/$label"
