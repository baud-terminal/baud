#!/usr/bin/env bash
# Runs the Linux app on two headless compositors and reads how its frames
# reach the screen: the presentation feedback the compositor sends it, and
# what each of its commits carries for the pacing globals the compositor
# offers -- fifo-v1's barrier, commit-timing-v1's timestamp and
# tearing-control-v1's hint.
#
#   scripts/linux-frames.sh <outdir>
#
# BAUD_FRAMES_TAG names the images and volumes it makes (by default
# baud-linux-frames), so two runs on one engine keep apart.
#
# Needs docker with an aarch64 Linux engine (colima's default VM on an
# Apple silicon Mac), and the repository under a directory the engine
# mounts. Nothing opens a window here: the compositor is headless, inside
# the container.
#
# The app is built once, in Debian bookworm, and run under:
#
#   weston   weston 16.0.0, Debian sid as the snapshot archive held it on
#            2026-10-30: wp_presentation 2, fifo-v1, commit-timing-v1 and
#            tearing-control-v1, all four
#   sway     sway 1.10.1, Debian trixie: wp_presentation 1 and
#            tearing-control-v1, neither fifo-v1 nor commit-timing-v1; its
#            headless backend says no refresh (zero), so the app plans
#            each frame for when it is drawn, as without the feedback
#
# Each runs two sessions, `[window] tearing` off and on, each three
# seconds of the launch splash and a program printing a line every 30 ms
# for a second and a half. The compositor runs with WAYLAND_DEBUG=server,
# which logs each request the app sends it and each event it sends the
# app, and baud-bench's linux-frames example reads that log. Every check prints
# "ok" or "FAILED" with what was read; the script exits non-zero when one
# failed or a step could not run.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:?usage: scripts/linux-frames.sh <outdir>}
mkdir -p "$out"
out=$(cd "$out" && pwd)

toolchain=$(rustc -Vv | sed -n 's/^commit-hash: //p')
nightly=${BAUD_NIGHTLY:-nightly-2026-04-11}
tag=${BAUD_FRAMES_TAG:-baud-linux-frames}
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
# sid moves daily, so its packages come from the snapshot archive at a
# fixed moment and its base image by digest.
docker build -q -t "$tag-weston" - >/dev/null <<'EOF'
FROM debian:sid@sha256:46e441d513e81eb6c04119061b4abe68a5bae6fa55fd42bf39147e7d2a548bf5
RUN rm -f /etc/apt/sources.list.d/* \
    && echo 'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/20261030T000000Z sid main' \
       > /etc/apt/sources.list \
    && apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
       --no-install-recommends weston wayland-utils \
    && rm -rf /var/lib/apt/lists/* && useradd -m baud
EOF
docker build -q -t "$tag-sway" - >/dev/null <<'EOF'
FROM debian:trixie
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
    --no-install-recommends sway wayland-utils \
    && rm -rf /var/lib/apt/lists/* && useradd -m baud
EOF
docker volume create "$tag-target" >/dev/null
docker volume create "$tag-registry" >/dev/null
docker run --rm -v "$root":/src:ro \
  -v "$tag-target":/target \
  -v "$tag-registry":/usr/local/cargo/registry \
  "$tag-build" bash -c \
  'cd /src && export CARGO_TARGET_DIR=/target RUSTFLAGS="-D warnings"
   cargo build -q --release -p baud --bin baud
   cargo build -q --release -p baud-bench --example linux-frames'

# One arm, as the user baud (sway will not run as root). Results go to
# /tmp/out, which leaves the container as tar on stdout.
inner=$(
  cat <<'INNER'
set -u
mkdir -p /tmp/out && D=$(mktemp -d) && cd "$D"
O=/tmp/out
export XDG_RUNTIME_DIR=$D/xdg && mkdir -m 0700 "$XDG_RUNTIME_DIR"
case $ARM in
  weston)
    # kiosk-shell starts no client of its own, so the app is the one
    # client the log holds once wayland-info has gone.
    WAYLAND_DEBUG=server weston --backend=headless --renderer=pixman \
      --shell=kiosk --width=1024 --height=768 --socket=wayland-1 > compositor.log 2>&1 &
    ;;
  sway)
    printf 'output HEADLESS-1 resolution 1024x768 position 0 0\ndefault_border none\n' > sway.conf
    WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
      WAYLAND_DEBUG=server sway -c sway.conf > compositor.log 2>&1 &
    ;;
esac
COMPOSITOR=$!
for _ in $(seq 100); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.1; done
export WAYLAND_DISPLAY=wayland-1
{
  case $ARM in weston) weston --version ;; sway) sway -v ;; esac
  wayland-info | grep -E "'(wp_presentation|wp_fifo_manager_v1|wp_commit_timing_manager_v1|wp_tearing_control_manager_v1)'" |
    sed 's/, name: *[0-9]*//; s/  */ /g'
} > $O/compositor.txt
cat $O/compositor.txt
FAILED=0
session() { # name, tearing
  S=$D/$1 && mkdir -p "$S"
  printf '[window]\ntearing = "%s"\n' "$2" > "$S/baud.toml"
  printf 'for i in $(seq 1 50); do printf "line %%d\\r\\n" $i; sleep 0.03; done\nexec sleep 30\n' > "$S/child.sh"
  from=$(wc -l < compositor.log)
  BAUD_LOG=debug BAUD_CONFIG=$S/baud.toml BAUD_CMD="bash $S/child.sh" \
    /target/release/baud > "$S/baud.log" 2>&1 &
  BAUD=$!
  sleep 3
  kill $BAUD; wait $BAUD 2>/dev/null
  tail -n +$((from + 1)) compositor.log > "$S/traffic.log"
  /target/release/examples/linux-frames "$S/traffic.log" "$O/compositor.txt" "$2" > "$O/frames-$1.txt" || FAILED=1
  echo "== $1, tearing $2"
  cat "$O/frames-$1.txt"
  cp "$S/baud.log" "$O/baud-$1.log"
  cp "$S/traffic.log" "$O/traffic-$1.log"
}
session paced off
session torn on
kill $COMPOSITOR; wait $COMPOSITOR 2>/dev/null
exit $FAILED
INNER
)

status=0
for arm in weston sway; do
  mkdir -p "$out/$arm"
  echo "== $arm"
  docker run --rm -i -e ARM="$arm" -v "$root":/src:ro -v "$tag-target":/target:ro \
    "$tag-$arm" bash -c \
    'cat > /tmp/arm.sh && chmod 755 /tmp/arm.sh && runuser -u baud -- env ARM="$ARM" bash /tmp/arm.sh > /tmp/results.txt 2>&1; s=$?; mkdir -p /tmp/out; cp /tmp/results.txt /tmp/out/; tar -C /tmp/out -c .; exit $s' \
    <<<"$inner" 2>"$out/$arm/container.log" | tar -C "$out/$arm" -x || status=1
  cat "$out/$arm/results.txt"
done
exit $status
