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
# This builds every feature of every listed crate INDIVIDUALLY. It is the mode
# the arms are actually used in (`--features new-no-write` alone), and the only
# mode that catches this rot.
#
# `cargo check`, not `build`: these failures are all compile-time, so checking
# is enough and far faster. `-D warnings` because the commonest rot is a stubbed
# body leaving a variable unused, which is a warning the arm must not carry.
#
# **One list, several crates.** The first version hard-wired `baud-bench`, so a
# feature arm in any other crate was gated by nothing -- the "scoped to what it
# was written beside" failure this repo records for `docs.rs`'s `ROOTS` and the
# audit's flat glob. Each entry carries the minimum number of features its
# manifest must yield, which is the positive control: the bench has ~50 and a
# parse that finds fewer than 30 is a parse that stopped matching, while a new
# crate may legitimately have none yet -- for it the control is that the
# manifest exists and was read.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

CRATES="baud-bench:30 baud-core:1 baud-font:0 baud-render:0 baud-present-macos:0 baud-app:0 baud-wayland:0 baud-xkb:0 baud-present-wayland:0"

fail=0
total=0
for entry in $CRATES; do
  crate="${entry%%:*}"
  min="${entry##*:}"
  manifest="$crate/Cargo.toml"
  if [ ! -f "$manifest" ]; then
    echo "FAIL: $manifest does not exist -- the gate would have read nothing for $crate"
    exit 1
  fi
  # The `[features]` section only: alternating awk flag, skip `default` and any
  # comment line whose first token is `#`.
  feats=$(awk '/^\[features\]/{f=1;next} /^\[/{f=0} f && /=/ && $1!="#" {print $1}' "$manifest" | grep -vx default || true)
  count=$(printf '%s\n' "$feats" | grep -c . || true)
  if [ "$count" -lt "$min" ]; then
    echo "FAIL: only $count features parsed from $manifest (expected at least $min) -- the gate is reading nothing"
    exit 1
  fi
  echo "$crate: checking $count feature arms individually (cargo check, -D warnings)"
  while IFS= read -r f; do
    [ -z "$f" ] && continue
    total=$((total + 1))
    # **`--all-targets`, because a bare `cargo check` covers lib and bins
    # only.** When `baud-bench`'s measurement programs were `src/bin/*.rs` a
    # bare check compiled them; they are `[[bench]]` targets now, and a bare
    # check compiled *nothing but the lib* -- measured: a deliberate syntax
    # error in `benches/transport.rs` gave 0 errors without this flag and 2
    # with it. So the move would have silently turned ~50 arm checks into ~50
    # checks of an almost-empty library, which is this repo's
    # gate-scoped-to-the-wrong-thing failure with a fresh carrier.
    if ! out=$(RUSTFLAGS="-D warnings" cargo check --release -q -p "$crate" --all-targets --features "$f" 2>&1); then
      fail=1
      echo "  BROKEN  $crate/$f"
      printf '%s\n' "$out" | grep -E "^error" | head -2 | sed 's/^/      /'
    fi
  done <<<"$feats"
done

if [ "$fail" -ne 0 ]; then
  echo "FAIL: at least one feature arm does not build -- a compiled arm that does not compile cannot be re-measured"
  exit 1
fi
echo "ok: all $total feature arms build"
