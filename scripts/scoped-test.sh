#!/usr/bin/env bash
# scripts/scoped-test.sh <from> <to>: the gates a change from <from> to <to>
# needs, run in the dev shell. `cargo xtask test-scope` names them; this
# runs each and exits 1 when any fails, 3 when the scope names every gate
# (the caller then runs `devenv test`), 0 otherwise.
#
# Every run: formatting, sources, no Python, paths and the changelog, which
# build nothing. Then, as the scope names them: clippy of the packages a
# code change reaches; the tests of whole packages and single binaries, in
# one nextest run, and their doc tests, under `release-probe` (optimised,
# no link-time optimisation: the full suite keeps `--release`); the feature arms
# of the crates the change reaches; the raw-integer audit; the event rig;
# the bare-metal builds; the Linux clippy; and the README GIF's flash
# census and `--verify`.
set -u
[ $# -eq 2 ] || {
  echo "usage: scripts/scoped-test.sh <from> <to>"
  exit 2
}
from=$1
to=$2
plan=$(cargo xtask test-scope "$from" "$to") || {
  echo "scoped-test: cargo xtask test-scope failed"
  exit 1
}
echo "scoped-test: $from..$to"
if [ -z "$plan" ]; then
  echo "  (no test reads what changed)"
else
  printf '%s\n' "$plan" | sed 's/^/  /'
fi
if printf '%s\n' "$plan" | grep -q '^full'; then
  exit 3
fi
field() { printf '%s\n' "$plan" | awk -F'\t' -v s="$1" -v n="$2" '$1 == s { print $n }'; }
failed=()
step() {
  local name=$1
  shift
  local t0=$SECONDS
  echo "scoped-test: $name"
  if "$@"; then
    echo "scoped-test: $name passed ($((SECONDS - t0)) s)"
  else
    echo "scoped-test: $name FAILED ($((SECONDS - t0)) s)"
    failed+=("$name")
  fi
}
step fmt treefmt --fail-on-change
step sources bash ./scripts/check-sources.sh
step no-python cargo xtask no-python
step paths bash ./scripts/check-paths.sh
step changelog bash ./scripts/check-changelog.sh

clippy=$(field clippy 2)
if [ -n "$clippy" ]; then
  args=()
  for p in $clippy; do args+=(-p "$p"); done
  step clippy cargo clippy --quiet "${args[@]}" --all-targets -- -D warnings
fi

packages=$(field package 2)
binaries=$(printf '%s\n' "$plan" | awk -F'\t' '$1 == "binary" { print $2 "\t" $3 }')
if [ -n "$packages" ] || [ -n "$binaries" ]; then
  args=()
  filter=""
  for p in $packages; do
    args+=(-p "$p")
    filter="$filter${filter:+ | }package($p)"
  done
  while IFS=$'\t' read -r p id; do
    [ -n "$p" ] || continue
    args+=(-p "$p")
    filter="$filter${filter:+ | }binary_id($id)"
  done <<<"$binaries"
  step tests cargo nextest run --cargo-profile release-probe --cargo-quiet "${args[@]}" -E "$filter" --no-fail-fast
fi

doc=$(field doc 2)
if [ -n "$doc" ]; then
  args=()
  for p in $doc; do args+=(-p "$p"); done
  step doctests cargo test --profile release-probe --quiet --doc "${args[@]}"
fi

if [ -n "$clippy" ]; then
  step features bash ./scripts/check-features.sh --since "$from"
  step audit sh ./audit_all.sh
fi

if [ -n "$(field rig 1)" ] && [ "$(uname -s)" = Darwin ]; then
  step rig env RUSTFLAGS='-D warnings' cargo nextest run --cargo-profile release-probe --cargo-quiet -p baud --features effect-probe --test event_rig
fi
if [ -n "$(field bare 1)" ]; then
  step bare sh -c 'cargo build --release --quiet -p baud-core --target x86_64-unknown-none && cargo build --release --quiet -p baud-font --target x86_64-unknown-none'
fi
if [ -n "$(field linux 1)" ]; then
  step linux cargo clippy --quiet -p baud --target aarch64-unknown-linux-gnu --all-targets -- -D warnings
fi
if [ -n "$(field gif 1)" ]; then
  step gif-flashes env BAUD_TRAILER_GIF="$PWD/assets/readme/session.gif" cargo test --quiet -p baud-bench --test trailer_flashes -- the_trailer_is_below_the_flash_thresholds --ignored --exact
  step gif-verify cargo run --quiet --example demo -- --verify assets/readme/session.gif
fi

if [ ${#failed[@]} -ne 0 ]; then
  echo "scoped-test: FAILED: ${failed[*]}"
  exit 1
fi
echo "scoped-test: every scoped gate passed"
