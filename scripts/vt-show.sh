#!/bin/bash
# Replays a recording once, closes any synchronised update it left open,
# and holds the screen so the terminal can be photographed -- the check
# `vt-child.sh`'s cursor report cannot make, that the bytes were *drawn*.
#
# usage: vt-show.sh <recording> [seconds]
cat "$1"
printf '\033[?2026l'
sleep "${2:-6}"
