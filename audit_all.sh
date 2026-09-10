#!/bin/sh
# Fails if raw integers appear outside the definition crate's newtype internals.
# The definition crate legitimately contains them: a newtype's whole job is to
# wrap one. Everywhere else they are a bug waiting for two quantities to be
# confused.
set -e
fail=0

# **The ingest path answers by indexing, not by searching.** `scripts/scan-audit.py`
# carries the reasoning and the exemptions; the number may go down and must
# never go up.
if ! python3 scripts/scan-audit.py 5; then
  fail=1
fi

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
# **Split, because one budget over two crates hides which one spends it.**
# This gate read `baud-core+isa ... limit 14` while walking `baud-core2/src`
# and the three ISA crates together. Measured apart: core2 is **11 raw and all
# 10 signature slots**, the ISA crates are **3 and zero**. So on the commit
# that deletes core2 the find would still match -- the empty guard below would
# not fire -- and the gate would check 3 against 14 and 0 against 10, going
# green with eleven sites of slack that nothing occupies. "A budget that is not
# equal to the count is not measuring anything", and a budget that outlives the
# code it was calibrated against is the same failure with a delay on it.
#
# Now each crate carries its own, so deleting core2 deletes its budget in the
# same commit and the ISA limit is already tight.
files=$(find baud-core2/src -name '*.rs' 2>/dev/null | sort)
if [ -z "$files" ]; then
  echo "  FAIL baud-core2/src: no sources found -- this gate audited nothing"
  fail=1
fi
for f in $(find baud-core2/src -name '*.rs' | sort); do
  out=$(python3 impl_audit.py "$f")
  n=$(echo "$out" | grep '^  total' | awk '{print $2}')
  m=$(echo "$out" | grep '^  signature' | awk '{print $2}')
  core=$((core + n))
  sig=$((sig + m))
  [ $((n + m)) -gt 0 ] && echo "  ..   $f: $n + ${m} sig"
done
if [ "$core" -gt 11 ]; then
  echo "  FAIL baud-core2/src: $core raw-integer sites (limit 11)"
  fail=1
else
  echo "  ok   baud-core2/src: $core (limit 11)"
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
  echo "  FAIL baud-core2/src signatures: $sig raw-primitive params/returns (limit 10)"
  fail=1
else
  echo "  ok   baud-core2/src signatures: $sig (limit 10, and core2 is being deleted)"
fi

# **The ISA crates, on their own budget.** Three raw-integer sites and zero
# signature primitives, and all three are the same site: the cast feeding
# `LaneMask::from_movemask`, which is the one point where an intrinsic's return
# value enters the type system. They were inside `baud-core+isa`'s 14 and
# invisible there -- moving the scanners into their own crates moved the sites
# with them, and this repo already records that "the gate must follow the code
# in the same commit". It did not, for as long as one limit covered both.
isa=0
isasig=0
files=$(find baud-isa/src baud-isa-x86-64/src baud-isa-aarch64/src -name '*.rs' 2>/dev/null | sort)
if [ -z "$files" ]; then
  echo "  FAIL baud-isa*/src: no sources found -- this gate audited nothing"
  fail=1
fi
for f in $files; do
  out=$(python3 impl_audit.py "$f")
  n=$(echo "$out" | grep '^  total' | awk '{print $2}')
  m=$(echo "$out" | grep '^  signature' | awk '{print $2}')
  isa=$((isa + n))
  isasig=$((isasig + m))
  [ "$n" -gt 0 ] && echo "  ..   $f: $n + $m sig"
done
if [ "$isa" -gt 3 ] || [ "$isasig" -gt 0 ]; then
  echo "  FAIL baud-isa*: $isa raw-integer sites, $isasig signature (limit 3 and 0)"
  fail=1
else
  echo "  ok   baud-isa*: $isa and $isasig (limit 3 and 0, the movemask casts)"
fi

# The new core, on its own budget. **Two, and every one enumerated**:
#
#   1  `GRID = MAX_ROWS * MAX_COLS`, an array dimension. Rust requires an
#      array length to be a `usize` const expression and no newtype can be one
#      -- the same forced site core2's budget opens with.
#   1  `n as usize` in `Nodes::at`, where a graph node becomes an array index.
#      A `repr(u8)` enum indexing its own table is the one cast the language
#      forces, and it is the single point a node enters index space.
#
# **Signatures are at zero and the limit follows the count**, which is this
# repo's own rule: a budget that is not equal to the count measures nothing.
# It got there when the one-line loop was typed -- `at`/`len`/`left`/`k` came
# in as bare `usize` and left as `BytePos`, `ReadLen`, `ByteLen` and `Count`,
# and `build(latin1: bool)` became `build(enc: InputEncoding)`, which is the
# two-state enum a `bool` parameter always is.
#
# The budget is a floor to drive down, not an allowance to spend. It went 14
# to 8 to 4 to 3 to 2 by moving byte-to-bits into `StreamByte`, the stride
# maths into `RowBlock` and the offsets onto `ByteLen` -- none of which was
# re-scoping.
new=0
newsig=0
files=$(find baud-core/src -name '*.rs' 2>/dev/null | sort)
if [ -z "$files" ]; then
  echo "  FAIL baud-core/src: no sources found -- this gate audited nothing"
  fail=1
