#!/usr/bin/env bash
# **The deterministic counter gate**: `bash scripts/perf-gate.sh`, or
# `devenv tasks run test:perf-gate`.
#
# Every gated entry in `perf-budgets.toml` is a hot path (the function a
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
#   * `D1mw`, simulated D1 write misses, fails above +2% at either
#     geometry, and the line says "reason required": the commit that
#     raises it gives its reason, recorded beside the new figure.
#   * `D1mr`, `LLmr` and `LLmw` warn above +2% and never fail.
#
# **A passing run never moves a baseline.** Only `--update` writes one: a
# figure that fell is written with the figure it fell from, and a figure
# that rose or is new needs `--reason "<why>"`, or the update refuses. A
# check run that measures more than 0.5% below a baseline says "ratchet
# available".
#
# **Each entry proves it measured something** before any budget applies:
# `Ir` above zero, the toggled function called at least once, and `Ir`
# within half and twice its baseline. A function the compiler inlined
# collects nothing and would otherwise pass every budget. Once a run, a
# control names a function no binary has, and must fail exactly that check,
# or the gate itself fails.
#
# **Each entry is run twice at the 64-byte line** and every count must be
# identical, else the entry fails as not deterministic: an input whose
# count moves run to run is fixed or left ungated, never given slack. Three
# things the counts would otherwise follow are held fixed for every run:
#
#   * the environment (`env -i`): ingest2's timed loop reads a variable each
#     chunk, and the lookup's cost grows with the environment it scans;
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
# depend on the heap's layout, which the recording path compiled into the
# bench moves: two checkouts at paths of different length differed by up
# to 0.8% in `d1mr` and 0.2% in `d1mw`, inside the budgets but not zero.
#
# `PERF_GATE_CPUS`, when set, pins every run to those CPUs (`taskset -c`).
# Work files go under `$TMPDIR`.
set -eu
shopt -s nullglob
cd "$(dirname "$0")/.." || exit 1
root=$(pwd -P)
budgets="$root/perf-budgets.toml"

