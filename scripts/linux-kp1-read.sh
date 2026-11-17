#!/usr/bin/env bash
# Reads a `linux-kp1.sh` run: per arm, over every round but the first, the
# median, quartiles and 95th percentile of the shown keys' latency in ms,
# each round's median, and the keys not shown or whose rect moved before
# the key.
#
#   scripts/linux-kp1-read.sh <outdir>
#
# A quantile is the exclusive one, Python's `statistics.quantiles` default:
# rank (n + 1) p, linear between the two ranks about it. A p95 between two
# refreshes' worth of keys lands between them, and other definitions read it
# far apart.
set -euo pipefail
out=${1:?usage: scripts/linux-kp1-read.sh <outdir>}
# q(sorted values, count, fraction), ranks clamped to the ends.
quant='function q(v, n, p,  h, lo) { h = (n + 1) * p; if (h < 1) h = 1; if (h > n) h = n; lo = int(h); return lo >= n ? v[n] : v[lo] + (h - lo) * (v[lo + 1] - v[lo]) }'
printf '%-10s %6s %13s %6s  %-20s %s\n' arm median IQR p95 'round medians' 'not shown, not idle'
for arm in $(find "$out" -name 'r*-*.txt' | sed 's|.*/r[0-9]*-||; s|\.txt$||' | sort -u); do
  files=$(find "$out" -name "r*-$arm.txt" ! -name 'r0-*' | sort -V)
  [ -n "$files" ] || continue
  rounds=""
  for f in $files; do
    m=$(awk '$1 == "key" { print $5 / 1000 }' "$f" | sort -g |
      awk "$quant"' { v[NR] = $1 } END { if (NR) printf "%.2f", q(v, NR, 0.5) }')
    rounds="$rounds${rounds:+, }$m"
  done
  # shellcheck disable=SC2086
  cat $files | awk '$1 == "key" { print $5 / 1000 }' | sort -g |
    awk -v arm="$arm" -v rounds="$rounds" \
      -v miss="$(cat $files | grep -c '^not-shown' || true), $(cat $files | grep -c '^not-idle' || true)" \
      "$quant"' { v[NR] = $1 } END {
        printf "%-10s %6.2f %6.2f-%-6.2f %6.2f  %-20s %s\n", arm, q(v, NR, 0.5), q(v, NR, 0.25), q(v, NR, 0.75), q(v, NR, 0.95), rounds, miss
      }'
done