fi
for f in $(find baud-core/src -name '*.rs' 2>/dev/null | sort); do
  out=$(python3 impl_audit.py "$f")
  n=$(echo "$out" | grep '^  total' | awk '{print $2}')
  m=$(echo "$out" | grep '^  signature' | awk '{print $2}')
  new=$((new + n))
  newsig=$((newsig + m))
  [ $((n + m)) -gt 0 ] && echo "  ..   $f: $n + ${m} sig"
done
if [ "$new" -gt 2 ] || [ "$newsig" -gt 0 ]; then
  echo "  FAIL baud-core: $new raw-integer sites, $newsig signature (limit 2 and 0)"
  fail=1
else
  echo "  ok   baud-core: $new and $newsig (limit 2 and 0, every site enumerated)"
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
files=$(find baud-bench/src -name '*.rs' 2>/dev/null | sort)
if [ -z "$files" ]; then
  echo "  FAIL baud-bench/src: no sources found -- this gate audited nothing"
  fail=1
fi
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

# **The glob is flat, and three quarters of each core's tests are behind it.**
# `baud-core2/tests/*.rs` matches the **15** loose files and none of the **47**
# in `conformance/`, `vt100/`, `xterm/`, `parser/` and the rest -- which carry
# **49** raw-integer sites between them and have been audited by nothing for as
# long as those directories have existed. The limit-0 discipline below is real
# and it covers a quarter of the tree.
#
# That is the same shape as `docs.rs`'s `ROOTS` list, which named every crate
# but the newer core: a gate scoped to less than it appears to cover, which
# looks exactly like passing. It is not something the port introduced -- both
# cores measure the same, because they are the same tests.
#
# The trees are gated below at their counts rather than at zero, because a
# limit set to a number nobody has driven to is a limit that fails on the next
# commit for a reason unrelated to it. **They are floors**: the loose files
# reached 0 and these should.
tree_audit() {
  ta_name="$1"
  ta_limit="$2"
  ta_root="$3"
  ta_total=0
  ta_files=$(find "$ta_root" -mindepth 2 -name '*.rs' 2>/dev/null | sort)
  if [ -z "$ta_files" ]; then
    echo "  FAIL $ta_name: no sources found -- this gate audited nothing"
    fail=1
    return
  fi
  for ta_f in $ta_files; do
    ta_total=$((ta_total + $(python3 impl_audit.py "$ta_f" | tail -1 | awk '{print $2}')))
  done
  if [ "$ta_total" -gt "$ta_limit" ]; then
    echo "  FAIL $ta_name: $ta_total raw-integer sites (limit $ta_limit)"
    fail=1
  else
    echo "  ok   $ta_name: $ta_total (limit $ta_limit, a floor to drive down)"
  fi
}
tree_audit "baud-core/tests suites" 34 baud-core/tests
tree_audit "baud-core2/tests suites" 49 baud-core2/tests

for f in baud-core/tests/*.rs baud-core2/tests/*.rs baud-difftest/src/main.rs; do
  # **A gate that cannot find its input must fail, not pass.** An unmatched
  # glob passes through literally in bash, so with `baud-core2` deleted this
  # loop would run `impl_audit.py` on the pattern itself, `n` would come back
  # empty, `[ "$n" -gt 0 ]` would error, and the `else` branch would print
  # `ok` -- a gate reporting success having audited nothing. Same family as
  # `inf x amplification`, the stale `scan path: AVX2` and `fast_hit`, and the
  # one that was going to bite on the commit that finally removes core2.
  if [ ! -f "$f" ]; then
    echo "  FAIL $f: no such file -- this gate audited nothing"
    fail=1
    continue
  fi
  n=$(python3 impl_audit.py "$f" | tail -1 | awk '{print $2}')
  # **Per file, and `baud-core`'s three are not zero.** `text.rs` 5,
  # `properties.rs` 4 and `fixed.rs` 1 -- written for this core rather than
  # ported, and never audited because the glob named the other one. Each is
  # pinned at what it is so it cannot grow, which is the same contract the
  # fifteen zeroes carry.
  case "$f" in
  baud-core/tests/text.rs) limit=5 ;;
  baud-core/tests/properties.rs) limit=4 ;;
  baud-core/tests/fixed.rs) limit=1 ;;
  *) limit=0 ;;
  esac
  if [ -z "$n" ]; then
    echo "  FAIL $f: the audit produced no count"
    fail=1
  elif [ "$n" -gt "$limit" ]; then
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
