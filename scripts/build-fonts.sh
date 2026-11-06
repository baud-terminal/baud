#!/usr/bin/env bash
# **Build baud's embedded font from what is committed, and prove it.**
#
#   scripts/build-fonts.sh          rebuild Regular, Bold, Italic and Bold Italic, gate all
#                                   four, and write baud-app/fonts/built/MANIFEST
#   scripts/build-fonts.sh --check  rebuild into a temporary directory and compare with
#                                   built/MANIFEST and the committed files; writes nothing
#
# Every step is tools/baud-fontbuild (Rust on read-fonts and write-fonts, and baud-font's
# own bake), built from the tools workspace's lock file. Each style is built in two stages.
#
# The Hebrew stage: `baud-fontbuild stage` merges
# baud-app/fonts/glyphs/languages/hebrew/stage/<Style>.txt into stock JetBrains Mono of the
# same style (baud-app/fonts/jetbrains-mono/). The source holds what the stage adds and
# nothing else (hebrew/SOURCE says what and from where).
#
# The Devanagari stage: Noto Sans Devanagari 2.007's variable font, which devanagari/SOURCE
# pins and this script fetches by URL and refuses unless it hashes as pinned (it is not in
# the repository), instanced at wght 400 (Regular, Italic) or 700 (Bold, Bold Italic) with
# its width axis kept (`baud-fontbuild instance`, through baud_font::bake_partial), and
# merged after every glyph, lookup and table the Hebrew stage holds (`baud-fontbuild
# merge`), which proves that stage unchanged object by object before it writes. Each
# instance was held equal, under `baud-fontbuild compare`, to fontTools 4.63.0's instance
# of the same file at the same weight when its hash was pinned (MANIFEST's oracle lines
# name both files; tools/baud-fontbuild/tests/bake.rs runs that comparison again where
# BAUD_FONTTOOLS_NOTO names fontTools' two files).
#
# **The gate is hashes per stage.** The Hebrew stage's sha256, the Noto instances' and the
# merged font's are pinned (gate_hash), so a build that is not deterministic fails, and the
# committed file must be the merged font byte for byte and hash as the gate and MANIFEST's
# output line say. A build that differs writes nothing: a change to the design changes the
# shipped bytes, which is a re-pin of its own.
#
# The epoch below stamps head.modified in the instances and the merged fonts, as fontTools
# stamped it from SOURCE_DATE_EPOCH.
#
# **Measured on aarch64-darwin only.**
set -euo pipefail
cd "$(dirname "$0")/.." || exit 1
HEB=baud-app/fonts/glyphs/languages/hebrew
JBM=baud-app/fonts/jetbrains-mono
OUT=baud-app/fonts/built
DEVA=baud-app/fonts/glyphs/languages/devanagari
TOOLS=tools
# Noto Sans Devanagari 2.007's variable font (devanagari/SOURCE), fetched once into the
# cache the README capture keeps its Noto faces in (NOTO_CACHE, else
# $XDG_CACHE_HOME/baud-readme, else ~/.cache/baud-readme). The build refuses any other
# file. It is data: nothing in it runs.
NOTO_URL='https://raw.githubusercontent.com/notofonts/notofonts.github.io/noto-monthly-release-2026.10.01/fonts/NotoSansDevanagari/unhinted/variable-ttf/NotoSansDevanagari%5Bwdth%2Cwght%5D.ttf'
NOTO_SHA256=9f82923465ef82fb86c6b99cbdd4947ba832fe3335948de815b18adfd75947c7
# fontTools 4.63.0's instances of that file, instantiateVariableFont(font, {"wght": w})
# under SOURCE_DATE_EPOCH=315532800: the oracle each instance below compared equal to when
# it was pinned. Not in the repository.
oracle_hash() {
  case $1 in
  400) echo 92cde87d462bf2a8a9e4cccb51c02c9aeaf6ef4e99db06af0ebc0e7c5e82f64b ;;
  700) echo f3e9be2b943be49cc1683f30456b9dba068422a917a4918677e8cb0dc652cdc4 ;;
  esac
}
EPOCH=315532800
# The gated hashes: the committed file (<style>), the Hebrew stage this build writes
# (<style>-hebrew), the merged font this build writes (<style>-merged), which is the
# committed file, and the two Noto instances (noto<weight>).
gate_hash() {
  case $1 in
  Regular) echo 69b154115da5ee894ac6aab6a531875cbe17d31a474563fc8be4e830e78a7a6f ;;
  Bold) echo ce3630a8c2f25c2879e4dd102e9aa9527f6583333266531a27de5b83c757db4c ;;
  Italic) echo 2c528a8d216e2b14eecc231fea857e7cca623d413de0bb384ae6040af702f2db ;;
  BoldItalic) echo 8a5b9de6ebd2a772f02527c107d66ae0691233ceef1a74e93e97608d1957d77e ;;
  Regular-hebrew) echo 6a0971d77c2309368c5c3bd9f5319065582d47d88090af60f6e016a361485fe3 ;;
  Bold-hebrew) echo 764c571091997e2a024df6dfc3df4c2aa5f18d5875d4c7e71a9a3b65858bc40f ;;
  Italic-hebrew) echo 2bbd7e0ca1b26ecc5da38b6f82a8acd428dbfe4af70d1bf4dbf809893bffb123 ;;
  BoldItalic-hebrew) echo dbeee069e7552ff3dd611aca816c02a7ec02f6768da1bba0eb457d58d38da1ae ;;
  Regular-merged | Bold-merged | Italic-merged | BoldItalic-merged) gate_hash "${1%-merged}" ;;
  noto400) echo e2569116e656d46710fe1133548fcbcda9decdc94ec5d3c3219213a046d13da7 ;;
  noto700) echo 7eea4776889a270ad2cdf4afa89b169ab3b513990abb838fc702cb03aa6052bb ;;
  *) return 1 ;;
  esac
}

