#!/usr/bin/env bash
# Feature-arm build gate.
#
# baud keeps its ceiling and probe arms behind cargo features, off by default,
# because "a compiled arm is how the next person re-measures rather than
# rebuilds." That only holds while the arms *compile*, and nothing checked it:
# the pre-push gate builds with no `--features`, and `--all-features` builds them
# all at once, which is a different thing -- mutually-exclusive stubs and
# cross-feature variable use mask the per-arm errors. A review found 10 of ~50
# arms broken while the tree was green, several rotted by ordinary signature
# changes (an `&mut`, a function moved behind another feature, a renamed slice).
#
# This builds every feature of every listed crate INDIVIDUALLY. It is the mode
# the arms are actually used in (`--features new-no-write` alone), and the only
# mode that catches this rot.
#
# `cargo check`, not `build`: these failures are all compile-time, so checking
# is enough and far faster. `-D warnings` because the commonest rot is a stubbed
# body leaving a variable unused, which is a warning the arm must not carry.
#
# **Do not run this concurrently with a build for another target** (e.g. the
# `x86_64-unknown-none` no_std check). A `cargo check` for the host and a build
# for a bare-metal target contend, and one arm's test compile picked up the
# no_std target and reported a false `BROKEN` with "can't find crate for `std`".
# The arm built fine alone. This gate says "an arm that does not compile cannot
# be re-measured", so a false BROKEN wastes exactly the time it exists to save --
# run it on its own.
#
# **One list, several crates.** The first version hard-wired `baud-bench`, so a
# feature arm in any other crate was gated by nothing -- the "scoped to what it
# was written beside" failure this repo records for `docs.rs`'s `ROOTS` and the
# audit's flat glob. Each entry carries the minimum number of features its
# manifest must yield, which is the positive control: the bench has ~50 and a
# parse that finds fewer than 30 is a parse that stopped matching, while a new
# crate may legitimately have none yet -- for it the control is that the
# manifest exists and was read.
#
# **The arms run in parallel, each worker in a target directory of its
# own.** One `cargo check` after another left most of the machine idle, and
# two cargo runs in one target directory queue on its lock, so
# CHECK_FEATURES_JOBS workers (half the CPUs by default) each build in
# `<CARGO_TARGET_DIR, or target>/arms-<i>`. An arm always goes to the same
# worker -- its place in the full list, modulo the count -- so a directory
# stays warm for the arms it built before, and the script never deletes
# one. The results print in the list's order as they come in: the same
# lines, and the same exit status, as one arm after another gave.
#
# **`--since <rev>` checks only the crates a change can reach.** An arm
# breaks only when something it compiles changed: its crate's directory, a
# crate it depends on, or a file it reads from outside its directory, which
# `cargo xtask features-since` finds in the sources -- baud-bench compiles
# `baud-app/src/trail.rs` without depending on baud-app. The change is
# everything between <rev> and the working tree, committed or not. A change
# to the lock file, the workspace manifest, cargo's configuration, the
# toolchain's pins, this script or that command's sources checks every
# crate, and so does a scope it cannot compute. Each crate skipped is named
# with the reason, and every floor below is checked whether its arms run or
# not.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

since=""
if [ $# -gt 0 ]; then
  if [ "$1" != "--since" ] || [ $# -ne 2 ] || [ -z "$2" ]; then
    echo "usage: scripts/check-features.sh [--since <rev>]"
    exit 2
  fi
  since=$2
fi

cpus=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)
jobs=${CHECK_FEATURES_JOBS:-$((cpus / 2))}
[ -n "${CHECK_FEATURES_JOBS:-}" ] || [ "$jobs" -ge 1 ] || jobs=1
case "$jobs" in
'' | *[!0-9]* | 0*)
  echo "FAIL: CHECK_FEATURES_JOBS is '$jobs'; it takes a number of workers, 1 or more"
  exit 2
  ;;
esac
base=${CARGO_TARGET_DIR:-${CARGO_BUILD_TARGET_DIR:-target}}

