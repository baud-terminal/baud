#!/bin/sh
# The child the README's clip is filmed on (scripts/readme-film.sh runs it
# through the film tool, baud-app/src/film.rs): baud's launch -- the logo
# carried off by the plasma, the prompt under it -- then btop, a git log
# scrolling past, a look back into the history with the wheel and a
# selection there, a photo drawn in place by kitten icat, one long cursor
# jump for the trail, and back to the empty prompt it started on, so the
# clip loops. Every place on screen is taken from the grid the pty has.
#
# usage: readme-demo-film.sh [btop]
# With `btop` it shows btop alone and ends, for the stills that choose the
# window's size. The film script exports README_BTOP, README_KITTEN,
# README_PHOTO, README_REPO, README_REV and README_TMP (where btop's config
# lives); the film tool names the file a gesture is asked for in
# BAUD_FILM_CUES.
# shellcheck disable=SC1091 # read beside this file, not from the input
. "$(dirname "$0")/readme-demo-lib.sh"
quiet
# shellcheck disable=SC2046 # the two numbers `stty size` prints
set -- "${1-}" $(stty size)
only=$1 rows=$2 cols=$3

# cue WORDS: a gesture for the film tool to make with the pointer.
cue() { printf '%s\n' "$*" >>"$BAUD_FILM_CUES"; }

# A shell's start: the prompt a moment after the launch, under the splash.
sleep 0.2
prompt
sleep 1.0

type_cmd btop
# btop quits cleanly on SIGINT, restoring the screen it found. Its config
# and state are under README_TMP, so it neither reads nor writes the
# machine's.
(
  sleep 3.5
  kill -INT "$(cat "$README_TMP/btop.pid")"
) &
XDG_CONFIG_HOME=$README_TMP XDG_STATE_HOME=$README_TMP \
  sh -c 'echo $$ >"$1"; exec "$2" </dev/tty' sh "$README_TMP/btop.pid" "$README_BTOP"
wait
csi '?25h'
prompt
if [ "$only" = btop ]; then
  sleep 0.3
  cue 'done'
  exec sleep 600
fi

type_cmd 'git log --graph --oneline'
# A fixed range of a fixed revision, so every run films the same lines,
# paced so the scroll is seen line by line: a screen and a half of them.
git --no-pager -C "$README_REPO" log --graph --oneline --no-decorate --color=always \
  -n $((rows * 3 / 2)) "$README_REV" |
  while IFS= read -r line; do
    printf '%s\n' "$line"
    sleep 0.03
  done
prompt
sleep 0.5

# Back into the history with the wheel, the overlay scrollbar showing; a
# selection across three lines there; a click that clears it; and the
# wheel back down, the scrollbar fading after it.
mid=$((rows / 2))
cue wheel "$mid" $((cols / 2)) -8
sleep 0.9
cue drag $((mid - 3)) 6 "$mid" $((cols / 2))
sleep 1.6
cue click $((mid + 2)) $((cols - 12))
sleep 0.4
cue wheel "$mid" $((cols / 2)) 8
sleep 1.0

clear_all
prompt
# The photo in five eighths of the width, at row 3, column 3, in a box as
# many rows high as a third of its columns: a cell about half as wide as
# it is high, and the photo 3:2, so the prompt below it follows it
# closely. --transfer-mode=stream: kitten's own detection times out in
# baud (scripts/readme-capture.sh says how), and stream needs no file on
# this machine.
pw=$((cols * 5 / 8))
ph=$((pw / 3))
type_cmd "kitten icat --transfer-mode=stream --place ${pw}x${ph}@2x2 canazei.jpg"
"$README_KITTEN" icat --transfer-mode=stream --place "${pw}x${ph}@2x2" "$README_PHOTO" </dev/tty
# The picture is on screen a moment after kitten returns.
sleep 0.3
csi "$((ph + 4));1H"
prompt
sleep 1.3

# The trail: top left to bottom right, the longest move the screen holds.
csi '1;1H'
sleep 0.4
csi "${rows};${cols}H"
sleep 0.9
clear_all
prompt
sleep 0.7
cue 'done'
exec sleep 600
