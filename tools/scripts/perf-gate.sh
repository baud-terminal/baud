#!/usr/bin/env bash
# **The deterministic counter gate**: `bash tools/scripts/perf-gate.sh`, or
# `devenv tasks run test:perf-gate`.
#
# Every gated entry in `tools/perf-budgets.toml` is a hot path (the function a
# `--toggle-collect` names), an input to the bench that reaches it, and an
# ISA. callgrind counts the instructions (`Ir`) executed inside that
# function and its callees and, with its cache simulation on, the simulated
# D1 and LL misses, at this host's L1d and LL geometry with a 64-byte line
# and again with a 128-byte one. Those counts are the same on every run of
# one binary, so the gate needs no repetition and no quiet machine, and a
# red is a change in the work done:
#
#   * `Ir` on an ingest entry fails above +0.2% and warns on any rise; on
#     any other path it fails above +0.5% and warns above +0.1%.
#   * `D1mw`, simulated D1 write misses, is read on an ingest entry as a
#     band over the heap plants (below), at each geometry, and fails when
#     the band's lowest figure is above the baseline band's highest by more
#     than 2%: "the band rose: reason required". Bands that overlap are no
#     result, said as such, and pass. On any other path `D1mw` is one
#     figure and fails above +2%, "reason required". Either way the commit
#     that raises it gives its reason, recorded beside the new figure.
#   * `D1mr`, `LLmr` and `LLmw` warn above +2% and never fail; an ingest
#     entry reads them at the first plant.
#
# **An ingest entry runs at every heap plant** that `plants` in
# `tools/perf-budgets.toml` lists, at both geometries. `BAUD_BENCH_PLANT=<plant>`
# in the bench's environment makes one live allocation of that many bytes
# before the bench's buffers, so the heap places them elsewhere while the
# code stays as it is; a plant is five digits, so the environment is one
# length on every run. Where an entry's hot set exceeds L1, `D1mw` follows
# where its buffers and the binary's static data fall in the sets: the
# plants read that spread on one binary, and a rise in write misses per
# byte lifts every plant. Every figure but `D1mw` is read at the first
# plant, `00000`, which allocates nothing; a heap placement can move a few
# instructions, so `Ir` is not compared across plants.
#
# **A counter the gate reads is never taken as 0.** A run whose counts
# lack one fails its entry, "MISSING <counter> at plant <plant>", and a
# plant run whose output does not say `heap plant: <plant> B` fails its
# entry: a run the plant never reached is not a reading of it.
#
# **A passing run never moves a baseline.** Only `--update` writes one: a
# figure or band that fell is written with the one it fell from, and one
# that rose or is new needs `--reason "<why>"`, or the update refuses. A
# band rose when either end rose. A check run that measures more than 0.5%
# below a baseline, or a band whose highest is that far below the
# baseline's highest, says "ratchet available".
#
# **Each entry proves it measured something** before any budget applies:
# `Ir` above zero, the toggled function called at least once, and `Ir`
# within half and twice its baseline. A function the compiler inlined
# collects nothing and would otherwise pass every budget. Once a run, a
# control names a function no binary has, and must fail exactly that check,
# or the gate itself fails.
#
# **Each entry is run twice at the 64-byte line**, an ingest entry at the
# first plant, and every count must be
# identical, else the entry fails as not deterministic: an input whose
# count moves run to run is fixed or left ungated, never given slack. Three
# things the counts would otherwise follow are held fixed for every run:
#
#   * the environment (`env -i`): ingest2's timed loop reads a variable each
#     chunk, and the lookup's cost grows with the environment it scans.
#     It holds `BAUD_MEASURES=counts`: counts do not move with load, so the
#     bench's guard reads none, and the bench prints no figure on the clock;
#   * the addresses (`setarch -R`): a randomised stack moves where the
#     loop's locals fall in a page, which changes which branch `memcmp`
#     takes and which D1 set a line maps to;
#   * the processes the bench can see (`unshare -p --mount-proc`): before
#     its loop the bench lists every process on the machine, and what that
#     listing allocated and freed moves where the loop's own allocations
#     land. In a namespace of its own the listing holds the bench alone,
#     and the bench copy runs as `./<bench>` so its path is the same length
#     wherever the gate runs.
#
# Ir is then the same in any checkout. The simulated miss counts also
# depend on the binary's layout, which the recording path compiled into the
# bench moves, so every tree builds at one path (below).
#
# `PERF_GATE_CPUS`, when set, pins every run to those CPUs (`taskset -c`).
# Work files go under `$TMPDIR`. `PERF_GATE_DROP=<plant>,<line>,<counter>`
# removes that counter from the counts of every ingest run at that plant
# and line size, after callgrind writes them; `tools/scripts/perf-gate-controls.sh`
# sets it to show that a missing counter fails. A run that exits non-zero
# fails its entry with the reason valgrind gives, and its log is kept under
# `PERF_GATE_KEEP` (`<PERF_GATE_TREE>.failed` when unset), which this script
# never empties.
set -eu
shopt -s nullglob
cd "$(dirname "$0")/../.." || exit 1
root=$(pwd -P)
budgets="$root/tools/perf-budgets.toml"