# **Every crate with a [features] block, and the list is the gate.** It
# omitted baud-domain, whose eight arms were therefore built by nothing --
# and `ceiling-no-rowfill`, the one no other crate forwards, was broken by
# exactly the stubbed-body-leaves-a-variable-unused rot this script's own
# header says it exists to catch. A list that names fewer crates than have
# features is the scoped-to-what-it-was-written-beside failure, and the
# control below is what stops it recurring silently -- and on its first
# run it found three more the audit had not named, the ISA crates, each
# with an arm nothing was building either.
# **Each floor is the crate's actual arm count, not a round number under
# it.** `:N` is a minimum, checked against what the manifest parse yields,
# and it existed to catch a parse that read nothing -- but `baud-core:1`
# against 54 real arms, and `baud-render:0` against 3, cannot fire: the
# gate would announce "checking 0 feature arms" and pass. That is this
# repo's own rule -- a budget that is not equal to the count is not
# measuring anything -- so the floors sit at the counts and a dropped arm
# fails here. Adding a feature means changing the number beside it.
CRATES="baud-bench:53 baud-core:49 baud-domain:7 baud-isa:1 baud-isa-aarch64:1 baud-isa-x86-64:1 baud-font:0 baud-render:4 baud-present-macos:1 baud-app:1 baud-wayland:0 baud-xkb:0 baud-present-wayland:0"

# **The control on the list itself.** Every crate carrying a `[features]`
# block must be named above, or the gate silently checks fewer crates than
# the workspace has -- which is how baud-domain's eight arms went unbuilt
# and one of them stayed broken. This compares the workspace's manifests
# against the list rather than trusting the list, so the next crate to
# grow a feature fails here instead of being skipped in silence.
missing=""
names=" $(printf '%s' "$CRATES" | sed 's/:[0-9]*//g') "
for m in */Cargo.toml; do
  grep -q '^\[features\]' "$m" || continue
  c="${m%/Cargo.toml}"
  case "$names" in
  *" $c "*) ;;
  *) missing="$missing $c" ;;
  esac
done
if [ -n "$missing" ]; then
  echo "FAIL: these crates have [features] and are not in CRATES:"
  for c in $missing; do echo "  $c"; done
  echo "  -- the gate would check fewer crates than the workspace has"
  exit 1
fi

# **A feature that nothing reads is worse than one that does not build.**
# A broken arm fails this gate loudly; a *declared* arm with no `#[cfg]`
# behind it builds, runs, and silently measures the base program -- so
# someone re-measuring with `--features ceiling-no-run-gate` reads the
# ceiling as zero and believes it. That is the probe-that-cannot-fire
# failure this repo records four times, arriving in the one script built
# to stop it.
#
# **Comments are stripped before the search, and that is load-bearing.**
# `write.rs` names `ceiling-no-run-gate` in a comment recording what the
# arm once priced, so a search for the bare name calls a dead arm live --
# the same flaw `cargo xtask audit`'s patterns carry, and the one that published "six
# crates forbid unsafe" from a doc comment. What counts as a reader is
# `feature = "name"` on a line with `//` removed.
#
# A feature that forwards (`dep/feat`) needs no reader of its own: the
# arm lives in the crate it names, and that leaf is checked when the loop
# reaches it. Anything else must be named here with a reason.
#
# **The list is empty, and that is the point.** It carried two: both were
# declared with no `#[cfg]` reading them, so `--features <either>` built the
# base program and priced its subject at nothing. Both are resolved rather
# than exempted.
#
# `ceiling-no-run-gate` is restored, because the run gate is live code. Its
# `#[cfg]` makes the gate always admit, and `a_flag_is_one_cluster` goes red
# under it -- `left: 4, right: 2`, four regional indicators where two flags
# belong -- which is the arm being wrong by construction rather than merely
# present.
#
# `probe-const-stride` is deleted, because the arm it names cannot be spelled
# against this code. Its hypothesis was a `Cols` field always equal to 2048;
# the pitch is a cache-derived `Stride` that differs by machine and by width
# (208 for the corpus here, 400 for the app, 200 on a 64-byte line), so the
# literal the probe needs does not exist. The two ways to force one are a
# memory bug -- thirty multiply sites diverging from the field the buffer was
# allocated at -- or overriding `store_stride`, the formula
# `baud-domain/tests/cache.rs` pins in fifteen places. It measured flat when
# it was measured, with a patch that was never committed.
DEAD_OK=""

