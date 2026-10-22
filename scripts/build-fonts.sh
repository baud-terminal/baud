#!/usr/bin/env bash
# **Build baud's embedded font from what is committed, and prove the bytes.**
#
#   scripts/build-fonts.sh            build Regular and Bold, name the stock italics into
#                                     their family, gate all four, and write
#                                     baud-app/fonts/built/{Regular,Bold,Italic,BoldItalic}.ttf
#                                     and MANIFEST
#   scripts/build-fonts.sh --check    rebuild into a temporary directory and compare with
#                                     built/MANIFEST and the committed files; writes nothing
#   scripts/build-fonts.sh --italics  build the four italic recipes (both leans) into a
#                                     temporary directory and compare each with its gated
#                                     hash; writes nothing, and nothing embeds them
#
# The inputs are the stock JetBrains Mono base (baud-app/fonts/jetbrains-mono/) and
# baud-app/fonts/glyphs/languages/hebrew/: Cascadia Mono's variable font from the
# v2407.24 release, the dagesh anchors read from its instances, and the build modules
# (build/). The steps: instance the variable font at wght 400 and 700, regenerate the
# anchors from those instances and require them equal to the committed ones, then run
# build/build.py once per style under `env -i` with that style's exact environment.
# Then the stock Italic and Bold Italic get the built family's name records
# (scripts/font-family-records.py: nameIDs 1, 4 and 6, every other table and every
# glyph stock), so the four embedded faces are one family, as the OpenType name
# table's nameID 1 has a style-linking group share one name.
#
# **The gate is the sha256 of the gated files, by value.** These outputs are the
# research tree's final3 set (note AC, docs/research/notes/ac-built-font.md, section
# 4b) with two conformance fixes and nothing else (2026-10-08): nameID 7 carries
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
# painted over the next. A build whose bytes differ is not that set, whatever it looks
# like, so the build refuses to write it. Changing the design means re-running those
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

# The gated hashes. Regular and Bold are embedded. The italics are recipes kept for
# --italics: the Italic and Bold Italic slots stay stock JetBrains Mono until the
# owner chooses the lean (1c) and the construction (shear or optical).
gate_hash() {
  case $1 in
  Regular) echo df06669a5d7748344b34f62dcdf50fb2a2141080d1e0ebb969d928780160122f ;;
  Bold) echo a02eb8b1a509d9ddb468c1a422dfeeb5949bf39acc7c5cfa48749f1c179890d3 ;;
  Italic-right) echo 852093090230e9c5f427fc1b3e70b1d5640d97ac280060fe6869c4f1e2bac001 ;;
  Italic-left) echo 800a1eaac9886e6f98c61968da931743828e3a2084c2d8036f3517d67f5318a6 ;;
  BoldItalic-right) echo 6175bc972ced48e18f7bdc141699775e4c6174821b05fdb47e23ea8d9905dd48 ;;
  BoldItalic-left) echo 2fbc56fb574adfe5cf906703c4b16061b364d00c4264fe94dc42a009d1b2f1ac ;;
  Italic) echo b31440833719f832d9c45467a6187e13564c976fcefe44ef9c3ce7a9fa98f83c ;;
  BoldItalic) echo aa44c16d76b1e0233395e3b5bbda0119af6d0230db4ec093f4e00e9753c4623c ;;
  *) return 1 ;;
  esac
}

# The environment every Python step gets, and nothing else.
BASE_ENV=(SOURCE_DATE_EPOCH=315532800 PYTHONHASHSEED=0 PYTHONDONTWRITEBYTECODE=1)

# Each style's environment, copied exactly from the research drivers that built the
# gated set. buildone.sh exported DIP_FLOOR=1 and LOWEST_DIP=-293 for every style.
# buildall.sh gave Bold and Bold Italic BOLDENV, and Bold Italic right its own hataf
# gap (54). buildall3.sh rebuilt the two right leans with the meteg cell fit
# (METEG_FIT=1) under UID_TAG prototype-bi-bold2-m. VARIANT=A and LAMED=L2 are the
# interim picks for the owner's 1a and 1b, the review page's shown defaults.
BOLDENV=(BOLD_DOT_W=120 BOLD_DOT_H=98 BOLD_GV=40 BOLD_HATAF_BAR_MIN_DROP=3
  BOLD_LOWEST_TWO=-272.0 BOLD_LOWEST_DIP=-298.0 BOLD_SEG_PITCH=133 BOLD_QUB_DX=146
  BOLD_QUB_PITCH=134.4 BOLD_SEG_HOME=-43 BOLD_QUB_HOME=-42)
