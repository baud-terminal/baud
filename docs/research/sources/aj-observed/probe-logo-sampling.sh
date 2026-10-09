#!/bin/bash
# Note AJ, section 2.2: how the `logo` hits in four of the six clones were
# sampled -- the files with the most matching lines, the directories that
# hold the rest, and the matching lines in app code, including iTerm2's top
# directory for the term. rg --sort path makes the output the same on every
# run. Run from the directory holding the clones (see probe-run.sh for the
# pins).
RG="rg --sort path -i -uu -g !.git --fixed-strings"
for d in ghostty-org_ghostty wezterm_wezterm; do
  echo "== $d: files with the most matching lines"
  (cd "$d" && $RG -c logo . | sort -t: -k2 -rn | head -12)
done
for d in microsoft_terminal gnachman_iTerm2; do
  echo "== $d: directories holding the matching files"
  (cd "$d" && $RG -c logo . | sed 's|/[^/]*$||' | sort | uniq -c | sort -rn | head -8)
done
echo "== matching lines read in app code"
(cd ghostty-org_ghostty && $RG -n logo macos/Sources/App/AppDelegate.swift src/apprt/gtk/class/window.zig src/termio/Exec.zig | cut -c1-160)
(cd microsoft_terminal && $RG -n logo src/cascadia/CascadiaPackage | cut -c1-160 | head -8)
(cd gnachman_iTerm2 && $RG -n logo sources | cut -c1-160 | head -12)
(cd gnachman_iTerm2 && $RG -n logo sources/OpenQuickly | cut -c1-160)
