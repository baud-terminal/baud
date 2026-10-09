#!/bin/bash
# Note AJ, section 2.2: run probe.sh over the six terminal clones, each a
# `git clone --depth 1` at the pin printed beside it. Run from the directory
# holding the clones, so that only the clone names appear in the output.
# usage: cd <clones> && bash probe-run.sh /path/to/probe.sh
P=${1:-./probe.sh}
for pair in "ghostty-org_ghostty:cursor-style" "kovidgoyal_kitty:cursor_trail" \
            "wezterm_wezterm:default_cursor_style" "gnachman_iTerm2:Profiles" \
            "microsoft_terminal:startupActions" "warpdotdev_Warp:warpify"; do
  d=${pair%%:*}; c=${pair#*:}
  echo "$d HEAD $(git -C "$d" rev-parse HEAD)"
  bash "$P" "$d" "$c"
done