COMMON=(VARIANT=A LAMED=L2 DIP_FLOOR=1 LOWEST_DIP=-293)
recipe() {
  case $1 in
  Regular) RECIPE=(STYLE=Regular LEAN=R "${COMMON[@]}") ;;
  Bold) RECIPE=(STYLE=Bold LEAN=R "${BOLDENV[@]}" UID_TAG=prototype-bi-bold2 BOLD_HATAF_GAP=46 "${COMMON[@]}") ;;
  Italic-right) RECIPE=(STYLE=Italic LEAN=R UID_TAG=prototype-bi-bold2-m METEG_FIT=1 "${COMMON[@]}") ;;
  Italic-left) RECIPE=(STYLE=Italic LEAN=L "${COMMON[@]}") ;;
  BoldItalic-right) RECIPE=(STYLE=BoldItalic LEAN=R "${BOLDENV[@]}" UID_TAG=prototype-bi-bold2-m BOLD_HATAF_GAP=54 METEG_FIT=1 "${COMMON[@]}") ;;
  BoldItalic-left) RECIPE=(STYLE=BoldItalic LEAN=L "${BOLDENV[@]}" UID_TAG=prototype-bi-bold2 BOLD_HATAF_GAP=46 "${COMMON[@]}") ;;
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
  for name in Italic-right Italic-left BoldItalic-right BoldItalic-left; do
    build_one "$name"
    got=$(sha "$WORK/$name.ttf")
    want=$(gate_hash "$name")
    if [ "$got" = "$want" ]; then
      echo "ok   $name-A-L2  $got  (the gated file; not embedded)"
    else
      echo "FAIL $name-A-L2  $got  want $want"
      fail=1
    fi
  done
  exit $fail
fi

STYLES=(Regular Bold)
# The stock italics, named into the built family: no recipe, the stock file in and
# the built Regular's family name out.
RENAMED=(Italic BoldItalic)
fail=0
for name in "${STYLES[@]}"; do
  build_one "$name"
done
for name in "${RENAMED[@]}"; do
  if ! py -- scripts/font-family-records.py "$JBM/JetBrainsMono-$name.ttf" "$WORK/Regular.ttf" \
    "$WORK/$name.ttf" >"$WORK/$name.log" 2>&1; then
    echo "FAIL name $name; the log's tail:" >&2
    tail -n 30 "$WORK/$name.log" >&2
    exit 1
  fi
done
for name in "${STYLES[@]}" "${RENAMED[@]}"; do
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
  echo "step name scripts/font-family-records.py JetBrainsMono-<style>.ttf Regular.ttf <style>.ttf, for ${RENAMED[*]}"
  local f
  while IFS= read -r f; do
    echo "input $(sha "$f") $f"
  done < <(find "$HEB" -type f -not -name '*.pyc' -not -name .DS_Store -not -path '*/__pycache__/*' | LC_ALL=C sort)
  for name in "${STYLES[@]}" "${RENAMED[@]}"; do
    echo "input $(sha "$JBM/JetBrainsMono-$(base_of "$name").ttf") $JBM/JetBrainsMono-$(base_of "$name").ttf"
  done
  echo "input $(sha scripts/font-family-records.py) scripts/font-family-records.py"
  for w in 400 700; do
    echo "intermediate $(sha "$WORK/cas$w.ttf") cas$w.ttf (the wght $w instance; not committed)"
  done
  for name in "${STYLES[@]}" "${RENAMED[@]}"; do
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
  for name in "${STYLES[@]}" "${RENAMED[@]}"; do cp "$WORK/$name.ttf" "$OUT/$name.ttf"; done
  cp "$WORK/MANIFEST" "$OUT/MANIFEST"
  grep '^output ' "$OUT/MANIFEST"
  echo "ok: wrote ${STYLES[*]} ${RENAMED[*]} into $OUT, each the gated bytes, and $OUT/MANIFEST"
  exit 0
fi

# --check
if [ ! -f "$OUT/MANIFEST" ]; then
  echo "FAIL: no $OUT/MANIFEST to check against" >&2
  exit 1
fi
for name in "${STYLES[@]}" "${RENAMED[@]}"; do
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
