#!/bin/sh
# Records the trail clips and stills in docs/cursor/, which docs/cursor.md and
# docs/cursor.html show. macOS only: it records baud's window through
# ScreenCaptureKit, so the terminal running it needs Screen Recording, and it
# raises the window through System Events, which needs Accessibility.
#
#   cargo build --release -p baud-app && scripts/trail-clips.sh
#
# For each trail it launches target/release/baud at an 80x24 grid with a
# config of its own -- the built-in fonts at 16 pt, a steady block cursor and
# that trail, nothing read from ~/.config -- runs scripts/trail-demo.sh in it,
# raises the window and records it with scripts/trail-record.swift. Then
# scripts/trail-still.swift takes the still from the diagonal, the fifth move.
# The theme follows the system's appearance, so run it the way the committed
# clips were taken (Dark) or say which in the docs.
#
# It checks what it made rather than trusting it:
#   - baud read the generated config without a complaint, so a clip labelled
#     "quad" cannot silently be the default fire;
#   - the recorder refuses a window that was not frontmost, or a clip under
#     its frame floor;
#   - the still found at least TRAIL_FLOOR changed samples for every trail,
#     and fewer for `off`, recorded as the control and not kept.
#
# The diagonal lands about 5.7 s into a clip: GO 0.4 s after the capture
# starts, the child's 0.6 s, four moves 1.15 s apart and a little latency.
# The still searches 5.5 s to 6.75 s: the fourth move's fire is out by about
# 5.45 s, the diagonal's by about 6.6, and the sixth move lands about 6.85.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
baud=$root/target/release/baud
out=$root/docs/cursor
# TRAILS names a subset to record, e.g. TRAILS=plasma; the control always runs.
trails=${TRAILS:-"fire plasma lightning ice ufo orb quad sweep ghost"}
TRAIL_FLOOR=20
X=/Applications/Xcode.app/Contents/Developer

[ -x "$baud" ] || {
  echo "no $baud: cargo build --release -p baud-app" >&2
  exit 1
}
tmp=$(mktemp -d)
pid=
cleanup() {
  [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  rm -rf "$tmp"
}
trap cleanup EXIT

for tool in record still; do
  DEVELOPER_DIR=$X env -u SDKROOT $X/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
    -O -suppress-warnings -sdk $X/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
    -o "$tmp/trail-$tool" "$root/scripts/trail-$tool.swift"
done
mkdir -p "$out"

# clip <style> <dir> <min-frames>: record one trail into <dir>/<style>.mp4
# and .png, and leave the still's changed-sample count in $n. Not run in a subshell, so the
# trap sees the pid and a failure part way cannot leave baud running.
clip() {
  style=$1 dir=$2 frames=$3
  cfg=$tmp/$style.toml log=$tmp/$style.log go=$tmp/go-$style done=$tmp/done-$style
  printf 'font_size = 16\n\n[cursor]\nshape = "block"\nblink = false\ntrail = "%s"\n' "$style" >"$cfg"
  for _ in 1 2 3; do
    rm -f "$go" "$done"
    BAUD_LOG=info BAUD_CONFIG=$cfg BAUD_GRID=80x24 \
      BAUD_CMD="$root/scripts/trail-demo.sh $style $go $done" "$baud" >"$log" 2>&1 &
    pid=$!
    sleep 1.2
    grep -qx "config: $cfg" "$log" || {
      echo "$style: baud did not read $cfg" >&2
      exit 1
    }
    if grep '^config: ' "$log" | grep -v "^config: $cfg\$" >&2; then
      echo "$style: baud complained about its config" >&2
      exit 1
    fi
    osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true" >/dev/null
    rc=0
    "$tmp/trail-record" "$pid" "$dir/$style.mp4" "$go" "$done" 1.3 "$frames" || rc=$?
    kill "$pid"
    wait "$pid" 2>/dev/null || true
    pid=
    # 3 is another window in front; anything else is not worth a retry.
    [ "$rc" -eq 3 ] || break
  done
  [ "$rc" -eq 0 ] || {
    echo "$style: the recorder refused ($rc)" >&2
    exit 1
  }
  still=$("$tmp/trail-still" "$dir/$style.mp4" "$dir/$style.png" 5.5 6.75 0.5)
  echo "$style: $still"
  n=${still##* }
}

for style in $trails; do
  clip "$style" "$out" 30
  [ "$n" -ge "$TRAIL_FLOOR" ] || {
    echo "$style: the still found no trail ($n)" >&2
    exit 1
  }
done
clip off "$tmp" 1
[ "$n" -lt "$TRAIL_FLOOR" ] || {
  echo "off: the control found a trail ($n)" >&2
  exit 1
}
ls -l "$out"
