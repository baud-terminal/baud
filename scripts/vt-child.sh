#!/bin/bash
# The child every terminal runs in the comparison: replays a recording
# `reps` times, waits for the terminal to acknowledge it has consumed the
# lot (a DSR 6 round trip -- the reply cannot come before the bytes ahead
# of it are parsed), and records the wall time, the terminal's CPU time,
# the geometry it actually got, and the reply itself.
#
# usage: vt-child.sh <name> <recording> <reps> <out-file>
name="$1"
rec="$2"
reps="$3"
out="$4"
size=$(stty size 2>/dev/null | tr ' ' 'x')
bytes=$(wc -c <"$rec")
sleep 1
t0=$(perl -MTime::HiRes=time -e 'print time')
# One `cat` over the recording `reps` times, not one a rep: a fork and
# exec is ~5 ms on this box, and twenty of them were half the wall of a
# 200 ms run -- the same half for every terminal, so the band they all sat
# in was mostly the harness's own forks.
args=()
i=0
while [ "$i" -lt "$reps" ]; do
  args+=("$rec")
  i=$((i + 1))
done
[ "$reps" -gt 0 ] && cat "${args[@]}"
# Every recording is truncated mid-session: btop's ends in a lone ESC and
# inside a synchronised update. So end the update -- wezterm holds its
# replies while one is open, and its RIS does not close it -- then reset
# the terminal, which is reached from any parser state, put the cursor at
# a position no reply to a query *inside* the recording could name, and
# only then ask where it is. The terminal answers once everything before
# the query has been processed, so the reply marks the end of the work.
old=$(stty -g)
stty raw -echo
printf '\033[?2026l\033c\033[13;37H\033[6n'
# The reply is `ESC [ row ; col R`, of whatever length the numbers make:
# `vt-reply.py` reads up to the `R` and gives up after ten seconds rather
# than sit on a terminal that answers differently -- the first version
# read exactly eight bytes and hung two of the five terminals on a
# seven-byte reply.
hex=$(python3 "$(dirname "$0")/vt-reply.py" 10 13 37)
stty "$old"
t1=$(perl -MTime::HiRes=time -e 'print time')
# The terminal is an ancestor of this shell: usually its parent, one hop
# further when it wrapped the command in a shell of its own. Walk up until
# the process is one of the terminals, and say which was found.
pid=$PPID
comm=""
hops=0
while [ "$hops" -lt 5 ]; do
  comm=$(ps -o comm= -p "$pid" 2>/dev/null)
  case "$comm" in
  *alacritty* | *kitty* | *ghostty* | *wezterm* | */baud | baud) break ;;
  esac
  pid=$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')
  hops=$((hops + 1))
done
cpu=$(/usr/bin/top -l 1 -pid "$pid" -stats time 2>/dev/null | tail -1 | tr -d ' ')
total=$((bytes * reps))
printf '%s %s %s %s %s %s %s %s %s\n' "$name" "$(basename "$rec")" "$size" "$total" "$t0" "$t1" "$cpu" "$(basename "$comm")" "$hex" >>"$out"
printf '\033[0m\n[done: %s]\n' "$name"