usage() {
  echo 'usage: scripts/perf-gate.sh [--update [--reason "<why>"]]' >&2
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

# The baselines: one `<path>.<input>.<isa>\t<counter>\t<value>` a line.
baselines=$(awk '
  /^\[/ { s = $0; sub(/^\[/, "", s); sub(/\].*$/, "", s); next }
  s !~ /^gate\./ && /^[a-z0-9.-]+ *= *\{ *value *= *[0-9]+/ {
    k = $0; sub(/ *=.*/, "", k)
    v = $0; sub(/^[^{]*\{ *value *= */, "", v); sub(/[^0-9].*$/, "", v)
    print s "\t" k "\t" v
  }' "$budgets")
base_get() { printf '%s\n' "$baselines" | awk -F'\t' -v s="$1" -v k="$2" '$1 == s && $2 == k { print $3 }'; }

# The simulated geometry: this host's L1d and last level, from the file.
d1=$(awk -F'"' '/^d1 *=/ { print $2 }' "$budgets")
ll=$(awk -F'"' '/^ll *=/ { print $2 }' "$budgets")
[ -n "$d1" ] && [ -n "$ll" ] || {
  echo "perf-gate: FAIL: $budgets names no d1 or ll geometry"
  exit 1
}

if [ "$update" -eq 0 ] && ! printf '%s\n' "$baselines" | awk -F'\t' -v i="$isa" '{ n = split($1, a, "."); if (a[n] == i) f = 1 } END { exit !f }'; then
  echo "perf-gate: FAIL: no baselines for $isa in perf-budgets.toml; tier D gates x86_64"
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

# run <dir> <fn> <bench> <line> <arg>...: one callgrind run of <bench> with
# <arg>..., its counts left in <dir>/counts as `<event> <count>` lines, the
# calls into <fn> in <dir>/calls, and the bench's exit status in <dir>/status.
#
# The bench is pid 1 of its own namespace, so its profile is `cg.1` (and
# `cg.1.<n>` for any part it dumps). The child it spawns after the timed
# loop is forked under valgrind and writes `cg.2`, a copy of its parent's
# counts, which is left out: summing the two would count the loop twice.
run() {
  local dir=$1 fn=$2 bench=$3 line=$4 status=0
  shift 4
  mkdir -p "$dir"
  cp "${binof[$bench]}" "$dir/$bench"
  (
    cd "$dir"
    exec "${pin[@]}" "$ns" -r -p -f --mount-proc env -i PATH=/usr/bin:/bin LC_ALL=C \
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
count() { awk -v k="$2" '$1 == k { print $2; f = 1 } END { if (!f) print 0 }' "$1/counts"; }

# Every run of every entry, and the control, at once: the counts do not
# depend on what else the machine is doing.
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
    run "$work/$e/a64" "$fn" "$bench" 64 "${argv[@]}" &
    run "$work/$e/b64" "$fn" "$bench" 64 "${argv[@]}" &
    run "$work/$e/a128" "$fn" "$bench" 128 "${argv[@]}" &
  done
done
control_fn=perf_gate::no_such_fn
run "$work/control" "$control_fn" ingest2 64 less 1 &
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

# vacuous <dir>: why the run measured nothing, or nothing when it measured.
vacuous() {
  local dir=$1
  if [ "$(cat "$dir/status")" -ne 0 ]; then
    echo "the bench exited $(cat "$dir/status"): $(grep -v '^==' "$dir/log" | tail -n 1)"
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

# The control first: a gate whose vacuity check cannot fire checks nothing.
why=$(vacuous "$work/control")
if [ -z "$why" ]; then
  echo "FAIL control: --toggle-collect=$control_fn measured $(count "$work/control" Ir) Ir and passed the vacuity check"
  hard=$((hard + 1))
else
  echo "ok   control: --toggle-collect=$control_fn is vacuous ($why)"
fi

measured=()
for e in "${entries[@]}"; do
  p=${e%%.*}
  kind=$(gate_get "$p" kind)
  bad=""
  for r in a64 b64 a128; do
    why=$(vacuous "$work/$e/$r")
    [ -z "$why" ] || bad="vacuous: $why"
  done
  ir=$(count "$work/$e/a64" Ir)
  b=$(base_get "$e" ir)
  if [ -n "$bad" ]; then
    :
  elif ! cmp -s "$work/$e/a64/counts" "$work/$e/b64/counts"; then
    bad="not deterministic: two runs at the 64-byte line differ ($(tr '\n' ' ' <"$work/$e/a64/counts")/ $(tr '\n' ' ' <"$work/$e/b64/counts"))"
  elif [ "$ir" -ne "$(count "$work/$e/a128" Ir)" ]; then
    bad="not deterministic: Ir at the 128-byte line is $(count "$work/$e/a128" Ir), at 64 $ir"
  elif [ -n "$b" ] && { [ "$ir" -gt "$((b * 2))" ] || [ "$((ir * 2))" -lt "$b" ]; }; then
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
    dir="$work/$e/a$g"
    counter "d1-$g.d1mw" "$(count "$dir" D1mw)"
    counter "d1-$g.d1mr" "$(count "$dir" D1mr)"
    counter "d1-$g.llmr" "$(($(count "$dir" ILmr) + $(count "$dir" DLmr)))"
    counter "d1-$g.llmw" "$(count "$dir" DLmw)"
  done
  case "$status" in
  FAIL) fails=$((fails + 1)) ;;
  WARN) warns=$((warns + 1)) ;;
  ok) status="ok  " ;;
  esac
  echo "$status $e: calls=$(cat "$work/$e/a64/calls"), A/A identical;$note"
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
    if [ -n "$b" ] && [ "$v" -lt "$b" ]; then
      why="ratchet: lowered from $b"
    else
      raised=$((raised + 1))
      why=$reason
    fi
    why=${why//\\/\\\\}
    why=${why//\"/\\\"}
    echo "$k = { value = $v, set_in = \"$set_in\", reason = \"$why\" }" >>"$out"
  done
  if [ "$raised" -gt 0 ] && [ -z "$reason" ]; then
    echo "perf-gate: no update: $raised figures rose or are new, and a raise needs --reason \"<why>\""
    exit 1
  fi
  cp "$out" "$budgets"
  echo "perf-gate: wrote perf-budgets.toml ($raised raised or new, $ratchets ratchets)"
  exit 0
fi

if [ "$fails" -gt 0 ]; then
  echo "perf-gate: FAIL ($fails failing, $warns warning, of ${#entries[@]} entries)"
  exit 1
fi
echo "perf-gate: PASS (${#entries[@]} entries, $warns warning, $ratchets ratchets available)"
