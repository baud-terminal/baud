#!/usr/bin/env bash
# Feature-arm build gate.
#
# baud keeps its ceiling and probe arms behind cargo features, off by default,
# because "a compiled arm is how the next person re-measures rather than
# rebuilds." That only holds while the arms *compile*, and nothing checked it:
# the pre-push gate builds with no `--features`, and `--all-features` builds them
# all at once, which is a different thing -- mutually-exclusive stubs and
# cross-feature variable use mask the per-arm errors. A review found 10 of ~50
# arms broken while the tree was green, several rotted by ordinary signature
# changes (an `&mut`, a function moved behind another feature, a renamed slice).
#
# This builds every `baud-bench` feature INDIVIDUALLY. It is the mode the arms
# are actually used in (`--features new-no-write` alone), and the only mode that
# catches this rot.
#
# `cargo check`, not `build`: these failures are all compile-time, so checking
# is enough and far faster. `-D warnings` because the commonest rot is a stubbed
# body leaving a variable unused, which is a warning the arm must not carry.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

MANIFEST="baud-bench/Cargo.toml"
# The `[features]` section only: alternating awk flag, skip `default` and any
# comment line whose first token is `#`.
feats=$(awk '/^\[features\]/{f=1;next} /^\[/{f=0} f && /=/ && $1!="#" {print $1}' "$MANIFEST" | grep -vx default)

count=$(printf '%s\n' "$feats" | grep -c .)
# Positive control: a gate that scans nothing looks exactly like a gate that
# passes. If the manifest parse ever stops matching, fail loudly rather than
# report success having checked zero arms -- the failure this repo already
# records for a `find` over a renamed directory and a CI step naming a deleted
# target.
if [ "$count" -lt 30 ]; then
  echo "FAIL: only $count features parsed from $MANIFEST -- the gate is reading nothing"
  exit 1
fi

echo "checking $count feature arms individually (cargo check, -D warnings)"
fail=0
while IFS= read -r f; do
  [ -z "$f" ] && continue
  if ! out=$(RUSTFLAGS="-D warnings" cargo check --release -q -p baud-bench --features "$f" 2>&1); then
    fail=1
    echo "  BROKEN  $f"
    printf '%s\n' "$out" | grep -E "^error" | head -2 | sed 's/^/      /'
  fi
done <<<"$feats"

if [ "$fail" -ne 0 ]; then
  echo "FAIL: at least one feature arm does not build -- a compiled arm that does not compile cannot be re-measured"
  exit 1
fi
echo "ok: all $count feature arms build"
