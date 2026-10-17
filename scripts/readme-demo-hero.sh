#!/bin/sh
# The child the README's hero clip is filmed on (scripts/readme-capture.sh
# runs it as BAUD_CMD): btop for three seconds, then quit; a git
# log scrolling past at a reader's pace; a photo drawn in place by kitten
# icat; one long cursor jump for the trail; and back to the empty prompt it
# started on, so the clip loops.
#
# usage: readme-demo-hero.sh <go-file> <done-file>
# The capture script exports README_BTOP, README_KITTEN, README_PHOTO,
# README_REPO, README_REV and README_TMP (where btop's config lives).
# shellcheck disable=SC1091 # read beside this file, not from the input
. "$(dirname "$0")/readme-demo-lib.sh"
go=$1
done=$2
quiet
clear_all
prompt
wait_for "$go"
sleep 0.3

type_cmd btop
# btop quits cleanly on SIGINT, restoring the screen it found. Its config
# and state are under README_TMP, so it neither reads nor writes the
# machine's.
(
  sleep 3
  kill -INT "$(cat "$README_TMP/btop.pid")"
) &
XDG_CONFIG_HOME=$README_TMP XDG_STATE_HOME=$README_TMP \
  sh -c 'echo $$ >"$1"; exec "$2" </dev/tty' sh "$README_TMP/btop.pid" "$README_BTOP"
wait
csi '?25h'
prompt

type_cmd 'git log --graph --oneline'
# A fixed range of a fixed revision, so every run films the same lines,
# paced so the smooth scroll is seen line by line.
git --no-pager -C "$README_REPO" log --graph --oneline --no-decorate --color=always -n 36 "$README_REV" |
  while IFS= read -r line; do
    printf '%s\n' "$line"
    sleep 0.03
  done
prompt
sleep 0.4

clear_all
prompt
# --transfer-mode=stream: kitten's own detection times out here (see
# scripts/readme-capture.sh), and stream needs no file on this machine.
type_cmd 'kitten icat --transfer-mode=stream --place 52x16@2x2 canazei.jpg'
"$README_KITTEN" icat --transfer-mode=stream --place 52x16@2x2 "$README_PHOTO" </dev/tty
# The picture is on screen a moment after kitten returns.
sleep 0.3
csi '19;1H'
prompt
sleep 1.3

# The trail: top left to bottom right, the longest move the screen holds.
csi '1;1H'
sleep 0.4
csi '24;80H'
sleep 0.9
clear_all
prompt
sleep 0.5
touch "$done"
exec sleep 600
