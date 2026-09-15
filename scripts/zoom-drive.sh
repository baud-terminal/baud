#!/usr/bin/env bash
# Drive the View menu's zoom and read the present counters back.
#
# Two things had to be got right before this could fire at all, and each
# cost a wrong conclusion first:
#
#  * **The menu has to be opened before its item can be clicked.** Clicking
#    the item alone returns a menu-item reference and looks like success
#    while doing nothing, and `enabled` read without opening reports
#    `false` for an item that is fine.
#  * **Frontmost has to be re-asserted every round.** Something takes focus
#    back immediately -- Safari, on this desktop -- and every click after
#    the first goes elsewhere.
#
# Without those, the same commit returned 3, 2, 1 and then 0, 0 relayouts
# from identical runs, and a bisect against it blamed the code.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT=${1:-/tmp/zoom.log}
N=${N:-8}

rm -f "$OUT"
BAUD_DEBUG_RESIZE=1 \
  BAUD_CMD='while :; do printf "\033[H"; seq 1 40; sleep 0.03; done' \
  "$ROOT/target/release/baud" >"$OUT" 2>&1 &
PID=$!
sleep 4

click() {
  osascript -e 'tell application "System Events" to set frontmost of process "baud" to true' >/dev/null 2>&1
  sleep 0.3
  osascript -e 'tell application "System Events" to tell process "baud" to click menu bar item "View" of menu bar 1' >/dev/null 2>&1
  sleep 0.3
  osascript -e "tell application \"System Events\" to tell process \"baud\" to click menu item \"$1\" of menu 1 of menu bar item \"View\" of menu bar 1" >/dev/null 2>&1
  sleep 0.4
}

front=$(osascript -e 'tell application "System Events" to set frontmost of process "baud" to true' 2>&1)
if [ -n "$front" ]; then
  echo "REFUSED: cannot bring baud frontmost -- $front"
  kill $PID 2>/dev/null
  wait $PID 2>/dev/null
  exit 2
fi

for _ in $(seq 1 "$N"); do click Bigger; done
for _ in $(seq 1 "$N"); do click Smaller; done

sleep 4
kill $PID 2>/dev/null
wait $PID 2>/dev/null

# `zoom:` lines exist only on a build that carries the instrument, so the
# control is the relayout count: a click that lands re-lays the grid.
lays=$(grep -c '^resize ' "$OUT" 2>/dev/null)
echo "clicks: $((N * 2)), relayouts: $lays, zooms instrumented: $(grep -c '^zoom: ' "$OUT" 2>/dev/null)"
if [ "$lays" -lt "$((N * 2 - 2))" ]; then
  echo "WARNING: clicks are not landing; nothing below is a measurement"
fi
grep -o 'busy Some([A-Za-z]*)' "$OUT" | sort | uniq -c
grep -o "present: .*" "$OUT" | tail -1
