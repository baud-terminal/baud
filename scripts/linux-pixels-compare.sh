#!/usr/bin/env bash
# The comparison stage of `linux-pixels.sh`, which runs it last: what the
# three arms drew, read from the folder that run filled.
#
#   scripts/linux-pixels-compare.sh <outdir>
#
# Run alone, it compares a finished run's frames again without the
# containers, on this machine's build of baud-bench's linux-pixels example.
#
# Exits 1 when any check does not hold -- a frame that differs from the
# one it is held to, a screenshot that changed in the second after, a
# control that is not its one value -- and names each at the end; 0 when
# every one holds.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=$(cd "${1:?usage: scripts/linux-pixels-compare.sh <outdir>}" && pwd)
fixtures=$(cat "$out/fixtures.txt")
# Each check that does not hold, a line each: the loops below run in
# pipelines, whose subshells cannot set a variable here.
failed=$out/failed.txt
: >"$failed"
fail() { echo "$*" >>"$failed"; }

# The comparisons, by baud-bench's linux-pixels example, built here as the Mac
# arm's bench was.
cargo build -q --release --manifest-path "$root/Cargo.toml" -p baud-bench --example linux-pixels
px=${CARGO_TARGET_DIR:-$root/target}/release/examples/linux-pixels
# The grid of a screenshot `$1`, `$2` x `$3` pixels, where the app centres
# it: half of what the screenshot leaves beyond it on each axis. A PNG's
# width and height are its IHDR's first eight bytes, big-endian, at 16.
grid_at() {
  local sw sh
  read -r sw sh < <(od -An -tu1 -j 16 -N 8 "$1" |
    awk '{print $1 * 16777216 + $2 * 65536 + $3 * 256 + $4, $5 * 16777216 + $6 * 65536 + $7 * 256 + $8}')
  $px crop "$1" $(((sw - $2) / 2)) $(((sh - $3) / 2)) "$2" "$3" "$4"
}
echo "$fixtures" | while read -r name _ ws scale comp; do
  [ -n "$name" ] || continue
  echo "== $name ($ws, scale $scale, $comp)"
  if [ -s "$out/shot/$name.told" ]; then sed 's/^/        /' "$out/shot/$name.told"; fi
  printf 'mac against linux, the bench: '
  if cmp -s "$out/mac/$name.pam" "$out/linux/$name.pam"; then
    echo identical
  else
    $px diff "$out/mac/$name.pam" "$out/linux/$name.pam" "$out/diff/$name-os.png" ||
      fail "$name: mac against linux, the bench"
  fi
  # The child writes the stream once the window's grid is the fixture's;
  # one that never saw it wrote nothing, and the page is empty.
  printf 'the child wrote the stream: '
  if [ -e "$out/shot/$name.fed" ]; then
    echo yes
  else
    echo "no, the window's grid never became $ws"
    fail "$name: the child never saw a $ws grid"
  fi
  printf 'the screenshot settled: '
  if $px diff "$out/shot/$name.png" "$out/shot/$name.again.png" >/dev/null; then
    echo yes
  else
    echo "no, it changed in the second after"
    fail "$name: the screenshot did not settle"
  fi
  $px over-black "$out/linux/$name.pam" "$out/linux/$name.over.pam"
  w=$(sed -n 's/^WIDTH //p' "$out/linux/$name.pam" | head -1)
  h=$(sed -n 's/^HEIGHT //p' "$out/linux/$name.pam" | head -1)
  grid_at "$out/shot/$name.png" "$w" "$h" "$out/shot/$name.grid.pam"
  printf 'linux over black against the screenshot: '
  $px diff "$out/linux/$name.over.pam" "$out/shot/$name.grid.pam" "$out/diff/$name-app.png" ||
    fail "$name: linux over black against the screenshot"
done
# The control's one value, which linux-pixels computes and reads from neither
# arm: rgb(185, 37, 105) at the dark dim's alpha, round(0.70 * 255) = 179,
# premultiplied over black and rounded.
for c in control control-x2 control-sway control-x1.5; do
  printf 'the %s holds one value, (130, 26, 74), everywhere: ' "$c"
  holds=$($px control "$out/shot/$c.grid.pam")
  echo "$holds"
  [ "$holds" = yes ] || fail "$c: not the control's one value"
done
# The moves: each step's window, what it attached and told, and its frame
# on the output it is on against the bench at that output's scale.
for arm in bookworm trixie; do
  m=$out/$arm/moves
  if [ "$arm" = trixie ]; then s2=1.5 bench2=less-pager-x1.5; else s2=2 bench2=less-pager-x2; fi
  echo "== moves, sway on $arm: $(tr '\n' ' ' <"$m/outputs.txt")"
  [ -e "$m/baud.fed" ] || {
    echo "-- the child never saw a 100x35 grid, and wrote nothing"
    fail "moves on $arm: the child never saw a 100x35 grid"
  }
  for step in first across-from-1 moved across-from-2 back; do
    echo "-- $step: the window at $(cat "$m/$step.rect")"
    sed 's/^/        /' "$m/$step.told"
  done
  for shot in first-HEADLESS-1:less-pager:1 moved-HEADLESS-2:$bench2:$s2 back-HEADLESS-1:less-pager:1; do
    IFS=: read -r f b sc <<<"$shot"
    w=$(sed -n 's/^WIDTH //p' "$out/linux/$b.pam" | head -1)
    h=$(sed -n 's/^HEIGHT //p' "$out/linux/$b.pam" | head -1)
    printf '%s, its screenshot %s, against the bench at %s: ' "$f" \
      "$($px png-size "$m/$f.png")" "$sc"
    grid_at "$m/$f.png" "$w" "$h" "$m/$f.grid.pam"
    $px diff "$out/linux/$b.over.pam" "$m/$f.grid.pam" "$out/diff/$arm-$f.png" ||
      fail "moves on $arm, $f: against the bench at $sc"
  done
  echo "-- every window buffer size and scale told, in order:"
  sed 's/^/        /' "$m/told.all"
done
echo "== clipboard"
cat "$out/clip/result.txt"
if [ -s "$failed" ]; then
  echo "== $(wc -l <"$failed") checks do not hold:"
  sed "s/^/  /" "$failed"
  exit 1
fi
echo "== every check holds"
