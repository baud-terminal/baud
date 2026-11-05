#!/usr/bin/env bash
# **Build baud's embedded font from what is committed, and prove it.**
#
#   scripts/build-fonts.sh          rebuild Regular, Bold, Italic and Bold Italic, gate all
#                                   four, and write baud-app/fonts/built/MANIFEST
#   scripts/build-fonts.sh --check  rebuild into a temporary directory and compare with
#                                   built/MANIFEST and the committed files; writes nothing
#
# Each style is built in two stages.
#
# The Hebrew stage: `baud-fontbuild stage` (tools/baud-fontbuild, Rust on read-fonts and
# write-fonts) merges baud-app/fonts/glyphs/languages/hebrew/stage/<Style>.txt into stock
# JetBrains Mono of the same style (baud-app/fonts/jetbrains-mono/). The source holds what
# the stage adds and nothing else: the Hebrew glyphs, drawn from Cascadia Mono, with their
# advances, code points and GDEF classes; the appended mark glyph sets and GPOS lookups; the
# features those lookups join; the hebr script records naming DFLT's Script table; and the
# OS/2 range fields and name records the stage sets. The sources were read out of the Hebrew
# stage the earlier parametric build wrote (the stage hashes FONTLOG.txt lists), and
# `baud-fontbuild compare` held the stage built from them equal to it in every table.
#
# The Devanagari stage: Noto Sans Devanagari 2.007's variable font, which devanagari/SOURCE
# pins and this script fetches by URL and refuses unless it hashes as pinned (it is not in
# the repository), instanced at wght 400 (Regular, Italic) or 700 (Bold, Bold Italic) with
# its width axis kept (hebrew/build/instance.py), and merged after every glyph, lookup and
# table the Hebrew stage holds (devanagari/build/merge.py), which proves that stage unchanged
# object by object before it writes. Both steps are Python, run as described below.
#
# **The gate is two hashes and one comparison per style.** The Hebrew stage's sha256 and the
# merged font's are pinned (gate_hash), so a build that is not deterministic fails. The
# committed files are the bytes the earlier build wrote from a Hebrew stage fontTools had
# packed: write-fonts lays this stage's GSUB and GPOS out in another order, and the merge
# copies GSUB's lookups byte for byte, so the merged font's GSUB bytes and head's checksum
# differ from the committed file's, and no other table's bytes do. `baud-fontbuild compare`
# then holds each merged font equal to the committed file: every glyph, mapping, metric,
# field and lookup it expands (its crate documentation lists them). The committed file
# must hash as the gate and MANIFEST's output line say. A build that differs writes
# nothing: a change to the design changes the shipped bytes, which is a re-pin of its own.
#
# **The Python steps' environment is pinned whole.** Python and its packages come from one
# nixpkgs revision fetched by hash, and run under `env -i` with only the variables written
# below; `-s` keeps a user site-packages out of sys.path. SOURCE_DATE_EPOCH fixes
# head.modified, and fontTools' USE_HARFBUZZ_REPACKER is pinned to True in code
# (hebrew/build/pin.py), which is why uharfbuzz is in the env: it is a build-time dependency
# only.
#
# **Measured on aarch64-darwin only.**
set -euo pipefail
cd "$(dirname "$0")/.." || exit 1
HEB=baud-app/fonts/glyphs/languages/hebrew
JBM=baud-app/fonts/jetbrains-mono
OUT=baud-app/fonts/built
DEVA=baud-app/fonts/glyphs/languages/devanagari
TOOLS=tools
NIXPKGS_REV=7dd199b0e2993e37b4775ed66b1c291699608c9f
NIXPKGS_SHA256=08kxnni9l87i089hy7p9a7yr64w6h9x5m2cc7k2hn455hidm9mm3
# Noto Sans Devanagari 2.007's variable font (devanagari/SOURCE), fetched once into the
# cache the README capture keeps its Noto faces in (NOTO_CACHE, else
# $XDG_CACHE_HOME/baud-readme, else ~/.cache/baud-readme). The build refuses any other
# file. It is data: nothing in it runs.
NOTO_URL='https://raw.githubusercontent.com/notofonts/notofonts.github.io/noto-monthly-release-2026.10.01/fonts/NotoSansDevanagari/unhinted/variable-ttf/NotoSansDevanagari%5Bwdth%2Cwght%5D.ttf'
NOTO_SHA256=9f82923465ef82fb86c6b99cbdd4947ba832fe3335948de815b18adfd75947c7
# The gated hashes, three per style: the committed file (<style>), the Hebrew stage this
# build writes (<style>-hebrew) and the merged font this build writes (<style>-merged),
# which compares equal to the committed file.
gate_hash() {
  case $1 in
  Regular) echo d8e9226d5e68c02e55b3d45232e8adffa22c10123f491de4c6be3f50433d901d ;;
  Bold) echo 87b66b6067bc8bb4d03b4d0cd32538cf59bccaa927d181b2cc831004093dcc0d ;;
  Italic) echo 0ae4e4c355152b4302aa006e52ae7cf3a23619317e8e1608eec9aac03e203a6e ;;
  BoldItalic) echo 20a50bbf6c4147e3eed6cee22ff9ba2df39993238654e06ef3afb840994a441c ;;
  Regular-hebrew) echo 6dfd02c43ae24cb5dcc3b7c62f38e5e1a89ce31837b2d33183597be45c9de857 ;;
  Bold-hebrew) echo b0004652bc6f20ac3c191851588c6164f7aaeed4171e90f9dc5c34fa455a9404 ;;
  Italic-hebrew) echo 69ff807e656889c71bccb23ed0314b9526fb8b7a05f07e209f6cbdde834e1db1 ;;
  BoldItalic-hebrew) echo f46eab60b785be45c8bddeb45fdcc109cf85a6e93303cca8ab7a385e8a5f6918 ;;
  Regular-merged) echo f1be32d9cdd3dc91b36fbab7319b13b5c5c3ff23fe231a50be1df12801566ae1 ;;
  Bold-merged) echo 8839f152bf72483419e32e39478fff68fa4943cb8b35a62e62cd15342d04a5da ;;
  Italic-merged) echo b23de6f037eedf1ecdce191141c67578cc7cd042187ba75ef2007738b763a185 ;;
  BoldItalic-merged) echo ae5207aab2ffb14ecf29924ba1c6e0f3a1d93745ffa5287da078d59aa07c4fdb ;;
  *) return 1 ;;
  esac
}

