#!/bin/sh
# Fails if raw integers appear outside the definition crate's newtype internals.
# The definition crate legitimately contains them: a newtype's whole job is to
# wrap one. Everywhere else they are a bug waiting for two quantities to be
# confused.
set -e
fail=0
for f in baud-core/src/lib.rs baud-core/tests/*.rs baud-bench/src/*.rs baud-difftest/src/main.rs; do
  n=$(python3 impl_audit.py "$f" | tail -1 | awk '{print $2}')
  # baud-core/src/lib.rs: 10 permitted, all MAX_COLS as an array dimension
  case "$f" in
    # All 10 remaining are structural, not choices:
    #   7  `const X: usize` — Rust array dimensions must be usize
    #   1  `arena: [u8; ARENA]` — the wire; bytes are bytes
    #   4  SIMD lane masks — a movemask result is a bitmask and
    #      trailing_zeros() on it is a lane index, not a domain quantity
    #      (scan_avx2, find_final, find_st)
    # Everything else goes through baud-domain. If this number rises, something
    # untyped was reintroduced.
    baud-core/src/lib.rs) limit=12 ;;
    baud-bench/src/main.rs) limit=4 ;;
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
exit $fail