usage() {
  echo 'usage: tools/scripts/perf-gate.sh [--update [--reason "<why>"]]' >&2
  exit 2
}

update=0
reason=""
while [ $# -gt 0 ]; do
  case "$1" in
  --update) update=1 ;;
  --reason)
    [ $# -ge 2 ] || usage
    reason=$2
    shift
    ;;
  *) usage ;;
  esac
  shift
done

isa=$(uname -m)
vg=$(command -v valgrind) || {
  echo "perf-gate: FAIL: no valgrind on PATH (the devenv shell carries it)"
  exit 1
}
if ! ns=$(command -v unshare) || ! sa=$(command -v setarch); then
  echo "perf-gate: FAIL: no unshare or setarch on PATH (util-linux)"
  exit 1
fi
"$ns" -r -p -f --mount-proc true || {
  echo "perf-gate: FAIL: this host refuses an unprivileged pid namespace"
  exit 1
}
pin=()
if [ -n "${PERF_GATE_CPUS:-}" ]; then
  pin=(taskset -c "$PERF_GATE_CPUS")
fi

work=$(mktemp -d "${TMPDIR:-/tmp}/perf-gate.XXXXXX")
trap 'rm -rf "$work"' EXIT

# --- the configuration -------------------------------------------------------

# `[gate.<path>]` tables, one line a key: `fn`, the function the toggle
# names; `kind`, which budgets apply; `bench`, the `baud-bench` bench that
# runs it (`ingest2` when absent); `args`, the bench's arguments, with
# `{input}` standing for each input (`{input} 1` when absent); and
# `inputs`, a string of space-separated inputs or an array of strings, each
# of which may hold spaces. An input's key in its entry is the input with
# each space a `-`. Inputs come out `;`-separated.
gates=$(awk '
  /^\[gate\./ { p = $0; sub(/^\[gate\./, "", p); sub(/\].*$/, "", p); next }
  /^\[/ { p = ""; next }
  p != "" && /^(fn|kind|bench|args|inputs) *=/ {
    k = $0; sub(/ *=.*/, "", k)
    v = $0; sub(/^[^=]*= */, "", v); sub(/ *$/, "", v)
    if (v ~ /^\[/) {
      out = ""
      while (match(v, /"[^"]*"/)) {
        out = out (out == "" ? "" : ";") substr(v, RSTART + 1, RLENGTH - 2)
        v = substr(v, RSTART + RLENGTH)
      }
      v = out
    } else {
      sub(/^"/, "", v); sub(/"$/, "", v)
      if (k == "inputs") gsub(/ +/, ";", v)
    }
    print p "\t" k "\t" v
  }' "$budgets")
gate_get() { printf '%s\n' "$gates" | awk -F'\t' -v p="$1" -v k="$2" '$1 == p && $2 == k { print $3 }'; }
paths=$(printf '%s\n' "$gates" | awk -F'\t' 'NF == 3 { print $1 }' | awk '!seen[$0]++')
[ -n "$paths" ] || {
  echo "perf-gate: FAIL: $budgets names no [gate.*] table"
  exit 1
}

# The baselines: one `<path>.<input>.<isa>\t<counter>\t<value>` a line,
# where a band's value is `<lo> <hi>`.
baselines=$(awk '
  /^\[/ { s = $0; sub(/^\[/, "", s); sub(/\].*$/, "", s); next }
  s !~ /^gate\./ && /^[a-z0-9.-]+ *= *\{ *value *= *[0-9]+/ {
    k = $0; sub(/ *=.*/, "", k)
    v = $0; sub(/^[^{]*\{ *value *= */, "", v); sub(/[^0-9].*$/, "", v)
    print s "\t" k "\t" v
  }
  s !~ /^gate\./ && /^[a-z0-9.-]+ *= *\{ *lo *= *[0-9]+ *, *hi *= *[0-9]+/ {
    k = $0; sub(/ *=.*/, "", k)
    lo = $0; sub(/^[^{]*\{ *lo *= */, "", lo); sub(/[^0-9].*$/, "", lo)
    hi = $0; sub(/^[^{]*\{ *lo *= *[0-9]+ *, *hi *= */, "", hi); sub(/[^0-9].*$/, "", hi)
    print s "\t" k "\t" lo " " hi
  }' "$budgets")
base_get() { printf '%s\n' "$baselines" | awk -F'\t' -v s="$1" -v k="$2" '$1 == s && $2 == k { print $3 }'; }

# The simulated geometry: this host's L1d and last level, from the file.
d1=$(awk -F'"' '/^d1 *=/ { print $2 }' "$budgets")
ll=$(awk -F'"' '/^ll *=/ { print $2 }' "$budgets")
[ -n "$d1" ] && [ -n "$ll" ] || {
  echo "perf-gate: FAIL: $budgets names no d1 or ll geometry"
  exit 1
}

# The heap plants every ingest entry runs at, `00000` first.
read -ra plants <<<"$(awk -F'"' '/^plants *=/ { print $2 }' "$budgets")"
[ "${#plants[@]}" -gt 0 ] && [ "${plants[0]}" = 00000 ] || {
  echo "perf-gate: FAIL: $budgets names no plants, or its first is not 00000"
  exit 1
}
for p in "${plants[@]}"; do
  case "$p" in
  [0-9][0-9][0-9][0-9][0-9]) ;;
  *)
    echo "perf-gate: FAIL: plant '$p' in $budgets is not five digits"
    exit 1
    ;;
  esac
done
drop=${PERF_GATE_DROP:-}

if [ "$update" -eq 0 ] && ! printf '%s\n' "$baselines" | awk -F'\t' -v i="$isa" '{ n = split($1, a, "."); if (a[n] == i) f = 1 } END { exit !f }'; then
  echo "perf-gate: FAIL: no baselines for $isa in tools/perf-budgets.toml; tier D gates x86_64"
  exit 1
fi

# --- the build ---------------------------------------------------------------

# **Every tree builds at one path.** The benches compile in the path of the
# recordings they read (`CARGO_MANIFEST_DIR`), and a path of another length
# moves the binary's layout: the simulated miss counts follow it by a few
# percent on the small entries. So the tree's files (tracked, and new ones
# git does not ignore) are copied, times kept, to one fixed place, and the
# benches are built there, with that place's own target directory kept
# between runs. `PERF_GATE_TREE` names another place.
#
# **One run holds the place at a time.** A second run on the same place
# waits for the first to exit, so neither replaces the other's tree or
# binaries mid-measurement. The lock is held until this script exits.
tree=${PERF_GATE_TREE:-${XDG_CACHE_HOME:-$HOME/.cache}/baud-perf-gate}
kept=${PERF_GATE_KEEP:-$tree.failed}
mkdir -p "$tree"
exec 9>"$tree.lock"
if ! flock -n 9; then
  echo "perf-gate: another run holds $tree; waiting for it to finish"
  flock 9
fi
rm -rf "$tree/src"
mkdir -p "$tree/src"
git -C "$root" ls-files -z --cached --others --exclude-standard |
  tar --null -C "$root" -T - -cf - | tar -C "$tree/src" -xf -

# Each bench a gate names, built once; ingest2 always, which the control
# runs on.
declare -A binof
for b in ingest2 $(for p in $paths; do gate_get "$p" bench; done); do
  [ -z "${binof[$b]:-}" ] || continue
  echo "perf-gate: building the $b bench"
  build=$(cd "$tree/src" && CARGO_TARGET_DIR="$tree/target" cargo bench -p baud-bench --bench "$b" --no-run 2>&1) || {
    printf '%s\n' "$build"
    echo "perf-gate: FAIL: the $b bench did not build"
    exit 1
  }
  bin=$(printf '%s\n' "$build" | sed -n "s/^ *Executable benches\/$b\.rs (\(.*\))\$/\1/p" | tail -n 1)
  case "$bin" in
  /*) ;;
  *) bin="$tree/src/$bin" ;;
  esac
  [ -x "$bin" ] || {
    printf '%s\n' "$build"
    echo "perf-gate: FAIL: no $b executable in cargo's output"
    exit 1
  }
  binof[$b]=$bin
done
rustc_now=$(rustc -V)
rustc_set=$(awk -F'"' '/^rustc *=/ { print $2 }' "$budgets")
if [ -n "$rustc_set" ] && [ "$rustc_set" != "$rustc_now" ]; then
  echo "perf-gate: note: baselines set with $rustc_set, this run is $rustc_now"
fi

# --- the runs ----------------------------------------------------------------

# run <dir> <fn> <bench> <line> <plant> <arg>...: one callgrind run of
# <bench> with <arg>..., and `BAUD_BENCH_PLANT=<plant>` in its environment
# unless <plant> is `-`; its counts left in <dir>/counts as
# `<event> <count>` lines, the calls into <fn> in <dir>/calls, and the
# bench's exit status in <dir>/status.
#
# The bench is pid 1 of its own namespace, so its profile is `cg.1` (and
# `cg.1.<n>` for any part it dumps). The child it spawns after the timed
# loop is forked under valgrind and writes `cg.2`, a copy of its parent's
# counts, which is left out: summing the two would count the loop twice.
run() {
  local dir=$1 fn=$2 bench=$3 line=$4 plant=$5 status=0 planted=()
  shift 5
  [ "$plant" = - ] || planted=("BAUD_BENCH_PLANT=$plant")
  mkdir -p "$dir"
  cp "${binof[$bench]}" "$dir/$bench"
  (
    cd "$dir"
    exec "${pin[@]}" "$ns" -r -p -f --mount-proc env -i PATH=/usr/bin:/bin LC_ALL=C BAUD_MEASURES=counts "${planted[@]}" \
      "$sa" -R "$vg" --tool=callgrind \
      --toggle-collect="$fn" --cache-sim=yes \
      --D1="$d1,$line" --LL="$ll,$line" \
      --callgrind-out-file="$dir/cg.%p" "./$bench" "$@"
  ) >"$dir/log" 2>&1 || status=$?
  echo "$status" >"$dir/status"
  awk '
    /^events:/ { n = split($0, e, " "); next }
    /^(summary|totals):/ {
      if (seen[FILENAME]++) next
      split($0, v, " ")
      for (i = 2; i <= n; i++) sum[e[i]] += v[i]
    }
    END { for (k in sum) print k, sum[k] }' "$dir/cg.1" "$dir"/cg.1.* </dev/null >"$dir/counts" 2>/dev/null || true
  if [ -n "$drop" ] && [ "$plant" != - ] && [ "${drop%,*}" = "$plant,$line" ]; then
    awk -v k="${drop##*,}" '$1 != k' "$dir/counts" >"$dir/counts.kept"
    mv "$dir/counts.kept" "$dir/counts"
  fi
  awk -v name="$fn" '
    {
      if (match($0, /^c?fn=\([0-9]+\) /)) {
        id = substr($0, index($0, "(") + 1); sub(/\).*/, "", id)
        if (substr($0, RLENGTH + 1) == name) want[id] = 1
      }
      if (prev != "" && /^calls=/) { c = $0; sub(/^calls=/, "", c); sub(/ .*/, "", c); calls += c }
      prev = ""
      if (/^cfn=\(/) { id = substr($0, 6); sub(/\).*/, "", id); if (id in want) prev = id }
    }
    END { print calls + 0 }' "$dir/cg.1" 2>/dev/null >"$dir/calls" || echo 0 >"$dir/calls"
  rm -f "${dir:?}/$bench"
}
# count <dir> <event>: the event's count, or nothing when the run's counts
# lack it; `absent` names the first of its events a run lacks.
count() { awk -v k="$2" '$1 == k { print $2 }' "$1/counts"; }
absent() {
  local dir=$1 k
  shift
  for k; do
    grep -q "^$k " "$dir/counts" 2>/dev/null || {
      echo "$k"
      return
    }
  done
}

# Every run of every entry, and the control, at once: the counts do not
# depend on what else the machine is doing. An ingest entry's runs are
# `p<plant>-<line>`, and `aa64` repeats the first plant at the 64-byte
# line; any other entry's are `a64`, `b64` and `a128`.
entries=()
for p in $paths; do
  fn=$(gate_get "$p" fn)
  bench=$(gate_get "$p" bench)
  bench=${bench:-ingest2}
  template=$(gate_get "$p" args)
  template=${template:-"{input} 1"}
  IFS=';' read -ra inputs <<<"$(gate_get "$p" inputs)"
  for input in "${inputs[@]}"; do
    e="$p.${input// /-}.$isa"
    entries+=("$e")
    read -ra argv <<<"${template//\{input\}/$input}"
    if [ "$(gate_get "$p" kind)" = ingest ]; then
      for plant in "${plants[@]}"; do
        run "$work/$e/p$plant-64" "$fn" "$bench" 64 "$plant" "${argv[@]}" &
        run "$work/$e/p$plant-128" "$fn" "$bench" 128 "$plant" "${argv[@]}" &
      done
      run "$work/$e/aa64" "$fn" "$bench" 64 "${plants[0]}" "${argv[@]}" &
    else
      run "$work/$e/a64" "$fn" "$bench" 64 - "${argv[@]}" &
      run "$work/$e/b64" "$fn" "$bench" 64 - "${argv[@]}" &
      run "$work/$e/a128" "$fn" "$bench" 128 - "${argv[@]}" &
    fi
  done
done
control_fn=perf_gate::no_such_fn
run "$work/control" "$control_fn" ingest2 64 - less 1 &
wait

# --- the verdicts ------------------------------------------------------------

# `hard` counts what no figure can answer -- a vacuous or nondeterministic
# entry, or a control that passed -- and `fails` adds the budgets.
hard=0
fails=0
warns=0
ratchets=0
pct() { awk -v m="$1" -v b="$2" 'BEGIN { if (b == 0) print (m == 0 ? 0 : 100); else printf "%+.3f", (m - b) * 100 / b }'; }
above() { awk -v x="$1" -v t="$2" 'BEGIN { exit !(x + 0 > t + 0) }'; }

# died <dir>: why a run that exited non-zero stopped, and where its log is
# kept. valgrind gives its own reasons on its `==<pid>==` lines: the signal
# the bench died of, or why valgrind gave up. As pid 1 of its namespace
# valgrind cannot die of the bench's signal itself, so a bench killed by
# one exits 1. A run with none of these gives the last line of valgrind's
# and the bench's.
died() {
  local dir=$1 s why name to
  s=$(cat "$dir/status")
  why=$(sed -n \
    -e 's/^==[0-9]*== *Process terminating with default action of \(signal [0-9]* ([A-Z0-9]*)\).*/died of \1/p' \
    -e 's/^==[0-9]*== *\(Valgrind.s memory management: out of memory\).*/\1/p' \
    -e 's/^==[0-9]*== *\(Valgrind cannot continue\).*/\1/p' \
    -e "s/^\(valgrind: the 'impossible' happened\).*/\1/p" \
    "$dir/log" | awk '!seen[$0]++' | paste -sd ';' - | sed 's/;/; /g')
  if [ "$s" -gt 128 ]; then
    name=$(kill -l "$((s - 128))" 2>/dev/null || echo "$((s - 128))")
    why="${why:+$why; }killed by SIG$name"
  fi
  [ -n "$why" ] ||
    why="valgrind's last line: $(grep '^==' "$dir/log" | tail -n 1); the bench's: $(grep -v '^==' "$dir/log" | tail -n 1)"
  name=${dir#"$work"/}
  mkdir -p "$kept"
  to="$kept/$(date +%Y%m%dT%H%M%S)-${name//\//-}.log"
  cp "$dir/log" "$to"
  echo "the bench exited $s ($why); log kept at $to"
}

# vacuous <dir>: why the run measured nothing, or nothing when it measured.
vacuous() {
  local dir=$1
  if [ "$(cat "$dir/status")" -ne 0 ]; then
    died "$dir"
  elif [ -n "$(absent "$dir" Ir)" ]; then
    echo "MISSING Ir"
  elif [ "$(count "$dir" Ir)" -eq 0 ]; then
    echo "Ir is 0"
  elif [ "$(cat "$dir/calls")" -eq 0 ]; then
    echo "the toggled function was never called"
  fi
}

# counter <name> <measured>: one figure of entry `$e` against its baseline,
# raising `$status`. `Ir` and `D1mw` always go in `$note`; a warn-only
# counter goes in when it is flagged.
counter() {
  local k=$1 m=$2 b d flag=""
  measured+=("$e	$k	$m")
  b=$(base_get "$e" "$k")
  if [ -z "$b" ]; then
    note="$note $k=$m (no baseline)"
    [ "$update" -eq 1 ] || status=FAIL
    return
  fi
  d=$(pct "$m" "$b")
  if above -0.5 "$d"; then
    flag=" [ratchet available]"
    ratchets=$((ratchets + 1))
  fi
  case "$k" in
  ir)
    if [ "$kind" = ingest ]; then
      if above "$d" 0.2; then
        flag="$flag [Ir over +0.2%]"
        status=FAIL
      elif [ "$m" -gt "$b" ]; then
        flag="$flag [warn: Ir rose]"
        [ "$status" = FAIL ] || status=WARN
      fi
    elif above "$d" 0.5; then
      flag="$flag [Ir over +0.5%]"
      status=FAIL
    elif above "$d" 0.1; then
      flag="$flag [warn: Ir over +0.1%]"
      [ "$status" = FAIL ] || status=WARN
    fi
    ;;
  *.d1mw)
    if above "$d" 2; then
      flag="$flag [D1mw: reason required]"
      status=FAIL
    fi
    ;;
  *)
    if above "$d" 2; then
      flag="$flag [warn: over +2%]"
      [ "$status" = FAIL ] || status=WARN
    fi
    ;;
  esac
  case "$k" in
  ir | *.d1mw) note="$note $k=$m ($d%)$flag" ;;
  *) [ -z "$flag" ] || note="$note $k=$m ($d%)$flag" ;;
  esac
}

# band <name> <lo> <hi>: an ingest entry's D1mw band, its lowest and
# highest over the plants, against its baseline band, raising `$status`.
band() {
  local k=$1 lo=$2 hi=$3 b blo bhi d flag=""
  measured+=("$e	$k	$lo $hi")
  b=$(base_get "$e" "$k")
  read -r blo bhi <<<"$b"
  if [ -z "$bhi" ]; then
    note="$note $k=[$lo,$hi] (no baseline band)"
    [ "$update" -eq 1 ] || status=FAIL
    return
  fi
  d=$(pct "$lo" "$bhi")
  if above -0.5 "$(pct "$hi" "$bhi")"; then
    flag=" [ratchet available]"
    ratchets=$((ratchets + 1))
  fi
  if above "$d" 2; then
    flag="$flag [D1mw: the band rose: reason required, lowest $d% above the baseline's highest]"
    status=FAIL
  elif [ "$lo" -le "$bhi" ] && [ "$hi" -ge "$blo" ]; then
    flag="$flag no result (bands overlap)"
  elif [ "$lo" -gt "$bhi" ]; then
    flag="$flag above the baseline band, lowest $d% above its highest, inside +2%"
  else
    flag="$flag below the baseline band"
  fi
  note="$note $k=[$lo,$hi] (baseline [$blo,$bhi])$flag"
}

# The control first: a gate whose vacuity check cannot fire checks nothing. A
# toggle on a function that does not exist reads Ir 0 from a bench that ran and
# exited 0; a bench that did not run is a broken environment, not that answer.
why=$(vacuous "$work/control")
if [ -z "$why" ]; then
  echo "FAIL control: --toggle-collect=$control_fn measured $(count "$work/control" Ir) Ir and passed the vacuity check"
  hard=$((hard + 1))
elif [ "$why" != "Ir is 0" ]; then
  echo "FAIL control: --toggle-collect=$control_fn did not read Ir 0 from a run that finished ($why)"
  hard=$((hard + 1))
else
  echo "ok   control: --toggle-collect=$control_fn is vacuous ($why)"
fi

measured=()
events=(Ir D1mr D1mw ILmr DLmr DLmw)
for e in "${entries[@]}"; do
  p=${e%%.*}
  kind=$(gate_get "$p" kind)
  bad=""
  if [ "$kind" = ingest ]; then
    first="$work/$e/p${plants[0]}-64"
    first128="$work/$e/p${plants[0]}-128"
    twin="$work/$e/aa64"
    runs=()
    for plant in "${plants[@]}"; do runs+=("p$plant-64" "p$plant-128"); done
    runs+=(aa64)
  else
    first="$work/$e/a64"
    first128="$work/$e/a128"
    twin="$work/$e/b64"
    runs=(a64 b64 a128)
  fi
  for r in "${runs[@]}"; do
    [ -z "$bad" ] || break
    dir="$work/$e/$r"
    why=$(vacuous "$dir")
    if [ -n "$why" ]; then
      bad="vacuous at $r: $why"
      continue
    fi
    if [ "$kind" = ingest ]; then
      case "$r" in
      aa64) plant=${plants[0]} at="plant $plant (d1-64, the repeat)" ;;
      *)
        plant=${r#p}
        plant=${plant%-*}
        at="plant $plant (d1-${r#*-})"
        ;;
      esac
      if ! grep -qx "heap plant: $plant B" "$dir/log"; then
        bad="the run at $at does not say 'heap plant: $plant B': $(grep '^heap plant:' "$dir/log" || echo 'it has no heap plant line')"
        continue
      fi
    else
      at=$r
    fi
    k=$(absent "$dir" "${events[@]}")
    [ -z "$k" ] || bad="MISSING $k at $at"
  done
  b=$(base_get "$e" ir)
  if [ -z "$bad" ]; then
    ir=$(count "$first" Ir)
    if ! cmp -s "$first/counts" "$twin/counts"; then
      bad="not deterministic: two runs at the 64-byte line differ ($(tr '\n' ' ' <"$first/counts")/ $(tr '\n' ' ' <"$twin/counts"))"
    elif [ "$ir" -ne "$(count "$first128" Ir)" ]; then
      bad="not deterministic: Ir at the 128-byte line is $(count "$first128" Ir), at 64 $ir"
    fi
  fi
  if [ -z "$bad" ] && [ -n "$b" ] && { [ "$ir" -gt "$((b * 2))" ] || [ "$((ir * 2))" -lt "$b" ]; }; then
    # A count far from its baseline is a different measurement, not a
    # regression to budget.
    bad="vacuous: Ir $ir is outside half to twice its baseline $b"
  fi
  if [ -n "$bad" ]; then
    echo "FAIL $e: $bad"
    hard=$((hard + 1))
    continue
  fi

  status=ok
  note=""
  counter ir "$ir"
  for g in 64 128; do
    if [ "$kind" = ingest ]; then
      dir="$work/$e/p${plants[0]}-$g"
      lo=""
      hi=""
      for plant in "${plants[@]}"; do
        m=$(count "$work/$e/p$plant-$g" D1mw)
        if [ -z "$lo" ] || [ "$m" -lt "$lo" ]; then lo=$m; fi
        if [ -z "$hi" ] || [ "$m" -gt "$hi" ]; then hi=$m; fi
      done
      band "d1-$g.d1mw" "$lo" "$hi"
    else
      dir="$work/$e/a$g"
      counter "d1-$g.d1mw" "$(count "$dir" D1mw)"
    fi
    counter "d1-$g.d1mr" "$(count "$dir" D1mr)"
    counter "d1-$g.llmr" "$(($(count "$dir" ILmr) + $(count "$dir" DLmr)))"
    counter "d1-$g.llmw" "$(count "$dir" DLmw)"
  done
  case "$status" in
  FAIL) fails=$((fails + 1)) ;;
  WARN) warns=$((warns + 1)) ;;
  ok) status="ok  " ;;
  esac
  echo "$status $e: calls=$(cat "$first/calls"), A/A identical;$note"
