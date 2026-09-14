#!/bin/sh
# Seven bands of opaque truecolour background, two rows each, the full
# width: what baud paints for the luma measurement under glass, run as
# BAUD_CMD=scripts/luma-swatch.sh; scripts/luma-swatch.swift is the control.
printf '\033[H\033[2J'
for c in "255;0;0" "0;255;0" "0;0;255" "128;128;128" "255;255;255" "0;0;0" "200;100;50"; do
  printf '\033[48;2;%sm%150s\n%150s\n\033[0m' "$c" "" ""
done
sleep 30