# The environment every Python step gets, and nothing else.
BASE_ENV=(SOURCE_DATE_EPOCH=315532800 PYTHONHASHSEED=0 PYTHONDONTWRITEBYTECODE=1)

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

echo "python: nixpkgs $NIXPKGS_REV" >&2
PYENV=$(nix-build --no-out-link -E "
  let pkgs = import (builtins.fetchTarball {
        url = \"https://github.com/NixOS/nixpkgs/archive/$NIXPKGS_REV.tar.gz\";
        sha256 = \"$NIXPKGS_SHA256\";
      }) { config = {}; overlays = []; };
  in pkgs.python3.withPackages (p: [ p.fonttools p.skia-pathops p.uharfbuzz p.numpy p.scipy ])")
PY=$PYENV/bin/python3
SYSTEM=$(nix-instantiate --eval -E builtins.currentSystem | tr -d '"')

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

# One Python step: env -i, the base environment, then the interpreter and its arguments.
py() {
  env -i "${BASE_ENV[@]}" "$PY" -s "$@"
}

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
  py "$HEB/build/instance.py" "$NOTO_VF" "$w" "$WORK/noto$w.ttf"
done
for name in "${STYLES[@]}"; do
  if ! py "$DEVA/build/merge.py" "$WORK/$name-hebrew.ttf" "$WORK/noto$(noto_weight "$name").ttf" "$name" \
    "$WORK/$name.ttf" "$WORK/$name.merge.json" >"$WORK/$name.merge.log" 2>&1; then
    echo "FAIL merge $name; the log's tail:" >&2
    tail -n 30 "$WORK/$name.merge.log" >&2
    exit 1
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
  if ! "$FONTBUILD" compare "$OUT/$name.ttf" "$WORK/$name.ttf" >"$WORK/$name.compare.log" 2>&1; then
    echo "GATE FAIL $name: the rebuilt font differs from $OUT/$name.ttf:" >&2
    head -n 20 "$WORK/$name.compare.log" >&2
    fail=1
  fi
done

# MANIFEST: every line not starting with '#' is compared by --check. Machine facts
# (system, store path, compiler) are comments: reported, never compared.
manifest() {
  echo "# baud-app/fonts/built/MANIFEST: what scripts/build-fonts.sh built, from what, and how."
  echo "# Written by scripts/build-fonts.sh. 'scripts/build-fonts.sh --check' rebuilds and compares"
  echo "# every line that does not start with '#'; '#' lines are the building machine's facts."
  echo "# system $SYSTEM"
  echo "# python-env $PYENV"
  echo "# rustc $RUSTC"
  echo "nixpkgs $NIXPKGS_REV fetchTarball-sha256 $NIXPKGS_SHA256"
  py -c '
import sys, fontTools, pathops, numpy, scipy, uharfbuzz
print("package python", sys.version.split()[0])
print("package fonttools", fontTools.version)
print("package skia-pathops", pathops.__version__)
print("package numpy", numpy.__version__)
print("package scipy", scipy.__version__)
print("package uharfbuzz", uharfbuzz.__version__, "(HarfBuzz %s)" % uharfbuzz.version_string())'
  echo "pin USE_HARFBUZZ_REPACKER=True (hebrew/build/pin.py)"
  local v
  for v in "${BASE_ENV[@]}"; do echo "env $v"; done
  echo "python-flags -s, under env -i"
  echo "step stage baud-fontbuild stage JetBrainsMono-<style>.ttf hebrew/stage/<style>.txt <style>-hebrew.ttf, once per style"
  echo "step instance-noto hebrew/build/instance.py NotoSansDevanagari[wdth,wght].ttf 400 noto400.ttf; hebrew/build/instance.py NotoSansDevanagari[wdth,wght].ttf 700 noto700.ttf"
  echo "step merge devanagari/build/merge.py <style>-hebrew.ttf noto<weight>.ttf <style> <style>.ttf, once per style (Regular, Italic: noto400; Bold, Bold Italic: noto700)"
  echo "step compare baud-fontbuild compare built/<style>.ttf <style>.ttf, once per style (each must be equal)"
  local f
  while IFS= read -r f; do
    echo "input $(sha "$f") $f"
  done < <({
    find "$DEVA" "$HEB/stage" "$TOOLS/baud-fontbuild/src" -type f \
      -not -name '*.pyc' -not -name .DS_Store -not -path '*/__pycache__/*'
    printf '%s\n' "$HEB/build/instance.py" "$HEB/build/pin.py" \
      "$TOOLS/Cargo.toml" "$TOOLS/Cargo.lock" "$TOOLS/baud-fontbuild/Cargo.toml"
  } | LC_ALL=C sort)
  echo "fetched $NOTO_SHA256 $NOTO_URL (cached outside the repository; not committed)"
  for name in "${STYLES[@]}"; do
    echo "input $(sha "$JBM/JetBrainsMono-$name.ttf") $JBM/JetBrainsMono-$name.ttf"
  done
  for w in 400 700; do
    echo "intermediate $(sha "$WORK/noto$w.ttf") noto$w.ttf (Noto Sans Devanagari at wght $w, its width axis kept; not committed)"
  done
  for name in "${STYLES[@]}"; do
    echo "intermediate $(sha "$WORK/$name-hebrew.ttf") $name-hebrew.ttf (the Hebrew stage; not committed)"
  done
  for name in "${STYLES[@]}"; do
    echo "intermediate $(sha "$WORK/$name.ttf") $name.ttf (the merged font; compares equal to the output below; not committed)"
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
  echo "ok: rebuilt ${STYLES[*]}, each equal to the committed file; wrote $OUT/MANIFEST"
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
echo "ok: rebuilt $(grep -c '^output ' "$WORK/MANIFEST") fonts from the committed inputs, each equal to the committed file; every MANIFEST line matches"
