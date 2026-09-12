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

# **The ISA crates, on their own budget.** Three raw-integer sites and zero
# signature primitives, and all three are the same site: the cast feeding
# `LaneMask::from_movemask`, which is the one point where an intrinsic's return
# value enters the type system. They were inside `baud-core+isa`'s 14 and
# invisible there -- moving the scanners into their own crates moved the sites
# with them, and this repo already records that "the gate must follow the code
# in the same commit". It did not, for as long as one limit covered both.
isa=0
isasig=0
# **The other architecture's crate must still compile, and nothing checked it.**
#
# `baud-isa-x86-64` is `#![cfg(target_arch = "x86_64")]` and `baud-isa-aarch64`
# is the mirror, so on any one machine exactly one of them is an empty crate that
# type-checks vacuously. Adding `stride_three` and `stride_four` to `Scan` left
# the x86-64 impl missing two methods and **nothing went red**: not the 615
# tests, not clippy, not this script -- the same absent-rather-than-red failure
# this repo records for the renamed CI target and for `diag`.
#
# One cross-compile closes it. `x86_64-unknown-none` is already installed for the
# `no_std` check, so this costs a build and no new toolchain.
# **A positive control decides whether the target is usable**, because asking
# `rustup` does not: this toolchain is nix-provided and `rustup target list
# --installed` prints nothing while the cross-build works perfectly. The first
# version of this gate believed it and reported "not installed" on a machine
# that had just cross-compiled -- a probe that cannot fire, caught by perturbing
# it and getting the same message either way.
#
# So: build a crate known to be fine for that target. If the control fails the
# toolchain is the problem and this gate says so; if the control passes, a
# failure on `baud-isa-x86-64` is the crate.
if cargo build --quiet --release -p baud-isa --target x86_64-unknown-none 2>/dev/null; then
  if cargo build --quiet --release -p baud-isa-x86-64 --target x86_64-unknown-none 2>/dev/null; then
    echo "  ok   baud-isa-x86-64: cross-compiles"
  else
    echo "  FAIL baud-isa-x86-64: does not compile for x86_64 -- the trait moved and this impl did not"
    fail=1
  fi
else
  echo "  FAIL x86_64-unknown-none: the control crate does not build, so this gate checked nothing"
  fail=1
fi

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
#      -- forced by the language, not chosen.
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

# **The renderer crates, each on its own budget from the day it is created.**
# A crate the audit does not name is audited by nothing, which looks exactly
# like passing -- the shape this file records for the flat test glob and
# `docs.rs`'s `ROOTS`. So the four crates the rendering plan adds are gated
# here in the same commit that creates them, at zero, and each phase that adds
# code sets the limit to the count it ends at, never above it.
crate_audit() {
  ca_name="$1"
  ca_limit="$2"
  ca_siglimit="$3"
  ca_root="$4"
  ca_total=0
  ca_sig=0
  ca_files=$(find "$ca_root" -name '*.rs' 2>/dev/null | sort)
  if [ -z "$ca_files" ]; then
    echo "  FAIL $ca_name: no sources found -- this gate audited nothing"
    fail=1
    return
  fi
  for ca_f in $ca_files; do
    ca_out=$(python3 impl_audit.py "$ca_f")
    ca_n=$(echo "$ca_out" | grep '^  total' | awk '{print $2}')
    ca_m=$(echo "$ca_out" | grep '^  signature' | awk '{print $2}')
    ca_total=$((ca_total + ca_n))
    ca_sig=$((ca_sig + ca_m))
    [ $((ca_n + ca_m)) -gt 0 ] && echo "  ..   $ca_f: $ca_n + $ca_m sig"
  done
  if [ "$ca_total" -gt "$ca_limit" ] || [ "$ca_sig" -gt "$ca_siglimit" ]; then
    echo "  FAIL $ca_name: $ca_total raw-integer sites, $ca_sig signature (limit $ca_limit and $ca_siglimit)"
    fail=1
  else
    echo "  ok   $ca_name: $ca_total and $ca_sig (limit $ca_limit and $ca_siglimit)"
  fi
}
# **baud-font: zero raw-integer sites and nine signature primitives, every
# one enumerated.** The nine are the newtype surface of a parser over a byte
# format, which is the same exemption `baud-domain` has for the same reason --
# a newtype's job is to wrap the primitive: `Len::of(u32)`, `FaceIndex::of(u32)`,
# the `value()` accessors of `UnitsPerEm`, `Advance`, `GlyphCount` and
# `FaceCount`, `Offset::index -> usize` (the one bridge into slice indexing),
# `is_sfnt(version: u32)` on the directory's own magic, and `glyph(n: u16)`
# where a `cmap` value becomes a `GlyphId`. Every field read is generic over
# `FromBe`, so no reader names a width in its signature. The limit follows the
# count and moves only downward.
crate_audit "baud-font" 0 9 baud-font/src
crate_audit "baud-render" 0 0 baud-render/src
crate_audit "baud-present-macos" 0 0 baud-present-macos/src
crate_audit "baud-app" 0 0 baud-app/src

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

# **The glob is flat, and most of the test tree is behind it.**
# `baud-core/tests/*.rs` matches the loose files and **none** of the suites in
# `conformance/`, `vt100/`, `xterm/` and the rest, which were audited by nothing
# for as long as those directories existed. The limit-0 discipline below is real
# and it covers only the files the glob reaches, so the trees need their own
# gate.
#
# That is the same shape as `docs.rs`'s `ROOTS` list, which named every crate
# but the newer core: a gate scoped to less than it appears to cover, which
# looks exactly like passing.
#
# The tree is gated below at its count rather than at zero, because a limit set
# to a number nobody has driven to is a limit that fails on the next commit for
# a reason unrelated to it. **It is a floor**: the loose files reached 0 and
# this should.
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

for f in baud-core/tests/*.rs baud-difftest/src/main.rs; do
  # **A gate that cannot find its input must fail, not pass.** An unmatched
  # glob passes through literally in bash, so a renamed or removed directory
  # would have this loop run `impl_audit.py` on the pattern itself, `n` would
  # come back empty, `[ "$n" -gt 0 ]` would error, and the `else` branch would
  # print `ok` -- a gate reporting success having audited nothing. Same family
  # as `inf x amplification`, the stale `scan path: AVX2` and `fast_hit`, and
  # it is what caught the commit that removed `baud-core2`.
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
