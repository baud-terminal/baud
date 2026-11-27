#!/usr/bin/env bash
# The AVX2 arm's checks: `devenv tasks run test:x86-avx2`.
#
# `baud-isa-x86-64` and baud-core's `avx2-scan` compile to anything at all
# only under `target_feature = "avx2"`, so an aarch64 machine cannot build
# them and an x86-64 build without `+avx2` builds the portable path in their
# place, with every test green. This runs where the arm exists: **any x86_64
# Linux whose /proc/cpuinfo lists avx2**. Anywhere else it refuses and exits
# non-zero, so it can never pass having checked nothing.
#
# Three checks, each with `-C target-feature=+avx2`:
#
#   1. `baud-isa-x86-64`'s tests: `tests/agrees.rs`, `Avx2` against
#      `Portable`, and `tests/cache.rs`, the CPUID cache walk. `agrees.rs`
#      is cfg'd on the feature, so a build that lost the flag runs no test
#      in it and passes; the count of tests it ran is asserted.
#   2. The workspace suite with `baud-core/avx2-scan`, the arm as baud
#      would ship it. It shows that the arm compiles and that the suite
#      passes under it, not that the flag reaches the scan: if the cfg
#      never matched, the build would take the portable path and pass.
#   3. That the flag reaches the scan. The release `ingest2` bench is built
#      twice, both with `+avx2` and only the feature differing; the two
#      binaries must differ (`cmp -s`), the avx2 one must hold more 32-byte
#      movemasks (`vpmovmskb` on a `%ymm`) than the base one -- the base
#      holds some of its own, from code outside the scan, so a count above
#      zero proves nothing -- and the two must write the same cells on five
#      recordings.
#
# **Every check runs, and the script fails if any failed**: each runs in a
# subshell of its own, so a red check 2 does not leave check 3 unrun, and
# the end names each check that failed. Sourced rather than run, the file
# defines its functions and runs nothing (`crates/xtask/tests/x86_avx2_script.rs`).
#
# Why 3 asserts on the register and not on `vpmovmskb` alone: both arms
# carry `+avx2`, so LLVM VEX-encodes the portable path's movemask too and
# the token is in both binaries. `Avx2::run` steps 32 bytes and movemasks a
# `%ymm`; the portable loop is `LANE_COUNT` = 16 and movemasks a `%xmm`
# however it is encoded.
#
# `OBJDUMP` names the disassembler; devenv.nix passes nixpkgs' GNU objdump
# on Linux. `CARGO_TARGET_DIR` is honoured as cargo honours it.

RF="-C target-feature=+avx2"

refuse() {
  echo "test:x86-avx2 REFUSES: $1" >&2
  echo "  it needs an x86_64 Linux whose /proc/cpuinfo lists avx2," >&2
  echo "  and passes nowhere else, so that it cannot pass having checked nothing" >&2
  exit 1
}

# **Whether the avx2 arm is live**: its binary holds more 32-byte
# movemasks, `$1`, than the base binary's, `$2`.
live() {
  [ "$1" -gt "$2" ]
}

check1() {
  RUSTFLAGS="$RF" cargo test --release --no-fail-fast -p baud-isa-x86-64 -- --nocapture
  local ran
  ran=$(RUSTFLAGS="$RF" cargo test --release -q -p baud-isa-x86-64 --test agrees -- --list 2>/dev/null |
    grep -c ': test$' || true)
  if [ "$ran" -eq 0 ]; then
    echo "FAIL: tests/agrees.rs holds no test under $RF -- target_feature = \"avx2\" was false" >&2
    return 1
  fi
  echo "tests/agrees.rs: $ran tests under $RF"
}

check2() {
  RUSTFLAGS="$RF" cargo test --release --no-fail-fast --workspace --features baud-core/avx2-scan
}

# **A `[[bench]]` target lands under a feature-dependent hash, not at
# `target/release/<name>`**, so the path is asked for rather than assumed.
# `--message-format=json` reports it even on a cached build, where the
# human-readable "Executable ..." line does not print -- and a glob plus
# "newest" would race the two arms, which differ only by a feature and so
# coexist.
bench_exe() {
  # shellcheck disable=SC2086 # $1 is zero or two words, split on purpose
  RUSTFLAGS="$RF" cargo bench --no-run -q -p baud-bench --bench ingest2 $1 --message-format=json |
    grep -o '"executable":"[^"]*ingest2[^"]*"' | tail -1 | cut -d'"' -f4 || true
}