mode=build
case ${1:-} in
"") ;;
--check) mode=check ;;
*)
  echo "usage: $0 [--check]" >&2
  exit 2
  ;;
esac

sha() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

SYSTEM=$(uname -sm)

# baud-fontbuild, built from the tools workspace's own lock file.
echo "baud-fontbuild: cargo build --release --locked" >&2
cargo build -q --release --locked --manifest-path "$TOOLS/Cargo.toml" -p baud-fontbuild
FONTBUILD=${CARGO_TARGET_DIR:-$TOOLS/target}/release/baud-fontbuild
RUSTC=$(rustc -V)

WORK=$(mktemp -d "${TMPDIR:-/tmp}/baud-fonts.XXXXXX")
# BUILD_FONTS_KEEP=1 keeps the work directory (instances, logs, the rebuilt MANIFEST).
if [ "${BUILD_FONTS_KEEP:-}" = 1 ]; then
  echo "work directory kept: $WORK" >&2
else
  trap 'rm -rf "$WORK"' EXIT
fi

# The Noto variable font: fetched once, refused unless it is the pinned file.
NOTO_DIR=${NOTO_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/baud-readme}
NOTO_VF="$NOTO_DIR/NotoSansDevanagari[wdth,wght].ttf"
if [ ! -e "$NOTO_VF" ]; then
  echo "fetch $NOTO_URL" >&2
  mkdir -p "$NOTO_DIR"
  curl -sSfL -o "$NOTO_VF.part" "$NOTO_URL"
  mv "$NOTO_VF.part" "$NOTO_VF"
fi
if [ "$(sha "$NOTO_VF")" != "$NOTO_SHA256" ]; then
  echo "FAIL $NOTO_VF is not Noto Sans Devanagari 2.007's variable font (want sha256 $NOTO_SHA256)" >&2
  exit 1
fi

# The Noto weight each style takes.
noto_weight() {
  case $1 in
  Regular | Italic) echo 400 ;;
  Bold | BoldItalic) echo 700 ;;
  esac
}

STYLES=(Regular Bold Italic BoldItalic)
fail=0
for name in "${STYLES[@]}"; do
  if ! "$FONTBUILD" stage "$JBM/JetBrainsMono-$name.ttf" "$HEB/stage/$name.txt" \
    "$WORK/$name-hebrew.ttf" >"$WORK/$name.stage.log" 2>&1; then
    echo "FAIL stage $name:" >&2
    cat "$WORK/$name.stage.log" >&2
    exit 1
  fi
done
for w in 400 700; do
  if ! "$FONTBUILD" instance "$NOTO_VF" "$w" "$EPOCH" "$WORK/noto$w.ttf" >"$WORK/noto$w.log" 2>&1; then
    echo "FAIL instance $w:" >&2
    cat "$WORK/noto$w.log" >&2
    exit 1
  fi
done
for name in "${STYLES[@]}"; do
  if ! "$FONTBUILD" merge "$WORK/$name-hebrew.ttf" "$WORK/noto$(noto_weight "$name").ttf" "$name" \
    "$EPOCH" "$WORK/$name.ttf" >"$WORK/$name.merge.log" 2>&1; then
    echo "FAIL merge $name; the log's tail:" >&2
    tail -n 30 "$WORK/$name.merge.log" >&2
    exit 1
  fi
done
for stage in noto400 noto700; do
  got=$(sha "$WORK/$stage.ttf")
  if [ "$got" != "$(gate_hash "$stage")" ]; then
    echo "GATE FAIL $stage: built $got, the gated file is $(gate_hash "$stage")" >&2
    fail=1
  fi
done
for name in "${STYLES[@]}"; do
  for stage in "$name-hebrew" "$name-merged"; do
    file=$WORK/$stage.ttf
    if [ "$stage" = "$name-merged" ]; then file=$WORK/$name.ttf; fi
    got=$(sha "$file")
    want=$(gate_hash "$stage")
    if [ "$got" != "$want" ]; then
      echo "GATE FAIL $stage: built $got, the gated file is $want" >&2
      fail=1
    fi
  done
  got=$(sha "$OUT/$name.ttf" 2>/dev/null || echo missing)
  if [ "$got" != "$(gate_hash "$name")" ]; then
    echo "GATE FAIL $OUT/$name.ttf is $got, the gated file is $(gate_hash "$name")" >&2
    fail=1
  fi
  if ! cmp -s "$OUT/$name.ttf" "$WORK/$name.ttf"; then
    echo "GATE FAIL $name: the rebuilt font is not $OUT/$name.ttf byte for byte" >&2
    fail=1
  fi
