#!/usr/bin/env bash
# **Build baud's embedded font from what is committed, and prove the bytes.**
#
#   scripts/build-fonts.sh            build Regular, Bold, Italic and Bold Italic, gate all
#                                     four, and write
#                                     baud-app/fonts/built/{Regular,Bold,Italic,BoldItalic}.ttf
#                                     and MANIFEST
#   scripts/build-fonts.sh --check    rebuild into a temporary directory and compare with
#                                     built/MANIFEST and the committed files; writes nothing
#   scripts/build-fonts.sh --italics  build the two italics with the rounds' correction off
#                                     (ROUND_K=0 LEVEL=0) and compare each with the plain shear's
#                                     gated hash: the control that the correction is the
#                                     only thing the italics add to the shear; writes nothing
#
# The inputs are the stock JetBrains Mono base (baud-app/fonts/jetbrains-mono/) and
# baud-app/fonts/glyphs/languages/hebrew/: Cascadia Mono's variable font from the
# v2407.24 release, the dagesh anchors read from its instances, and the build modules
# (build/). The steps: instance the variable font at wght 400 and 700, regenerate the
# anchors from those instances and require them equal to the committed ones, then run
# build/build.py once per style under `env -i` with that style's exact environment.
# The italics are the upright design leaning right 9 degrees with the Latin, the
# owner's call of 2026-10-24, with JetBrains Mono Italic's correction of rounds: its
# bowls' joints levelled and its round corners' handles re-balanced (build/optical.py);
# each merges into JetBrains Mono's own Italic or Bold Italic and
# carries the built family's name, so the four embedded faces are one family, as the
# OpenType name table's nameID 1 has a style-linking group share one name.
#
# **The gate is the sha256 of the gated files, by value.** These outputs are the
# research tree's final3 set (note AC, docs/research/notes/ac-built-font.md, section
# 4b) with two conformance fixes (2026-10-08) and one letter redrawn (2026-10-24,
# below), and nothing else. The fixes: nameID 7 carries
# Cascadia's trademark notice after JetBrains Mono's, and GSUB and GPOS each carry a
# hebr script record naming DFLT's Script table (build.py, with_script_copy). Take
# that 6-byte record out of each table and GSUB and GPOS are final3's byte for byte;
# every other table but name and head (its checksum) is final3's. final3's gates --
# the generalised gate, OTS, the strip-and-compare gate, Latin against stock, the
# style path -- ran on the final3 bytes, and were run again on these. OTS passes.
# The generalised gate fails "GSUB byte-identical" and "GPOS scripts unchanged", and
# the strip-and-compare gate "GPOS minus appended != JBM GPOS", "mark reach set
# differs", "mkmk reach set differs" and "GSUB differs": each only because of the
# hebr record, and with it taken out every check of both passes. Every style-path,
# Latin and floor render is pixel-identical to final3's; that checks the latn path
# only, because baud's shaper selects latn, else DFLT (baud-font/src/layout.rs), so no
# render reads the hebr record. Overpaint ran on them too, but through baud-render it
# cannot fail for any font: the composite clips each glyph to its own row's band
# (baud-render/src/composite.rs, `Composite::over`), so ink below a row is cut, not
# painted over the next. The letter: dalet (U+05D3), whose leg build/distinct.py
# moves to its roof's right end so that it no longer reads as zayin (note AC 4d).
# Against the build before it each style differs in that glyph's outline, its
# anchors and head's checksum alone; the generalised and strip-and-compare gates print
# the same lines on both, OTS passes, and the meteg check passes the same clusters. A
# build whose bytes differ is not that set, whatever it looks like, so the build
# refuses to write it. Changing the design means re-running those
# gates and then these constants, in one change, with the reason.
#
# **Every byte depends on the environment, so the environment is pinned whole.**
# - Python and its packages come from one nixpkgs revision fetched by hash. The
#   interpreter it gives on aarch64-darwin is the store path that built final3.
# - The build modules read generic variable names (EY, DX, GV, SM, LAMED and more), so
#   a stray one in a shell would silently change the outputs. Python runs under
#   `env -i` and gets only the variables written below. `-s` keeps a user
#   site-packages out of sys.path. `-I` and `-E` are not used, because they would
#   drop PYTHONHASHSEED and PYTHONDONTWRITEBYTECODE.
# - SOURCE_DATE_EPOCH fixes head.modified; head.created is stock JetBrains Mono's.
#   Without it, Regular differs at byte 161 (head).
# - fontTools' USE_HARFBUZZ_REPACKER is pinned to True in code (build/pin.py). That is
#   why uharfbuzz is in the env: without it, GPOS packs in another order and every
#   output's hash changes (measured, pin.py). It is a build-time dependency only.
#
# **Measured on aarch64-darwin only.** Whether another system reproduces these bytes
# (skia-pathops, numpy and scipy do floating point here) is unmeasured.
set -euo pipefail
cd "$(dirname "$0")/.." || exit 1