unexplained=""
for m in */Cargo.toml; do
  c="${m%/Cargo.toml}"
  # Every feature this crate's own code reads, gathered once.
  read_names=$(find "$c/src" "$c/tests" "$c/benches" "$c/examples" "$c/build.rs" \
    -name '*.rs' 2>/dev/null | while IFS= read -r f; do sed 's|//.*||' "$f"; done |
    grep -o 'feature[[:space:]]*=[[:space:]]*"[^"]*"' |
    sed 's/.*"\([^"]*\)"/ \1 /' | sort -u | tr -d '\n')
  declared=$(awk '/^\[features\]/{f=1;next} /^\[/{f=0} f && /=/ && $1!="#" {print $1}' "$m" |
    grep -vx default)
  # Word-splitting is the intent: a feature name carries no whitespace.
  # shellcheck disable=SC2086
  for name in $declared; do
    line=$(grep -m1 "^${name}[[:space:]]*=" "$m")
    case "$line" in *'/'*) continue ;; esac
    case "$read_names" in *" $name "*) continue ;; esac
    case " $DEAD_OK " in *" $c/$name "*) continue ;; esac
    unexplained="$unexplained $c/$name"
  done
done
if [ -n "$unexplained" ]; then
  echo "FAIL: these features are declared and no #[cfg] reads them:"
  for d in $unexplained; do
    echo "  $d"
    leaf="${d##*/}"
    for m in */Cargo.toml; do
      grep -n "\"[^\"]*/$leaf\"" "$m" 2>/dev/null |
        sed "s|^|      forwarded from ${m%/Cargo.toml}: |"
    done
  done
  echo "  -- enabling one builds the base program and measures nothing"
  exit 1
fi

# ── Which crates a change can reach, with --since ────────────────────────
# `scoped` is set when only `reach`'s crates are checked; it may name none.
scoped=""
reach=" "
if [ -n "$since" ]; then
  scope=$(cargo xtask features-since "$since")
  status=$?
  tab=$(printf '\t')
  case "$status:$scope" in
  1:FAIL:*)
    printf '%s\n' "$scope"
    exit 1
    ;;
  0:"all$tab"*)
    echo "since $since: ${scope#all"$tab"}"
    ;;
  0:*)
    scoped=1
    reach=" $(printf '%s\n' "$scope" | cut -f1 | tr '\n' ' ')"
    listed=$(for entry in $CRATES; do
      printf '%s\n' "$scope" | awk -F'\t' -v c="${entry%%:*}" '$1 == c { print "  " c ": " $2 }'
    done)
    if [ -n "$listed" ]; then
      echo "since $since: checking the crates a change can reach"
      printf '%s\n' "$listed"
    else
      echo "since $since: no change reaches a crate with feature arms"
    fi
    ;;
  *)
    echo "since $since: the scope command failed, so every crate is checked"
    ;;
  esac
fi

