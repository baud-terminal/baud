#!/usr/bin/env bash
# Feature-arm test gate: baud-core's suite under its arms.
#
# `check-features.sh` builds every arm alone and runs none of them. An arm
# that compiles can still be wrong, and two were: `two-pass-csi` declined a
# `CSI` without resetting its parameters (fixed in 08bc0f9), and on c6a5494
# this suite's own
# `a_gr_byte_in_any_escape_or_control_sequence_acts_as_its_seven_bit_byte_under_latin1`
# was red under it while no gate ran the suite under it; and an `sgr-table`
# hit left the parser inside the sequence, so five of the suite's tests, the
# golden digests among them, were red under it on c6a5494 and a9bf3a1. This
# runs the suite through nextest, as `test:unit` does.
#
# **Every declared baud-core feature is in exactly one class, and the
# classes are the gate.** A feature in none, or in two, fails here before
# anything builds, so the next arm added to `baud-core/Cargo.toml` cannot be
# skipped in silence, which is how both of those went untested. The floor
# is the declared count, as `check-features.sh` keeps its floors.
#
#   BUILDS    one build a line, its arms joined by commas.
#             `two-pass-csi` carries `probe-double-final-scan`, which only
#             adds work over it and implies it. `param3`,
#             `probe-no-fast-csi` and `colour-in-match` share a build
#             because none hides another: `probe-no-fast-csi` sends
#             every `CSI` down the graph walk, where `param3` is, and of the
#             arms run here it hides only those in `fast_csi`,
#             `two-pass-csi`, `sgr-table` and the probes, so those are built
#             apart from it. `diag` adds counters and
#             the tests that read them (`tests/bulk_paths.rs`), and is built
#             alone: `probe-double-scan` runs `run_of` a second time, and the
#             stride counts with it. `cell-table` is built alone: it reads
#             a three-byte character's cell from a table where the default
#             build decodes it, and was EXCLUDED until its read wrote what
#             the computing path writes: it asks the cluster machine what
#             that path asks, leaves IRM to it and frees an image under it.
#   TOGETHER  additive probes, each running a second copy of some work into
#             `black_box` (or a scratch) and leaving every counter and the
#             screen as they were. One build carries them all: an arm that is
#             not additive changes the screen and turns the run red, and since
#             none of them removes work, none can undo another's change. Built
#             together they cost one build instead of eighteen.
#   OFF_ARM64 built alone on a host that is not aarch64, and skipped, with
#             its reason printed, on one that is. `route-stride` reroutes the
#             portable stride scan, which `neon-scan` replaces on aarch64,
#             and `neon-scan` is on in every test build of baud-core: its
#             dev-dependency on itself turns the default features back on,
#             so `--no-default-features` does not take it off.
#   EXCLUDED  not run, for one of three reasons: wrong by construction,
#             naming a test it turns red, or for `ceiling-no-act` the tests
#             that ran past 1320 s under it; covered by another gate; or not
#             additive, `probe-double-intern-full`, naming the test it turns
#             red.
#
# nextest only, with no `cargo test --doc` after it: baud-core has no
# doctest to run (its two doc fences, both in `graph.rs`, are `text`).
#
# **A wrong-by-construction arm writes proptest regressions.** A run of an
# EXCLUDED arm by hand fails its properties and appends their seeds to the
# checked-in `.proptest-regressions` files; `git checkout` them after.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

MANIFEST=baud-core/Cargo.toml
FLOOR=49

BUILDS="
two-pass-csi,probe-double-final-scan
cell-table
sgr-table
diag
param3,probe-no-fast-csi,colour-in-match
"

TOGETHER="probe-double-break probe-double-attach probe-double-classify
probe-double-extent probe-double-decode probe-double-scan probe-warm-scroll
probe-double-params probe-csi-scratch probe-double-text-scan probe-double-sgr
probe-sgr-copy probe-double-fold probe-double-sgr-color
probe-sgr-color-reborrow probe-double-intern probe-double-parse
probe-double-put"

OFF_ARM64="route-stride"