HEB=baud-app/fonts/glyphs/languages/hebrew
JBM=baud-app/fonts/jetbrains-mono
OUT=baud-app/fonts/built

NIXPKGS_REV=7dd199b0e2993e37b4775ed66b1c291699608c9f
NIXPKGS_SHA256=08kxnni9l87i089hy7p9a7yr64w6h9x5m2cc7k2hn455hidm9mm3

# Cascadia Mono's variable font, ttf/CascadiaMono.ttf in the v2407.24 release zip
# (hebrew/SOURCE). The build refuses any other file.
VF_SHA256=08026f617eea315caca59898784e999f6cdb2d0c34d2040deeb042bcdc359fa2

# The gated hashes: the four embedded styles, and for --italics the two italics as
# the plain 9-degree shear the earlier gated right-lean recipes built (note AC 4b),
# with round 5's dalet since 2026-10-24, which the italics' recipes reproduce byte for
# byte with ROUND_K=0 LEVEL=0.
gate_hash() {
  case $1 in
  Regular) echo 769a5d99505247e42c8cfd0d013f68a1ad1cf5f4c64ccd96d73f1a3387c881bb ;;
  Bold) echo 13ce890885c189eeb3c21bef5cca32f4f96c362383fb1ecc7a428154f5575fe8 ;;
  Italic) echo c6b0e8a3375541de332ab95249471c5da8dcc0e4c5d801e8bbd2079e813712be ;;
  BoldItalic) echo 183877058f5ee024254897522bee85066f5b795350f7bba72e90ca9a45f503f1 ;;
  Italic-plain) echo c20a35aa4769f7c11c6098e496dd696c8e3b4e7a245020b3be9b74889c190463 ;;
  BoldItalic-plain) echo 5f83a8f97ca6a10c348bab1428968f8609b13d891256a90eef5efa3d25c35b5d ;;
  *) return 1 ;;
  esac
}

# The environment every Python step gets, and nothing else.
BASE_ENV=(SOURCE_DATE_EPOCH=315532800 PYTHONHASHSEED=0 PYTHONDONTWRITEBYTECODE=1)

# Each style's environment, copied exactly from the research drivers that built the
# gated set. buildone.sh exported DIP_FLOOR=1 and LOWEST_DIP=-293 for every style.
# buildall.sh gave Bold and Bold Italic BOLDENV, and Bold Italic right its own hataf
# gap (54). buildall3.sh rebuilt the two right leans with the meteg cell fit
# (METEG_FIT=1) under UID_TAG prototype-bi-bold2-m: those are the italics' recipes,
# whose rounds' correction is style.py's ROUND_K and LEVEL, set by STYLE. VARIANT=A and LAMED=L2
# are the interim picks for the owner's 1a and 1b, the review page's shown defaults.
BOLDENV=(BOLD_DOT_W=120 BOLD_DOT_H=98 BOLD_GV=40 BOLD_HATAF_BAR_MIN_DROP=3
  BOLD_LOWEST_TWO=-272.0 BOLD_LOWEST_DIP=-298.0 BOLD_SEG_PITCH=133 BOLD_QUB_DX=146
  BOLD_QUB_PITCH=134.4 BOLD_SEG_HOME=-43 BOLD_QUB_HOME=-42)
