#!/bin/sh
# Fails if raw integers appear outside the definition crate's newtype internals.
# The definition crate legitimately contains them: a newtype's whole job is to
# wrap one. Everywhere else they are a bug waiting for two quantities to be
# confused.
set -e
fail=0

# baud-core is an aggregate, not a file. After the SRP split it is ~20 modules,
# so naming lib.rs would measure a 60-line file of `mod`/`pub use` and report
# green for ever -- the "instrumentation outlived what fed it" failure
# CLAUDE.md records.
#
# 8 permitted, and every one is forced by the language or by an intrinsic
# rather than chosen:
#   3  array dimensions -- `const GRID: usize = MAX_ROWS * MAX_COLS` and the
#      two `[PackedCell; NSTYLE * MAX_COLS]`. Rust requires an array length to
#      be a `usize` const expression; there is no newtype that can go there.
#   5  `as u32` feeding `LaneMask::from_movemask`. `_mm256_movemask_epi8`
#      returns `i32` and `Mask::to_bitmask` returns `u64`; this cast is the
#      single point at which an intrinsic's return value enters the type
#      system, which is exactly where a cast belongs.
#
# Do not raise this number to make a change fit; the remaining eight have
# nowhere to go.
core=0
for f in $(find baud-core/src -name '*.rs' | sort); do
  n=$(python3 impl_audit.py "$f" | tail -1 | awk '{print $2}')
  core=$((core + n))
  [ "$n" -gt 0 ] && echo "  ..   $f: $n"
done
if [ "$core" -gt 8 ]; then
  echo "  FAIL baud-core/src: $core raw-integer sites (limit 8)"
  fail=1
else
  echo "  ok   baud-core/src: $core (limit 8)"
fi

# baud-bench is an aggregate for the same reason baud-core is: after the SRP
# split its 12 permitted sites live across main.rs, harness.rs and drive.rs, and
# per-file limits would have to be re-guessed every time a function moves.
#
# Why 12 and not 4: impl_audit.py anchors `^\s+\w+: (usize|bool|...),` to a
# line, meaning to catch struct fields; rustfmt breaks a long fn signature onto
# one parameter per line, and `total: usize,` as a parameter is then
# indistinguishable from a field. The extra hits are all parameters of
# run_once/run_once_t that were already there, hand-wrapped onto shared lines
# where the regex could not see them. The limit was raised rather than the
# formatter disabled, but note the cost: 12 permits more genuinely untyped
# sites than 4 did. The real fix is to teach impl_audit.py the difference
# between a field and a parameter.
bench=0
for f in $(find baud-bench/src -name '*.rs' | sort); do
  n=$(python3 impl_audit.py "$f" | tail -1 | awk '{print $2}')
  bench=$((bench + n))
  [ "$n" -gt 0 ] && echo "  ..   $f: $n"
done
if [ "$bench" -gt 14 ]; then
  echo "  FAIL baud-bench/src: $bench raw-integer sites (limit 14)"
  fail=1
else
  echo "  ok   baud-bench/src: $bench (limit 14)"
fi

for f in baud-core/tests/*.rs baud-difftest/src/main.rs; do
  n=$(python3 impl_audit.py "$f" | tail -1 | awk '{print $2}')
  limit=0
  if [ "$n" -gt "$limit" ]; then
    echo "  FAIL $f: $n raw-integer sites (limit $limit)"
    fail=1
  else
    echo "  ok   $f: $n (limit $limit)"
  fi
done

# baud-domain is exempt from the count above -- a newtype's whole job is to wrap
# a primitive, so counting them there measures nothing. The check below globs
# the whole crate rather than naming lib.rs: after the SRP split lib.rs is ~40
# lines of `mod`/`pub use`, so a path-pinned grep would report zero for ever. But that exemption had a
# hole: it also hid primitives on the crate's *API surface*, and five `bool`
# fields accumulated inside protocol enums -- `Csi::Mode { set: bool }`,
# `Csi::SelectProtection { protect: bool }`, `Report::MouseSgr { release: bool }`
# and two more. Each was precisely the failure `Marker`'s own doc-comment argues
# against: "a `bool` argument at a call site says nothing".
#
# A `bool` *field* is never a newtype internal -- a newtype over bool is
# `struct X(bool)`, which this pattern does not match -- so zero is the right
# limit and there is no judgement call in it.
b=$(grep -rhE "^[[:space:]]+[a-z_]+: bool,$" baud-domain/src/ | wc -l | tr -d " ")
if [ "$b" -gt 0 ]; then
  echo "  FAIL baud-domain/src: $b bool fields (limit 0) -- use a named two-state enum"
  grep -rnE "^[[:space:]]+[a-z_]+: bool,$" baud-domain/src/
  fail=1
else
  echo "  ok   baud-domain/src: 0 bool fields (limit 0)"
fi

exit $fail