# name  reason
EXCLUDED="
alloc                    on in every test build: the dev-dependency on baud-core itself enables it
neon-scan                the default feature, so test:unit runs it
avx2-scan                inert off x86-64; test:x86-avx2 runs the workspace suite with it and +avx2
ceiling-no-write         wrong by construction: the_cell_table_is_what_the_write_path_writes
ceiling-no-decode        wrong by construction: the_cell_table_is_what_the_write_path_writes
ceiling-no-classify      wrong by construction: every_line_of_the_suite_clusters_through_the_terminal
ceiling-no-run-gate      wrong by construction: a_flag_is_one_cluster
ceiling-no-blank         wrong by construction: a_row_reused_by_a_scroll_while_narrow_comes_back_clean
ceiling-no-scroll        wrong by construction: a_taller_primary_screen_pulls_the_scrolled_lines_back_above_it
ceiling-no-store         wrong by construction: the_cell_table_is_what_the_write_path_writes
ceiling-no-act           wrong by construction, and three maximal_subparts tests ran past 1320 s
ceiling-no-text          wrong by construction: every_recording_renders_to_its_pinned_digest
ceiling-implicit-tail    wrong by construction: a_control_function_ends_the_cluster_before_it
ceiling-free-sgr         wrong by construction: every_recording_renders_to_its_pinned_digest
ceiling-no-region-copy   wrong by construction: a_copying_region_scroll_carries_a_hidden_tail
ceiling-segment-cache    wrong by construction: a_copy_is_the_same_at_every_read_size
probe-free-width         wrong by construction: a_leading_mark_has_no_base_and_is_counted
probe-pinned-node        wrong by construction: a_copy_is_the_same_at_every_read_size
probe-no-csi-op          wrong by construction: every_recording_renders_to_its_pinned_digest
probe-no-csi-params      wrong by construction: every_recording_renders_to_its_pinned_digest
ascii-only               wrong by construction: every_recording_renders_to_its_pinned_digest
probe-double-intern-full not additive, it writes the bucket slot whatever holds it: a_hit_writes_nothing
"

if [ "$#" -ne 0 ]; then
  echo "usage: $0"
  exit 2
fi

names() { printf '%s\n' "$1" | awk 'NF {print $1}'; }

feats=$(awk '/^\[features\]/{f=1;next} /^\[/{f=0} f && /=/ && $1!="#" {print $1}' "$MANIFEST" | grep -vx default || true)
count=$(printf '%s\n' "$feats" | grep -c . || true)
if [ "$count" -ne "$FLOOR" ]; then
  echo "FAIL: $count features parsed from $MANIFEST, the floor says $FLOOR -- classify the arm and move the floor with it"
  exit 1
fi

builds=$(printf '%s\n' "$BUILDS" | grep . || true)
together=$(printf '%s\n' "$TOGETHER" | tr ' ' '\n' | grep . || true)
off=$(printf '%s\n' "$OFF_ARM64" | tr ' ' '\n' | grep . || true)
listed=$(printf '%s\n' "$(printf '%s\n' "$builds" | tr ',' '\n')" "$together" "$off" "$(names "$EXCLUDED")" | grep . | sort)
dups=$(printf '%s\n' "$listed" | uniq -d)
if [ -n "$dups" ]; then
  echo "FAIL: in more than one class:" "$dups"
  exit 1
fi
unclassified=$(comm -23 <(printf '%s\n' "$feats" | sort) <(printf '%s\n' "$listed"))
stale=$(comm -13 <(printf '%s\n' "$feats" | sort) <(printf '%s\n' "$listed"))
if [ -n "$unclassified" ] || [ -n "$stale" ]; then
  [ -n "$unclassified" ] && echo "FAIL: declared in $MANIFEST and in no class:" "$unclassified"
  [ -n "$stale" ] && echo "FAIL: classified here and not declared in $MANIFEST:" "$stale"
  exit 1
fi

fail=0
run() {
  echo "=== baud-core --features $1"
  if ! RUSTFLAGS="-D warnings" cargo nextest run --release --cargo-quiet --no-fail-fast \
    -p baud-core --features "$1"; then
    fail=1
    echo "  RED  baud-core --features $1"
  fi
}

host=$("${RUSTC:-rustc}" -vV | awk '/^host:/ {print $2}')
case "$host" in
aarch64-*)
  for f in $off; do
    echo "=== skip $f on $host: neon-scan replaces the scan it reroutes, and baud-core's dev-dependency on itself keeps neon-scan on"
  done
  ;;
"")
  echo "FAIL: no host triple from ${RUSTC:-rustc} -vV"
  exit 1
  ;;
*) builds=$(printf '%s\n%s\n' "$builds" "$off") ;;
esac

n=0
for f in $builds; do
  run "$f"
  n=$((n + 1))
done
run "$(printf '%s\n' "$together" | paste -sd, -)"
n=$((n + 1))

if [ "$fail" -ne 0 ]; then
  echo "FAIL: baud-core's suite is red under an arm that claims the default screen"
  exit 1
fi
echo "ok: baud-core's suite passes in $n builds of its arms on $host"