done

# MANIFEST: every line not starting with '#' is compared by --check. Machine facts
# (system, compiler) are comments: reported, never compared.
manifest() {
  echo "# baud-app/fonts/built/MANIFEST: what scripts/build-fonts.sh built, from what, and how."
  echo "# Written by scripts/build-fonts.sh. 'scripts/build-fonts.sh --check' rebuilds and compares"
  echo "# every line that does not start with '#'; '#' lines are the building machine's facts."
  echo "# system $SYSTEM"
  echo "# rustc $RUSTC"
  echo "epoch $EPOCH, stamped into head.modified as SOURCE_DATE_EPOCH was"
  echo "step stage baud-fontbuild stage JetBrainsMono-<style>.ttf hebrew/stage/<style>.txt <style>-hebrew.ttf, once per style"
  echo "step instance-noto baud-fontbuild instance NotoSansDevanagari[wdth,wght].ttf 400 $EPOCH noto400.ttf; baud-fontbuild instance NotoSansDevanagari[wdth,wght].ttf 700 $EPOCH noto700.ttf"
  echo "step merge baud-fontbuild merge <style>-hebrew.ttf noto<weight>.ttf <style> $EPOCH <style>.ttf, once per style (Regular, Italic: noto400; Bold, Bold Italic: noto700)"
  echo "step cmp built/<style>.ttf <style>.ttf, once per style (each must be the same bytes)"
  local f
  while IFS= read -r f; do
    echo "input $(sha "$f") $f"
  done < <({
    find "$DEVA" "$HEB/stage" "$TOOLS/baud-fontbuild/src" -type f -not -name .DS_Store
    printf '%s\n' "$TOOLS/Cargo.toml" "$TOOLS/Cargo.lock" "$TOOLS/baud-fontbuild/Cargo.toml" \
      baud-font/src/bake.rs baud-font/src/bake/partial.rs
  } | LC_ALL=C sort)
  echo "fetched $NOTO_SHA256 $NOTO_URL (cached outside the repository; not committed)"
  for name in "${STYLES[@]}"; do
    echo "input $(sha "$JBM/JetBrainsMono-$name.ttf") $JBM/JetBrainsMono-$name.ttf"
  done
  for w in 400 700; do
    echo "intermediate $(sha "$WORK/noto$w.ttf") noto$w.ttf (Noto Sans Devanagari at wght $w, its width axis kept; not committed)"
    echo "oracle $(oracle_hash "$w") fontTools 4.63.0's noto$w.ttf, which noto$w.ttf compared equal to when it was pinned (not committed)"
  done
  for name in "${STYLES[@]}"; do
    echo "intermediate $(sha "$WORK/$name-hebrew.ttf") $name-hebrew.ttf (the Hebrew stage; not committed)"
  done
  for name in "${STYLES[@]}"; do
    echo "output $(sha "$OUT/$name.ttf") $OUT/$name.ttf"
  done
}
manifest >"$WORK/MANIFEST"

if [ "$mode" = build ]; then
  if [ "$fail" -ne 0 ]; then
    echo "FAIL: not the gated fonts; nothing written to $OUT" >&2
    exit 1
  fi
  cp "$WORK/MANIFEST" "$OUT/MANIFEST"
  grep '^output ' "$OUT/MANIFEST"
  echo "ok: rebuilt ${STYLES[*]}, each the committed file byte for byte; wrote $OUT/MANIFEST"
  exit 0
fi

# --check
if [ ! -f "$OUT/MANIFEST" ]; then
  echo "FAIL: no $OUT/MANIFEST to check against" >&2
  exit 1
fi
grep -v '^#' "$OUT/MANIFEST" >"$WORK/committed"
grep -v '^#' "$WORK/MANIFEST" >"$WORK/rebuilt"
if ! cmp -s "$WORK/committed" "$WORK/rebuilt"; then
  fail=1
  echo "FAIL the rebuild differs from $OUT/MANIFEST ('-' committed, '+' rebuilt):"
  diff "$WORK/committed" "$WORK/rebuilt" | grep '^[<>]' | sed 's/^</-/; s/^>/+/' || true
  for kind in output intermediate input; do
    n=$(diff "$WORK/committed" "$WORK/rebuilt" | grep -c "^> $kind " || true)
    if [ "$n" -gt 0 ]; then echo "  $kind lines that differ: $n"; fi
  done
fi
if ! diff -q <(grep '^#' "$OUT/MANIFEST") <(grep '^#' "$WORK/MANIFEST") >/dev/null; then
  echo "note: this machine differs from the one that wrote MANIFEST (reported, not compared):"
  diff <(grep '^#' "$OUT/MANIFEST") <(grep '^#' "$WORK/MANIFEST") | grep '^[<>]' || true
fi
if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "ok: rebuilt $(grep -c '^output ' "$WORK/MANIFEST") fonts from the committed inputs, each the committed file byte for byte; every MANIFEST line matches"