# ── The plan: every arm, in the order the output gives it ────────────────
# Every crate's arms are found and its floor checked before any arm runs,
# and a crate whose manifest is missing or reads short still stops the gate
# at its place in the list, after the crates before it are reported.
c_name=()
c_count=()
c_first=()
c_end=()
c_skip=()
a_crate=()
a_feat=()
a_slot=()
halt=""
place=0
for entry in $CRATES; do
  crate="${entry%%:*}"
  min="${entry##*:}"
  manifest="$crate/Cargo.toml"
  if [ ! -f "$manifest" ]; then
    halt="FAIL: $manifest does not exist -- the gate would have read nothing for $crate"
    break
  fi
  # The `[features]` section only: alternating awk flag, skip `default` and any
  # comment line whose first token is `#`.
  feats=$(awk '/^\[features\]/{f=1;next} /^\[/{f=0} f && /=/ && $1!="#" {print $1}' "$manifest" | grep -vx default || true)
  count=$(printf '%s\n' "$feats" | grep -c . || true)
  if [ "$count" -lt "$min" ]; then
    halt="FAIL: only $count features parsed from $manifest (expected at least $min) -- the gate is reading nothing"
    break
  fi
  skip=""
  if [ -n "$scoped" ]; then
    case "$reach" in *" $crate "*) ;; *) skip=1 ;; esac
  fi
  # **And the build without the defaults, where a crate has them**, as an
  # empty feature. It is an arm too: `--no-default-features` on
  # `baud-bench` is the portable scan's, and it ran NEON while a crate it
  # depends on took `baud-core`'s defaults, because cargo builds a
  # dependency once, with every feature any crate in the build enables on
  # it. `ingest2` will not compile when the scan it is built as is not the
  # one `baud-core` runs, so this check is where that shows.
  arms=$feats
  grep -q '^default[[:space:]]*=' "$manifest" && arms=$(printf '%s\n-\n' "$feats")
  c_name+=("$crate")
  c_count+=("$count")
  c_skip+=("$skip")
  c_first+=("${#a_crate[@]}")
  while IFS= read -r f; do
    [ -z "$f" ] && continue
    [ "$f" = "-" ] && f=""
    if [ -z "$skip" ]; then
      a_crate+=("$crate")
      a_feat+=("$f")
      a_slot+=($((place % jobs)))
    fi
    place=$((place + 1))
  done <<<"$arms"
  c_end+=("${#a_crate[@]}")
done

# One arm: its BROKEN lines, or nothing.
check_arm() {
  local crate=${a_crate[$1]} f=${a_feat[$1]} out
  # **`--all-targets`, because a bare `cargo check` covers lib and bins
  # only.** When `baud-bench`'s measurement programs were `src/bin/*.rs` a
  # bare check compiled them; they are `[[bench]]` targets now, and a bare
  # check compiled *nothing but the lib* -- measured: a deliberate syntax
  # error in `benches/transport.rs` gave 0 errors without this flag and 2
  # with it. So the move would have silently turned ~50 arm checks into ~50
  # checks of an almost-empty library, which is this repo's
  # gate-scoped-to-the-wrong-thing failure with a fresh carrier.
  # By manifest, because an entry is a directory and `baud-app/` holds the
  # package `baud`.
  if [ -n "$f" ]; then
    if ! out=$(RUSTFLAGS="-D warnings" cargo check --release -q --manifest-path "$crate/Cargo.toml" --all-targets --features "$f" 2>&1); then
      echo "  BROKEN  $crate/$f"
      printf '%s\n' "$out" | grep -E "^error" | head -2 | sed 's/^/      /'
    fi
  elif ! out=$(RUSTFLAGS="-D warnings" cargo check --release -q --manifest-path "$crate/Cargo.toml" --all-targets --no-default-features 2>&1); then
    echo "  BROKEN  $crate --no-default-features"
    printf '%s\n' "$out" | grep -E "^error" | head -2 | sed 's/^/      /'
  fi
}

