#!/bin/sh
# Replays the recordings through five terminals on this Mac at one
# geometry and one font, and records what each took.
#
# Every terminal runs `scripts/vt-child.sh`, which replays a recording
# `reps` times, waits for a DSR 6 reply -- the terminal answers only once
# it has consumed everything ahead of the query -- and appends wall time,
# the terminal process's CPU time and the geometry it got. Terminals are
# interleaved per round so drift lands on all of them; the reader discards
# the first round. A terminal that outlives its window is killed between
# runs, so each run is a fresh process.
#
# usage: vt-compare.sh <out-file> <rounds> <reps> <recording>...
# env:   ALACRITTY, KITTY, GHOSTTY, WEZTERM, BAUD -- the executables.
set -u
out="$1"
rounds="$2"
reps="$3"
shift 3
here=$(cd "$(dirname "$0")/.." && pwd)
child="$here/scripts/vt-child.sh"
: "${ALACRITTY:=alacritty}" "${KITTY:=kitty}" "${GHOSTTY:=ghostty}" "${WEZTERM:=wezterm}"
: "${BAUD:=$here/target/release/baud}"
: "${TERMINALS:=baud alacritty kitty ghostty wezterm}"
run_one() {
  term="$1"
  rec="$2"
  case "$term" in
  baud)
    BAUD_GPU=cells BAUD_GRID=200x50 \
      BAUD_CMD="exec bash $child baud $rec $reps $out" "$BAUD" 2>/dev/null
    ;;
  alacritty)
    "$ALACRITTY" -o window.dimensions.columns=200 -o window.dimensions.lines=50 \
      -o 'font.normal.family="Menlo"' -o font.size=13 \
      -e bash "$child" alacritty "$rec" "$reps" "$out" 2>/dev/null
    ;;
  kitty)
    "$KITTY" -o initial_window_width=200c -o initial_window_height=50c \
      -o remember_window_size=no -o font_family=Menlo -o font_size=13 \
      -o macos_quit_when_last_window_closed=yes \
      bash "$child" kitty "$rec" "$reps" "$out" 2>/dev/null
    pkill -f kitty-wrapped 2>/dev/null
    ;;
  ghostty)
    # The command goes in as configuration rather than through `-e`, which
    # Ghostty guards with a dialog asking the user to allow it.
    "$GHOSTTY" --window-width=200 --window-height=50 --font-family=Menlo --font-size=13 \
      --quit-after-last-window-closed=true --confirm-close-surface=false \
      --initial-command="bash $child ghostty $rec $reps $out" 2>/dev/null
    pkill -x ghostty 2>/dev/null
    ;;
  wezterm)
    "$WEZTERM" --config initial_cols=200 --config initial_rows=50 \
      --config 'font=wezterm.font("Menlo")' --config font_size=13 \
      --config quit_when_all_windows_are_closed=true \
      start --always-new-process -- bash "$child" wezterm "$rec" "$reps" "$out" 2>/dev/null
    pkill -x wezterm-gui 2>/dev/null
    ;;
  esac
}
r=0
while [ "$r" -lt "$rounds" ]; do
  for rec in "$@"; do
    for term in $TERMINALS; do
      echo "round $r $term $(basename "$rec")"
      run_one "$term" "$rec"
      sleep 1
    done
  done
  r=$((r + 1))
done