check3() {
  local base avx2 ymm_v ymm_b any_v any_b n b v
  base=$(bench_exe "")
  avx2=$(bench_exe "--features avx2-scan")
  test -x "$base" || {
    echo "no base bench binary" >&2
    return 1
  }
  test -x "$avx2" || {
    echo "no avx2 bench binary" >&2
    return 1
  }
  cp "$base" "$scratch/ingest2-base"
  cp "$avx2" "$scratch/ingest2-avx2"

  ymm_v=$("$OBJDUMP" -d "$scratch/ingest2-avx2" | grep -cE 'vpmovmskb[[:space:]]+%ymm' || true)
  ymm_b=$("$OBJDUMP" -d "$scratch/ingest2-base" | grep -cE 'vpmovmskb[[:space:]]+%ymm' || true)
  any_v=$("$OBJDUMP" -d "$scratch/ingest2-avx2" | grep -c 'pmovmskb' || true)
  any_b=$("$OBJDUMP" -d "$scratch/ingest2-base" | grep -c 'pmovmskb' || true)
  echo "movemask, ymm (32-byte): avx2 arm $ymm_v, base arm $ymm_b"
  echo "movemask, any width:     avx2 arm $any_v, base arm $any_b"

  # Byte-identical binaries are the same program, whatever the features said.
  if cmp -s "$scratch/ingest2-base" "$scratch/ingest2-avx2"; then
    echo "ARMS ARE THE SAME BINARY -- the feature changed nothing" >&2
    return 1
  fi
  if ! live "$ymm_v" "$ymm_b"; then
    echo "AVX2 ARM IS NOT LIVE: $ymm_v 32-byte movemasks against the base's $ymm_b, so the feature did not reach the scan" >&2
    return 1
  fi
  echo "avx2 arm is live and the two binaries differ"

  # Cells must be identical or the two arms are not doing the same work. **The
  # emptiness guard is the point**: if the grep stops matching, both sides are
  # the empty string, `!=` is false, and the loop would report a match having
  # compared nothing.
  for n in cat git-log less cat-cjk btop; do
    b=$("$scratch/ingest2-base" "$n" 2 | grep -oE 'cells [0-9]+' | head -1 || true)
    v=$("$scratch/ingest2-avx2" "$n" 2 | grep -oE 'cells [0-9]+' | head -1 || true)
    if [ -z "$b" ] || [ -z "$v" ]; then
      echo "NO CELL COUNT PARSED on $n -- this check compared nothing" >&2
      return 1
    fi
    if [ "$b" != "$v" ]; then
      echo "CELLS DIFFER on $n: $b vs $v -- the arms are different programs" >&2
      return 1
    fi
  done
  echo "ok: cells identical on all five; the avx2 arm agrees, passes and is live on this host"
}

# **Each check run, in its own subshell under `set -e`**, whatever came
# before it; non-zero, naming each that failed, if any did. The subshell is
# a command of its own, with the caller's `-e` off around it: bash ignores
# `set -e` inside a subshell that is an `if` condition or part of a `&&` or
# `||` list, so a check whose first command failed would run on and pass.
run_checks() {
  local failed="" c rc errexit=""
  case $- in *e*) errexit=1 ;; esac
  for c in "$@"; do
    echo "=== $c"
    set +e
    (
      set -euo pipefail
      "$c"
    )
    rc=$?
    [ -n "$errexit" ] && set -e
    [ "$rc" -eq 0 ] || failed="$failed $c"
  done
  if [ -n "$failed" ]; then
    echo "test:x86-avx2 FAILED:$failed" >&2
    return 1
  fi
}

main() {
  if [ "$#" -ne 0 ]; then
    echo "usage: $0" >&2
    exit 2
  fi
  cd "$(dirname "$0")/../.." || exit 1
  local arch
  arch=$(uname -m)
  [ "$arch" = x86_64 ] || refuse "this host is $arch, not x86_64"
  [ -r /proc/cpuinfo ] || refuse "no /proc/cpuinfo on this $arch host"
  grep -qw avx2 /proc/cpuinfo || refuse "/proc/cpuinfo does not list avx2"
  OBJDUMP=${OBJDUMP:-objdump}
  command -v "$OBJDUMP" >/dev/null || refuse "no $OBJDUMP to disassemble the arms with"
  scratch=$(mktemp -d)
  trap 'rm -rf "$scratch"' EXIT
  run_checks check1 check2 check3
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  set -euo pipefail
  main "$@"
fi
