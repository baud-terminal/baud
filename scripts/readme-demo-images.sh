#!/bin/sh
# The child the README's images clip is filmed on (scripts/readme-capture.sh
# runs it as BAUD_CMD): kitten icat draws a photo, then a smaller one beside
# a few lines of text, and the screen goes back to the empty prompt it
# started on, so the clip loops. kitten is the real client, pinned with the
# photos by scripts/readme-capture.sh.
#
# usage: readme-demo-images.sh <go-file> <done-file>
# The capture script exports README_KITTEN, README_PHOTO and README_PHOTO2.
# shellcheck disable=SC1091 # read beside this file, not from the input
. "$(dirname "$0")/readme-demo-lib.sh"
go=$1
done=$2
quiet
clear_all
prompt
wait_for "$go"
sleep 0.2

# --transfer-mode=stream: kitten's detection timed out on baud's earlier
# Primary DA (scripts/readme-capture.sh says how).
type_cmd 'kitten icat --transfer-mode=stream --place 56x18@2x2 canazei.jpg'
"$README_KITTEN" icat --transfer-mode=stream --place 56x18@2x2 "$README_PHOTO" </dev/tty
csi '21;1H'
prompt
sleep 1.2

clear_all
prompt
type_cmd 'cat notes.md'
csi 1m
printf '# Day three'
csi 0m
printf '\n\n'
printf 'The pier was empty by eight,\n'
printf 'and the lake went flat as glass.\n'
printf 'It held the last of the light\n'
printf 'for nearly an hour after the\n'
printf 'sun was down behind the ridge.\n'
prompt
sleep 0.3
# Beside the notes, rows 2 to 8, so the command typed below them is not
# drawn over.
type_cmd 'kitten icat --transfer-mode=stream --place 24x7@46x1 pier.jpg'
"$README_KITTEN" icat --transfer-mode=stream --place 24x7@46x1 "$README_PHOTO2" </dev/tty
sleep 0.3
prompt
sleep 1.5

clear_all
prompt
sleep 0.3
touch "$done"
exec sleep 600
