#!/usr/bin/env bash
# **The counter gate's controls**: `bash scripts/perf-gate-controls.sh
# [aa] [rise] [layout] [missing] [old <rev>]`, all but `old` when none is
# named. Each shows one thing the gate (`scripts/perf-gate.sh`) must do,
# against a baseline it sets itself:
#
#   * `aa`: the same binary measured twice gives identical D1mw bands on
#     every ingest entry, and the gate passes.
#   * `rise`: a bench that stores one extra 64-byte line per input byte,
#     into a buffer the size of L1, lifts every plant: each ingest entry's
#     band clears the baseline's by more than 2%, and the gate fails with
#     "the band rose".
#   * `layout`: more static data in the bench, `PERF_GATE_CONTROLS_PAD`
#     bytes (256 when unset), moves where the binary's data falls in the
#     sets and nothing else: no band rises, at least one reads "no result
#     (bands overlap)", and the gate passes.
#   * `missing`: a plant run whose counts lack D1mw (`PERF_GATE_DROP`) fails
#     every ingest entry with "MISSING D1mw at plant 00200".
#   * `old <rev>`: the gate as it is at <rev>, baseline set and then run on
#     the `layout` bench, to show what that gate reads of a layout move.
#
# The tree (its tracked files and the new ones git does not ignore, as the
# gate copies them) is copied to a scratch directory under `$TMPDIR`, and
# the bench changes are made there, never here. The gate builds at
# `PERF_GATE_TREE` as it always does, so the baseline and every control
# build at one path. Logs are kept in the scratch directory, whose path is
# printed. Exits non-zero when any control did not do what it shows.
set -eu
cd "$(dirname "$0")/.." || exit 1
pad=${PERF_GATE_CONTROLS_PAD:-256}
case "$pad" in
'' | *[!0-9]*)
  echo "controls: PERF_GATE_CONTROLS_PAD=$pad is not a byte count" >&2
  exit 2
  ;;
esac
root=$(pwd -P)

controls=()
old=""
while [ $# -gt 0 ]; do
  case "$1" in
  aa | rise | layout | missing) controls+=("$1") ;;
  old)
    [ $# -ge 2 ] || {
      echo 'usage: scripts/perf-gate-controls.sh [aa] [rise] [layout] [missing] [old <rev>]' >&2
      exit 2
    }
    old=$2
    shift
    ;;
  *)
    echo 'usage: scripts/perf-gate-controls.sh [aa] [rise] [layout] [missing] [old <rev>]' >&2
    exit 2
    ;;
  esac
  shift
done
[ "${#controls[@]}" -gt 0 ] || [ -n "$old" ] || controls=(aa rise layout missing)
want() {
  local c
  for c in "${controls[@]}"; do [ "$c" = "$1" ] && return 0; done
  return 1
}

scratch=$(mktemp -d "${TMPDIR:?TMPDIR names where the controls work}/perf-gate-controls.XXXXXX")
echo "controls: working in $scratch"
copy() {
  mkdir -p "$1"
  git -C "$root" ls-files -z --cached --others --exclude-standard |
    tar --null -C "$root" -T - -cf - | tar -C "$1" -xf -
  git -C "$1" init -q
  git -C "$1" add -A
}
src=$scratch/tree
copy "$src"
bench=baud-bench/benches/ingest2.rs
cp "$src/$bench" "$scratch/ingest2.rs.base"

# gate <tree> <log> <arg>...: one gate run in <tree>, its output in <log>
# and its exit status echoed.
gate() {
  local tree=$1 log=$2 s=0
  shift 2
  bash "$tree/scripts/perf-gate.sh" "$@" >"$log" 2>&1 || s=$?
  echo "$s"
}
# bands <log>: one `<entry> <counter>=[lo,hi]` line for each band a run
# measured.
bands() {
  awk '/^(ok  |WARN|FAIL) ingest\./ {
    e = $2; sub(/:$/, "", e)
    for (i = 3; i <= NF; i++) if ($i ~ /^d1-[0-9]+\.d1mw=\[/) print e, $i
  }' "$1"
}
# edit <file> <anchor> <text>: puts <text> after the one line that is
# exactly <anchor>, or stops when the anchor is not there exactly once.
edit() {
  local n
  n=$(grep -cxF -- "$2" "$1" || true)
  [ "$n" -eq 1 ] || {
    echo "controls: the anchor '$2' is in $1 $n times, not once"
    exit 1
  }
  awk -v a="$2" -v t="$3" '{ print } $0 == a { print t }' "$1" >"$1.edited"
  mv "$1.edited" "$1"
}
plant_rise() {
  edit "$src/$bench" '    let mut per_rep = Vec::with_capacity(reps);' \
    '    let mut l1 = vec![0u8; 49152];
    let mut line = 0usize;'
  edit "$src/$bench" '            t.write_at(BytePos::ZERO, &data[at..at + n]);' \
    '            for &byte in &data[at..at + n] {
                l1[line] = byte;
                line += 64;
                if line == 49152 {
                    line = 0;
                }
            }
            std::hint::black_box(&mut l1);'
}
plant_layout() {
  printf '\n#[used]\nstatic PLANTED_RODATA: [u8; %d] = [0x5a; %d];\n' "$pad" "$pad" >>"$1/$bench"
}
restore() { cp "$scratch/ingest2.rs.base" "$src/$bench"; }