COMMON=(VARIANT=A LAMED=L2 DIP_FLOOR=1 LOWEST_DIP=-293)
recipe() {
  case $1 in
  Regular) RECIPE=(STYLE=Regular LEAN=R "${COMMON[@]}") ;;
  Bold) RECIPE=(STYLE=Bold LEAN=R "${BOLDENV[@]}" UID_TAG=prototype-bi-bold2 BOLD_HATAF_GAP=46 "${COMMON[@]}") ;;
  Italic) RECIPE=(STYLE=Italic LEAN=R UID_TAG=prototype-bi-bold2-m METEG_FIT=1 "${COMMON[@]}") ;;
  BoldItalic) RECIPE=(STYLE=BoldItalic LEAN=R "${BOLDENV[@]}" UID_TAG=prototype-bi-bold2-m BOLD_HATAF_GAP=54 METEG_FIT=1 "${COMMON[@]}") ;;
  # the italics with the rounds' correction off: the plain shear (--italics)
  Italic-plain) recipe Italic && RECIPE+=(ROUND_K=0 LEVEL=0) ;;
  BoldItalic-plain) recipe BoldItalic && RECIPE+=(ROUND_K=0 LEVEL=0) ;;
  *) return 1 ;;
  esac
}

# The JetBrains Mono file each recipe merges into.
base_of() {
  case $1 in
  Regular) echo Regular ;;
  Bold) echo Bold ;;
  Italic | Italic-*) echo Italic ;;
  BoldItalic | BoldItalic-*) echo BoldItalic ;;
  esac
}

mode=build
case ${1:-} in
"") ;;
--check) mode=check ;;
--italics) mode=italics ;;
*)
  echo "usage: $0 [--check | --italics]" >&2
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

WORK=$(mktemp -d "${TMPDIR:-/tmp}/baud-fonts.XXXXXX")
# BUILD_FONTS_KEEP=1 keeps the work directory (instances, logs, the rebuilt MANIFEST).
if [ "${BUILD_FONTS_KEEP:-}" = 1 ]; then
  echo "work directory kept: $WORK" >&2
else
  trap 'rm -rf "$WORK"' EXIT
fi

# One Python step: env -i, the base environment, the step's own variables, then the
# interpreter and its arguments after `--`.
py() {
  local vars=()
  while [ "$1" != -- ]; do
    vars+=("$1")
    shift
  done
  shift
  # ${vars[@]+...}: bash 3.2 (macOS /bin/bash) calls an empty array unbound under set -u.
  env -i "${BASE_ENV[@]}" HEBREW_WORK="$WORK" ${vars[@]+"${vars[@]}"} "$PY" -s "$@"
}

if [ "$(sha "$HEB/CascadiaMono.ttf")" != "$VF_SHA256" ]; then
  echo "FAIL $HEB/CascadiaMono.ttf is not the release's variable font (want sha256 $VF_SHA256)" >&2
  exit 1
fi

for w in 400 700; do
  py -- "$HEB/build/instance.py" "$HEB/CascadiaMono.ttf" "$w" "$WORK/cas$w.ttf"
  py -- "$HEB/casanchors.py" "$WORK/cas$w.ttf" "$WORK/anchors-$w.json" >"$WORK/anchors-$w.log"
  if ! cmp -s "$WORK/anchors-$w.json" "$HEB/anchors-$w.json"; then
    echo "FAIL anchors-$w.json: regenerated from the wght $w instance, it differs from the committed file" >&2
    exit 1
  fi
done

build_one() {
  local name=$1
  recipe "$name"
  if ! py "${RECIPE[@]}" -- "$HEB/build/build.py" "$WORK/$name.ttf" >"$WORK/$name.log" 2>&1; then
    echo "FAIL build $name; the log's tail:" >&2
    tail -n 30 "$WORK/$name.log" >&2
    exit 1
  fi
}

if [ "$mode" = italics ]; then
  fail=0
  for name in Italic-plain BoldItalic-plain; do
    build_one "$name"
    got=$(sha "$WORK/$name.ttf")
    want=$(gate_hash "$name")
    if [ "$got" = "$want" ]; then
      echo "ok   $name-A-L2  $got  (the plain shear; not embedded)"
    else
      echo "FAIL $name-A-L2  $got  want $want"
      fail=1
    fi
  done
  exit $fail
fi

STYLES=(Regular Bold Italic BoldItalic)
fail=0
for name in "${STYLES[@]}"; do
  build_one "$name"
done
for name in "${STYLES[@]}"; do
  got=$(sha "$WORK/$name.ttf")
  want=$(gate_hash "$name")
  if [ "$got" != "$want" ]; then
    echo "GATE FAIL $name: built $got, the gated file is $want" >&2
    fail=1
  fi