done
fails=$((fails + hard))

# --- the update --------------------------------------------------------------

if [ "$update" -eq 1 ]; then
  if [ "$hard" -gt 0 ]; then
    echo "perf-gate: no update: an entry above failed a check no figure can answer"
    exit 1
  fi
  set_in="$(date +%F), $(uname -m)"
  raised=0
  out="$work/perf-budgets.toml"
  # Everything above the first entry stays as written, and other ISAs'
  # entries are kept as they are; the rustc line is the one this run built
  # with. Blank lines are held until a line follows them, so each entry
  # below is set off by exactly one.
  awk -v r="$rustc_now" -v i="$isa" '
    /^\[/ && $0 !~ /^\[gate\./ {
      s = $0; sub(/^\[/, "", s); sub(/\].*$/, "", s)
      n = split(s, a, "."); keep = (a[n] != i); started = 1
    }
    started && !keep { next }
    /^$/ { blank++; next }
    /^rustc *=/ { $0 = "rustc = \"" r "\"" }
    { for (; blank > 0; blank--) print ""; print }' "$budgets" >"$out"
  last=""
  for m in "${measured[@]}"; do
    IFS='	' read -r e k v <<<"$m"
    if [ "$e" != "$last" ]; then
      echo >>"$out"
      echo "[$e]" >>"$out"
      last=$e
    fi
    b=$(base_get "$e" "$k")
    if [ "$b" = "$v" ]; then
      awk -v s="$e" -v k="$k" '
        /^\[/ { c = $0; sub(/^\[/, "", c); sub(/\].*$/, "", c); next }
        c == s { x = $0; sub(/ *=.*/, "", x); if (x == k) { print; exit } }' "$budgets" >>"$out"
      continue
    fi
    case "$v" in
    *" "*)
      # A band: lowered when neither end rose against a baseline band.
      read -r lo hi <<<"$v"
      read -r blo bhi <<<"$b"
      if [ -n "$bhi" ] && [ "$lo" -le "$blo" ] && [ "$hi" -le "$bhi" ]; then
        why="ratchet: lowered from [$blo,$bhi]"
      else
        raised=$((raised + 1))
        why=$reason
      fi
      fields="lo = $lo, hi = $hi"
      ;;
    *)
      if [ -n "$b" ] && [ "$v" -lt "$b" ]; then
        why="ratchet: lowered from $b"
      else
        raised=$((raised + 1))
        why=$reason
      fi
      fields="value = $v"
      ;;
    esac
    why=${why//\\/\\\\}
    why=${why//\"/\\\"}
    echo "$k = { $fields, set_in = \"$set_in\", reason = \"$why\" }" >>"$out"
  done
  if [ "$raised" -gt 0 ] && [ -z "$reason" ]; then
    echo "perf-gate: no update: $raised figures rose or are new, and a raise needs --reason \"<why>\""
    exit 1
  fi
  cp "$out" "$budgets"
  echo "perf-gate: wrote tools/perf-budgets.toml ($raised raised or new, $ratchets ratchets)"
  exit 0
fi

if [ "$fails" -gt 0 ]; then
  echo "perf-gate: FAIL ($fails failing, $warns warning, of ${#entries[@]} entries)"
  exit 1
fi
echo "perf-gate: PASS (${#entries[@]} entries, $warns warning, $ratchets ratchets available)"