# ── The workers ──────────────────────────────────────────────────────────
# Each arm's lines go to a file of its own, and its number down a FIFO when
# they are written, so the loop below waits on a read rather than a timer.
# A worker that ends writes `x`: once every worker has, an arm with no
# result is reported BROKEN rather than waited on.
#
# **A gate that stops takes its workers with it.** Background jobs of a
# script ignore SIGINT, and so does the cargo they start, so a Ctrl-C, a
# closed pipe or a TERM that ended this shell left the workers checking
# arms with no one to read them. Any end of this shell stops every process
# under the workers, and a worker whose gate is gone, killed past any trap,
# stops before its next arm.
n=${#a_crate[@]}
tmp=$(mktemp -d)
pids=""
gate=$$
# Every process below the PIDs given, whatever its depth.
descendants() {
  ps -Ao pid=,ppid= | awk -v roots="$*" '
    { kids[$2] = kids[$2] " " $1 }
    END {
      n = split(roots, q, " ")
      r = n
      for (i = 1; i <= n; i++) {
        if (i > r) print q[i]
        m = split(kids[q[i]], more, " ")
        for (j = 1; j <= m; j++) q[++n] = more[j]
      }
    }'
}
stop() {
  trap - EXIT INT TERM HUP PIPE
  # A signal is mostly taken while the loop below waits in `IFS= read`,
  # and the trap runs inside that command's IFS: empty, which left the
  # PIDs one word that `kill` refused, so the workers ran on unstopped.
  local IFS=$' \t\n'
  if [ -n "$pids" ]; then
    # shellcheck disable=SC2046,SC2086 # one PID a word
    kill $pids $(descendants $pids) 2>/dev/null
  fi
  rm -rf "$tmp"
}
trap stop EXIT
trap 'stop; exit 130' INT
trap 'stop; exit 143' TERM
trap 'stop; exit 129' HUP
trap 'stop; exit 141' PIPE
mkfifo "$tmp/done"
exec 3<>"$tmp/done"
w=0
while [ "$n" -gt 0 ] && [ "$w" -lt "$jobs" ]; do
  {
    (
      export CARGO_TARGET_DIR="$base/arms-$w"
      k=0
      while [ "$k" -lt "$n" ]; do
        kill -0 "$gate" 2>/dev/null || exit 1
        if [ "${a_slot[$k]}" -eq "$w" ]; then
          check_arm "$k" >"$tmp/arm-$k" 3>&-
          echo "$k" >&3
        fi
        k=$((k + 1))
      done
    )
    echo x >&3
  } &
  pids="$pids $!"
  w=$((w + 1))
done

ended=0
got=()
await() {
  while [ -z "${got[$1]-}" ]; do
    [ "$ended" -lt "$jobs" ] || return 1
    IFS= read -r line <&3 || return 1
    if [ "$line" = x ]; then
      ended=$((ended + 1))
    else
      got[line]=1
    fi
  done
}

fail=0
total=0
skipped=0
i=0
while [ "$i" -lt "${#c_name[@]}" ]; do
  crate=${c_name[$i]}
  if [ -n "${c_skip[$i]}" ]; then
    echo "$crate: skipped, nothing it is built from changed since $since"
    skipped=$((skipped + 1))
  else
    echo "$crate: checking ${c_count[$i]} feature arms individually (cargo check, -D warnings)"
  fi
  k=${c_first[$i]}
  while [ "$k" -lt "${c_end[$i]}" ]; do
    total=$((total + 1))
    if ! await "$k"; then
      fail=1
      f=${a_feat[$k]}
      if [ -n "$f" ]; then echo "  BROKEN  $crate/$f"; else echo "  BROKEN  $crate --no-default-features"; fi
      echo "      no result: the worker checking it ended first"
    elif [ -s "$tmp/arm-$k" ]; then
      fail=1
      cat "$tmp/arm-$k"
    fi
    k=$((k + 1))
  done
  i=$((i + 1))
done
# shellcheck disable=SC2086 # one PID a word
[ -z "$pids" ] || wait $pids
pids=""

if [ -n "$halt" ]; then
  echo "$halt"
  exit 1
fi
if [ "$fail" -ne 0 ]; then
  echo "FAIL: at least one feature arm does not build -- a compiled arm that does not compile cannot be re-measured"
  exit 1
fi
if [ "$skipped" -gt 0 ]; then
  echo "ok: the $total feature arms a change since $since can reach build; $skipped crates skipped"
else
  echo "ok: all $total feature arms build"
fi
