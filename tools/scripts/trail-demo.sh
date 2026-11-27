#!/bin/sh
# The child the trail clips are recorded on (tools/scripts/trail-clips.sh runs it
# as BAUD_CMD). It draws a header and four `+` marks, waits for the
# recorder's GO file, makes six long cursor moves 1.15 s apart -- right, down,
# left, up, then the two diagonals -- touches DONE and idles.
#
# 1.15 s is longer than any trail lasts on these moves, so each move starts
# from rest and the diagonal the stills are taken from (the fifth move) has
# the screen to itself.
#
# usage: trail-demo.sh <style> <go-file> <done-file>
style="$1"
go="$2"
done="$3"
e=$(printf '\033')
printf '%s[?25h%s[2J%s[1;3H%s[2mbaud  [cursor] trail = %s[0m%s[1m"%s"%s[0m' \
  "$e" "$e" "$e" "$e" "$e" "$e" "$style" "$e"
mark() { printf '%s[%s;%sH%s[2m+%s[0m' "$e" "$1" "$2" "$e" "$e"; }
mark 6 8
mark 6 72
mark 20 72
mark 20 8
printf '%s[6;8H' "$e"
while [ ! -e "$go" ]; do sleep 0.02; done
sleep 0.6
for p in "6;72" "20;72" "20;8" "6;8" "20;72" "6;8"; do
  printf '%s[%sH' "$e" "$p"
  sleep 1.15
done
touch "$done"
exec sleep 600