failed=0
verdict() {
  if [ "$2" = ok ]; then
    echo "control $1: ok: $3"
  else
    echo "control $1: FAILED: $3"
    failed=$((failed + 1))
  fi
}

if [ "${#controls[@]}" -gt 0 ]; then
  s=$(gate "$src" "$scratch/base.log" --update --reason "controls: the base")
  [ "$s" -eq 0 ] || {
    cat "$scratch/base.log"
    echo "controls: the baseline run exited $s"
    exit 1
  }
  echo "controls: baseline set; bands:"
  bands "$scratch/base.log" | sed 's/^/  /'
fi

if want aa; then
  s=$(gate "$src" "$scratch/aa.log")
  grep -E '^(ok  |WARN|FAIL) ingest\.|^perf-gate:' "$scratch/aa.log" | sed 's/^/  /'
  if [ -z "$(bands "$scratch/aa.log")" ]; then
    verdict aa no "the run measured no band"
  elif [ "$s" -eq 0 ] && diff <(bands "$scratch/base.log") <(bands "$scratch/aa.log") >/dev/null; then
    verdict aa ok "every band identical to the baseline's, and the gate passed"
  else
    verdict aa no "exit $s; bands: $(diff <(bands "$scratch/base.log") <(bands "$scratch/aa.log") | tr '\n' ' ')"
  fi
fi

if want rise; then
  plant_rise
  s=$(gate "$src" "$scratch/rise.log")
  restore
  grep -E '^(ok  |WARN|FAIL) ingest\.|^perf-gate:' "$scratch/rise.log" | sed 's/^/  /'
  n=$(grep -c '^FAIL ingest\..*the band rose' "$scratch/rise.log" || true)
  all=$(grep -c '^\(ok  \|WARN\|FAIL\) ingest\.' "$scratch/rise.log" || true)
  if [ "$s" -ne 0 ] && [ "$n" -gt 0 ] && [ "$n" -eq "$all" ]; then
    verdict rise ok "the gate failed, the band rose on $n of $all ingest entries"
  else
    verdict rise no "exit $s, the band rose on $n of $all ingest entries"
  fi
fi

if want layout; then
  plant_layout "$src"
  s=$(gate "$src" "$scratch/layout.log")
  restore
  grep -E '^(ok  |WARN|FAIL) ingest\.|^perf-gate:' "$scratch/layout.log" | sed 's/^/  /'
  rose=$(grep -c 'the band rose' "$scratch/layout.log" || true)
  overlap=$(grep -o 'no result (bands overlap)' "$scratch/layout.log" | wc -l)
  moved=$(diff <(bands "$scratch/base.log") <(bands "$scratch/layout.log") | grep -c '^>' || true)
  if [ "$s" -eq 0 ] && [ "$rose" -eq 0 ] && [ "$overlap" -gt 0 ]; then
    verdict layout ok "the gate passed; $moved bands moved, none rose, $overlap read no result (bands overlap)"
  else
    verdict layout no "exit $s; $rose bands rose, $overlap overlapped, $moved moved"
  fi
fi

if want missing; then
  export PERF_GATE_DROP=00200,64,D1mw
  s=$(gate "$src" "$scratch/missing.log")
  unset PERF_GATE_DROP
  grep -E '^(ok  |WARN|FAIL) ingest\.|^perf-gate:' "$scratch/missing.log" | sed 's/^/  /'
  n=$(grep -c '^FAIL ingest\..*MISSING D1mw at plant 00200' "$scratch/missing.log" || true)
  all=$(grep -c '^\(ok  \|WARN\|FAIL\) ingest\.' "$scratch/missing.log" || true)
  if [ "$s" -ne 0 ] && [ "$n" -gt 0 ] && [ "$n" -eq "$all" ]; then
    verdict missing ok "the gate failed, MISSING D1mw at plant 00200 on $n of $all ingest entries"
  else
    verdict missing no "exit $s, MISSING on $n of $all ingest entries"
  fi
fi

if [ -n "$old" ]; then
  was=$scratch/old
  copy "$was"
  git -C "$root" show "$old:scripts/perf-gate.sh" >"$was/scripts/perf-gate.sh"
  git -C "$root" show "$old:perf-budgets.toml" >"$was/perf-budgets.toml"
  s=$(gate "$was" "$scratch/old-base.log" --update --reason "controls: the base")
  [ "$s" -eq 0 ] || {
    cat "$scratch/old-base.log"
    echo "controls: the baseline run of the gate at $old exited $s"
    exit 1
  }
  plant_layout "$was"
  s=$(gate "$was" "$scratch/old-layout.log")
  grep -E '^(ok  |WARN|FAIL) ingest\.|^perf-gate:' "$scratch/old-layout.log" | sed 's/^/  /'
  n=$(grep -c 'D1mw: reason required' "$scratch/old-layout.log" || true)
  echo "control old: the gate at $old on the layout bench exited $s, D1mw failed on $n figures"
fi

[ "$failed" -eq 0 ] || {
  echo "controls: $failed did not do what they show"
  exit 1
}
echo "controls: every control did what it shows"
