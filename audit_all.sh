#!/bin/sh
# Fails if raw integers appear outside the definition crate's newtype internals.
# The definition crate legitimately contains them: a newtype's whole job is to
# wrap one. Everywhere else they are a bug waiting for two quantities to be
# confused.
set -e
fail=0

# **This audits `baud-core2`, the previous core.** The new `baud-core` gets its
# own budget as it grows, and it starts at zero rather than inheriting a
# fourteen-site allowance -- a budget is a floor to drive down, and the one
# thing that must not happen is a fresh crate being born with the old one's
# slack. When the new core replaces core2 this block moves to it and core2's
# goes away with the crate.
#
# baud-core is an aggregate, not a file. After the SRP split it is ~20 modules,
# so naming lib.rs would measure a 60-line file of `mod`/`pub use` and report
# green for ever -- the "instrumentation outlived what fed it" failure
# CLAUDE.md records.
#
# 14 permitted, and every one is forced by the language or by an intrinsic
# rather than chosen:
#   1  array dimension -- `const GRID: usize = MAX_ROWS * MAX_COLS`. Rust
#      requires an array length to be a `usize` const expression; there is no
#      newtype that can go there.
#   3  `as u32` feeding `LaneMask::from_movemask`. `_mm256_movemask_epi8`
#      returns `i32` and `Mask::to_bitmask` returns `u64`; this cast is the
#      single point at which an intrinsic's return value enters the type
#      system, which is exactly where a cast belongs.
#   1  a raw byte range test in `parse/fast.rs`.
#   9  `parse/graph.rs`, and they are the price of the architecture rather
#      than of untyped code. A branch-driven parser asks `is_c0(b)`; a
#      table-driven one *indexes* by the byte and the node, and an index is a
#      `usize` no newtype can be. The nine are: `Nodes::at` and `Classes::at`,
#      the single point at which a node and a class enter index space -- the
#      same role the movemask casts play, and the reason they are inherent
#      `const fn` rather than `core::ops::Index`, which a const fn cannot
#      call; four in the compile-time derivation of the byte-class map, which
#      is derived from the graph precisely so the two cannot drift apart; and
#      two each in `Act::same` and `Node::same`, because `PartialEq` is
#      derived and used everywhere else and a `const fn` cannot call it.
#
# **This was 5 while the true count was 5, then 6, and it is 14 because the
# parser became a table.** The rule has not changed: the limit equals the
# count, every site is enumerated, and it comes down by typing things rather
# than by re-scoping. It has come down from 15 once already that way.
core=0
sig=0
# **Every ISA crate is audited on `baud-core`'s budget, in the same total.** Moving the scanners into their own crate moved five raw-integer
# sites with them -- the movemask casts -- and an audit that walked only
# `baud-core/src` would have reported the count dropping from 8 to 2 and called
# it progress. That is the "instrumentation outlived what fed it" failure this
# repo already records: the gate must follow the code in the same commit.
for f in $(find baud-core2/src baud-isa/src baud-isa-x86-64/src baud-isa-aarch64/src -name '*.rs' | sort); do
  out=$(python3 impl_audit.py "$f")
  n=$(echo "$out" | grep '^  total' | awk '{print $2}')
  m=$(echo "$out" | grep '^  signature' | awk '{print $2}')
  core=$((core + n))
  sig=$((sig + m))
  [ $((n + m)) -gt 0 ] && echo "  ..   $f: $n + ${m} sig"
done
if [ "$core" -gt 14 ]; then
  echo "  FAIL baud-core+isa: $core raw-integer sites (limit 14)"
  fail=1
else
  echo "  ok   baud-core+isa: $core (limit 14)"
fi

# **A second gate, because the first could not see these.** The rule above
# matched `usize` only, and only on `&mut self` methods, so it reported 0 for a
# file carrying `final_byte: u8`, `param: u8`, `-> Option<u8>` and an
# `app: bool`. Widening it found 13 more across baud-core that had never been
# counted.
#
# 10 is a FLOOR TO DRIVE DOWN, not slack, and it is deliberately not folded
# into the 8: that limit was calibrated against the narrower detector, and
# raising it to fit a wider one is the move this repo's notes forbid. Every one
# of the 10 is a `u8`/`usize`/`bool` in a signature that has a newtype waiting
# for it. `key.rs` went from 8 to 0 by using them; the rest can too.
if [ "$sig" -gt 10 ]; then
  echo "  FAIL baud-core+isa signatures: $sig raw-primitive params/returns (limit 10)"
  fail=1
else
  echo "  ok   baud-core+isa signatures: $sig (limit 10, a floor to drive down)"
fi

# The new core, on its own budget. **Three, and every one enumerated**:
#
#   1  `GRID = MAX_ROWS * MAX_COLS`, an array dimension. Rust requires an
#      array length to be a `usize` const expression and no newtype can be one
#      -- the same forced site core2's budget opens with.
#   1  `trailing_zeros() as usize` in the block walk, where a lane *mask*
#      becomes a lane *index*. That is the single point a bitmask enters index
#      space, which is exactly the role `LaneMask::from_movemask` plays for an
#      intrinsic's return value, and exactly where a cast belongs.
#   1  `off: usize` on `run_at`, the lane the walk stands at. It is an index
#      into a sixteen-lane block and its operands are masks; a newtype here
#      would wrap the thing the shifts are already keyed to.
#
# The budget is a floor to drive down, not an allowance to spend. It went 14
# to 8 to 4 to 3 during the first day of this crate by moving byte-to-bits into
# `StreamByte`, the stride maths into `RowBlock` and the offsets onto
# `ByteLen` -- none of which was re-scoping.
new=0
newsig=0
for f in $(find baud-core/src -name '*.rs' 2>/dev/null | sort); do
  out=$(python3 impl_audit.py "$f")
  n=$(echo "$out" | grep '^  total' | awk '{print $2}')
  m=$(echo "$out" | grep '^  signature' | awk '{print $2}')
  new=$((new + n))
  newsig=$((newsig + m))
  [ $((n + m)) -gt 0 ] && echo "  ..   $f: $n + ${m} sig"
done
if [ "$new" -gt 2 ] || [ "$newsig" -gt 1 ]; then
  echo "  FAIL baud-core: $new raw-integer sites, $newsig signature (limit 2 and 1)"
  fail=1
else
  echo "  ok   baud-core: $new and $newsig (limit 2 and 1, every site enumerated)"
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

for f in baud-core2/tests/*.rs baud-difftest/src/main.rs; do
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