done

# MANIFEST: every line not starting with '#' is compared by --check. Machine facts
# (system, store path) are comments: reported, never compared.
manifest() {
  echo "# baud-app/fonts/built/MANIFEST: what scripts/build-fonts.sh built, from what, and how."
  echo "# Written by scripts/build-fonts.sh. 'scripts/build-fonts.sh --check' rebuilds and compares"
  echo "# every line that does not start with '#'; '#' lines are the building machine's facts."
  echo "# system $SYSTEM"
  echo "# python-env $PYENV"
  echo "nixpkgs $NIXPKGS_REV fetchTarball-sha256 $NIXPKGS_SHA256"
  py -- -c '
import sys, fontTools, pathops, numpy, scipy, uharfbuzz
print("package python", sys.version.split()[0])
print("package fonttools", fontTools.version)
print("package skia-pathops", pathops.__version__)
print("package numpy", numpy.__version__)
print("package scipy", scipy.__version__)
print("package uharfbuzz", uharfbuzz.__version__, "(HarfBuzz %s)" % uharfbuzz.version_string())'
  echo "pin USE_HARFBUZZ_REPACKER=True (build/pin.py)"
  local v
  for v in "${BASE_ENV[@]}"; do echo "env $v"; done
  echo "env HEBREW_WORK=<a temporary directory holding the instances>"
  echo "python-flags -s, under env -i"
  for name in "${STYLES[@]}"; do
    recipe "$name"
    echo "recipe $name ${RECIPE[*]}"
  done
  echo "step instance build/instance.py CascadiaMono.ttf 400 cas400.ttf; build/instance.py CascadiaMono.ttf 700 cas700.ttf"
  echo "step anchors casanchors.py cas400.ttf anchors-400.json; casanchors.py cas700.ttf anchors-700.json (each must equal the committed file)"
  echo "step build build/build.py <style>.ttf, once per recipe"
  local f
  while IFS= read -r f; do
    echo "input $(sha "$f") $f"
  done < <(find "$HEB" -type f -not -name '*.pyc' -not -name .DS_Store -not -path '*/__pycache__/*' | LC_ALL=C sort)
  for name in "${STYLES[@]}"; do
    echo "input $(sha "$JBM/JetBrainsMono-$(base_of "$name").ttf") $JBM/JetBrainsMono-$(base_of "$name").ttf"
  done
  for w in 400 700; do
    echo "intermediate $(sha "$WORK/cas$w.ttf") cas$w.ttf (the wght $w instance; not committed)"
  done
  for name in "${STYLES[@]}"; do
    echo "output $(sha "$WORK/$name.ttf") $OUT/$name.ttf"
  done
}
manifest >"$WORK/MANIFEST"

if [ "$mode" = build ]; then
  if [ "$fail" -ne 0 ]; then
    echo "FAIL: not the gated bytes; nothing written to $OUT" >&2
    exit 1
  fi
  mkdir -p "$OUT"
  for name in "${STYLES[@]}"; do cp "$WORK/$name.ttf" "$OUT/$name.ttf"; done
  cp "$WORK/MANIFEST" "$OUT/MANIFEST"
  grep '^output ' "$OUT/MANIFEST"
  echo "ok: wrote ${STYLES[*]} into $OUT, each the gated bytes, and $OUT/MANIFEST"
  exit 0
fi

# --check
if [ ! -f "$OUT/MANIFEST" ]; then
  echo "FAIL: no $OUT/MANIFEST to check against" >&2
  exit 1
fi
for name in "${STYLES[@]}"; do
  want=$(awk -v p="$OUT/$name.ttf" '$1 == "output" && $3 == p { print $2 }' "$OUT/MANIFEST")
  got=$(sha "$OUT/$name.ttf" 2>/dev/null || echo missing)
  if [ -z "$want" ] || [ "$got" != "$want" ]; then
    echo "FAIL committed $OUT/$name.ttf is $got; MANIFEST says ${want:-nothing}"
    fail=1
  fi
done
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
echo "ok: rebuilt $(grep -c '^output ' "$WORK/MANIFEST") outputs from the committed inputs; every MANIFEST line matches"
