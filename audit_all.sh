#!/bin/sh
# Fails if raw integers appear outside the definition crate's newtype internals.
# The definition crate legitimately contains them: a newtype's whole job is to
# wrap one. Everywhere else they are a bug waiting for two quantities to be
# confused.
set -e
fail=0
for f in baud-core/src/lib.rs baud-core/tests/*.rs baud-bench/src/*.rs baud-difftest/src/main.rs; do
  n=$(python3 impl_audit.py "$f" | tail -1 | awk '{print $2}')
  # baud-core/src/lib.rs: 8 permitted, and every one is forced by the language
  # or by an intrinsic rather than chosen:
  #   3  array dimensions -- `const GRID: usize = MAX_ROWS * MAX_COLS` and the
  #      two `[PackedCell; NSTYLE * MAX_COLS]`. Rust requires an array length to
  #      be a `usize` const expression; there is no newtype that can go there.
  #   5  `as u32` feeding `LaneMask::from_movemask`. `_mm256_movemask_epi8`
  #      returns `i32` and `Mask::to_bitmask` returns `u64`; this cast is the
  #      single point at which an intrinsic's return value enters the type
  #      system, which is exactly where a cast belongs.
  #
  # Was 15. The seven removed were not exemptions that expired -- they were
  # untyped quantities: a movemask result and a lane index (now `LaneMask` and
  # `Lane`), a sub-parameter bitmask (`SubParams`), an SGR loop counter that had
  # to step in lockstep with that mask (`ParamIdx`), and two `rows * stride`
  # slice computations (`RowBlock`). Do not raise this number to make a change
  # fit; the remaining eight have nowhere to go.
  case "$f" in
  baud-core/src/lib.rs) limit=8 ;;
  # 4 before treefmt, 12 after, with nothing untyped added. impl_audit.py
  # anchors `^\s+\w+: (usize|bool|...),` to a line, meaning to catch struct
  # fields; rustfmt breaks a long fn signature onto one parameter per line, and
  # `total: usize,` as a parameter is then indistinguishable from a field. The
  # six new field hits and four new bool hits at lines 22-23, 122-123, 133-135
  # and 157-159 are all parameters of run_once/run_once_t that were already
  # there, hand-wrapped onto shared lines where the regex could not see them.
  #
  # The limit is raised rather than the formatter disabled, but note what that
  # costs: 12 permits more genuinely untyped sites than 4 did, and the old
  # calibration cannot be recovered without unformatting the file. The real fix
  # is to make impl_audit.py distinguish a field from a parameter.
  baud-bench/src/main.rs) limit=12 ;;
  baud-bench/src/payload.rs) limit=2 ;;
  *) limit=0 ;;
  esac
  if [ "$n" -gt "$limit" ]; then
    echo "  FAIL $f: $n raw-integer sites (limit $limit)"
    fail=1
  else
    echo "  ok   $f: $n (limit $limit)"
  fi
done

# baud-domain is exempt from the count above -- a newtype's whole job is to wrap
# a primitive, so counting them there measures nothing. But that exemption had a
# hole: it also hid primitives on the crate's *API surface*, and five `bool`
# fields accumulated inside protocol enums -- `Csi::Mode { set: bool }`,
# `Csi::SelectProtection { protect: bool }`, `Report::MouseSgr { release: bool }`
# and two more. Each was precisely the failure `Marker`'s own doc-comment argues
# against: "a `bool` argument at a call site says nothing".
#
# A `bool` *field* is never a newtype internal -- a newtype over bool is
# `struct X(bool)`, which this pattern does not match -- so zero is the right
# limit and there is no judgement call in it.
b=$(grep -cE "^[[:space:]]+[a-z_]+: bool,$" baud-domain/src/lib.rs || true)
if [ "$b" -gt 0 ]; then
  echo "  FAIL baud-domain/src/lib.rs: $b bool fields (limit 0) -- use a named two-state enum"
  grep -nE "^[[:space:]]+[a-z_]+: bool,$" baud-domain/src/lib.rs
  fail=1
else
  echo "  ok   baud-domain/src/lib.rs: 0 bool fields (limit 0)"
fi

exit $fail
