#!/bin/bash
# usage: probe.sh REPO_DIR CONTROL
d=$1; ctl=$2
RG="rg -i -n -uu -g !.git -c --fixed-strings"
echo "== $d  control '$ctl': $($RG -- "$ctl" $d | awk -F: '{s+=$NF} END{print s+0}') hits"
for t in "splash" "welcome" "first launch" "first_launch" "firstLaunch" "first run" "first_run" "firstRun" "onboarding" "don't show" "do not show" "don't ask" "ask again" "show again" "remind" "dismissed" "dismiss" "never show" "tip of the day" "tips" "what's new" "whats new" "whatsnew" "release notes" "update window" "logo"; do
  n=$($RG -- "$t" $d | awk -F: '{s+=$NF} END{print s+0}')
  printf "  %-16s %s\n" "$t" "$n"
done
