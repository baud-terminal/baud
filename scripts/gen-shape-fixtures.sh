#!/usr/bin/env bash
# Golden shaping fixtures from hb-shape (HarfBuzz), one line per (face, text):
#
#   <face file>\t<codepoints, hex, space-separated>\t<hb-shape output>
#
# where the output is hb-shape's text form with positions and glyph names
# off: `[gid=cluster|gid=cluster|...]`, clusters being codepoint indices.
#
# Features: the grid set (D8) -- `ccmp` and `calt` on, prose ligatures and
# everything a monospace grid cannot use off -- so the oracle shapes the
# same feature set `baud_font::Features::GRID` names.
#
# Regenerate under the hb-shape a fixture's header names, which
# `tools/hb-shape.nix` pins (14.5.1 and 13.2.1):
#
#   nix-shell tools/hb-shape.nix --argstr harfbuzz 14.5.1 --run \
#     'BAUD_ORACLE=<bin> scripts/gen-shape-fixtures.sh'
#
# Run it under a UTF-8 locale (LANG=en_US.UTF-8): under the C locale
# hb-shape refuses every non-ASCII --text with "Invalid byte sequence in
# conversion input".
#
# and keep each file from the run whose version its header names. What the
# script computes between the hb-shape calls -- the digests, the code
# points, the renamed tables, the `morx` defaults and the word lists -- is
# `tools/baud-oracle`'s `shape-helpers`; BAUD_ORACLE is its `baud-oracle`
# bin, built in `tools/shell.nix`.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -x "${BAUD_ORACLE:-}" ] || {
  echo "BAUD_ORACLE must name tools/baud-oracle's baud-oracle bin: nix-shell tools/shell.nix --run 'cargo build --release --manifest-path tools/Cargo.toml --bin baud-oracle'" >&2
  exit 1
}
helper() { "$BAUD_ORACLE" shape-helpers "$@"; }

# Devanagari through the Indic engine (`baud-font/src/indic.rs`,
# `[[source: ms-devanagari-script-development]]`): the README's line, the
# specification's own examples -- akhand, reph, rakaar, half forms, nukta,
# ZWJ and ZWNJ, the pre-base matra and broken clusters -- and every word of
# two pages of Premchand's Bade Ghar ki Beti (premchand-saptasaroj-11 and
# -12, fetched by their pinned oldid and refused unless they hash as the
# manifest says). Faces: Noto Sans Devanagari 2.007, pinned by tag and
# sha256 as scripts/readme-capture.sh pins its Noto faces and fetched once
# into the same cache outside the repository (NOTO_CACHE, default
# ~/.cache/baud-readme), named `noto-cache/` in the path column, which the
# test reads from that cache; Kohinoor Devanagari, macOS's own Devanagari face, and ITF
# Devanagari, both `dev2`; Shree Devanagari 714 and Arial Unicode MS,
# `deva` alone, the old specification; the built Regular and Bold, which
# carry Noto Sans Devanagari merged in (glyphs/languages/devanagari/), its
# variable font at wght 400 and 700; and stock JetBrains Mono Regular, with
# no Devanagari, the control. The specification's examples are followed by
# lines that reach a pre-base matra moved past a halant that stays (ट्कि,
# where Noto Sans Devanagari has no half form of TTA) and a lookup a
# syllable boundary stops (ट्रयही on Shree Devanagari 714).
# Shaped with every feature HarfBuzz applies but `liga`, `clig` and `curs`,
# which the grid does not take, the language English, which none of these
# faces has a language system for, so each is shaped with its default.
# Positions kept. Columns as `arabic.txt`'s. Run alone with
# `scripts/gen-shape-fixtures.sh devanagari`; hb-shape is HarfBuzz 14.5.1's,
# the `-dev` output of /nix/store/vxihlv2pafxmwlxafg8dsnfag6qcyayz-harfbuzz-14.5.1.drv,
# which `tools/hb-shape.nix` pins. The pages are fetched here and the
# words taken from them by `shape-helpers devanagari`, offline.
devanagari_fixtures() {
  local out=baud-font/tests/shape/devanagari.txt
  local features="-liga,-clig,-curs"
  local noto_url=https://raw.githubusercontent.com/notofonts/notofonts.github.io/noto-monthly-release-2026.10.01/fonts/NotoSansDevanagari/unhinted/ttf/NotoSansDevanagari-Regular.ttf
  local noto_sha=9c7d935139ea6a1e6ad9dbac4f6d27ece1e04bca8123c8888d00a0f9df4724cd
  local cache=${NOTO_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/baud-readme}
  local noto=$cache/NotoSansDevanagari-Regular.ttf
  if [ ! -e "$noto" ]; then
    mkdir -p "$cache"
    curl -sSfL -o "$noto.part" "$noto_url" || exit 1
    mv "$noto.part" "$noto"
  fi
  [ "$(shasum -a 256 "$noto" | cut -d' ' -f1)" = "$noto_sha" ] || {
    echo "$noto does not hash to its pin" >&2
    exit 1
  }
  local pages page=0 url sha
  pages=$(mktemp -d)
  while read -r url sha; do
    page=$((page + 1))
    curl -sSfL -A "baud gen-shape-fixtures.sh" -o "$pages/$page" "$url" || exit 1
    [ "$(shasum -a 256 "$pages/$page" | cut -d' ' -f1)" = "$sha" ] || {
      echo "$url does not hash as pinned" >&2
      exit 1
    }
  done <<'URLS'
https://hi.wikisource.org/w/index.php?title=%E0%A4%AA%E0%A5%83%E0%A4%B7%E0%A5%8D%E0%A4%A0%3A%E0%A4%B8%E0%A4%AA%E0%A5%8D%E0%A4%A4%E0%A4%B8%E0%A4%B0%E0%A5%8B%E0%A4%9C.djvu/%E0%A5%A7%E0%A5%A7&action=raw&oldid=610939 5c357de229988ff7e2502a03a2961baa4351de123024c959e89edff81003ca03
https://hi.wikisource.org/w/index.php?title=%E0%A4%AA%E0%A5%83%E0%A4%B7%E0%A5%8D%E0%A4%A0%3A%E0%A4%B8%E0%A4%AA%E0%A5%8D%E0%A4%A4%E0%A4%B8%E0%A4%B0%E0%A5%8B%E0%A4%9C.djvu/%E0%A5%A7%E0%A5%A8&action=raw&oldid=610947 0eb29482310718fd81c53d1f8df6a73029241439b90709b74511086579a30ad3
URLS
  local texts
  texts=$(
    helper devanagari "$pages/1" "$pages/2" <<'SPEC'
टर्मिनल हर अक्षर को ग्रिड में रखता है।
टर्मिनल
क्ष
त्र
ज्ञ
र्क
कि
क्ति
र्कि
क्क
क्त
क्य
द्ध
द्म
द्य
ह्म
ह्न
श्र
श्च
क्र
प्र
ट्र
ड्र
क्ष्म
स्त्री
र्द्मि
र्द्धि
र्त्स्न्य
कर्म
धर्म
कार्य
पत्नी
प्रेम
श्रीकण्ठ
अङ्ग
सङ्कोच
ट्ट
ड्ड
क्त्र
न्त्र
ष्ट्र
संस्कृत
हिंदी
हिन्दी
हँसी
दुःख
क़
क़
ज़
फ़
ड़
ढ़
ऩ
ऩ
ऱ
ऴ
बडे़
क़्ष
क्‍ष
क्‌ष
र्‍य
द्‍
द्‍ध
ट्‌य
ट्‍य
कि‍
ि
्
ं
क्ि
अा
आे
उु
एे
र्इ
 ि
◌ि
१२३
ॐ
ऽ
क॑
क्
र्
रु
रू
हृ
कृ
ॅ
कॉ
क्‌
ट्कि
ठ्कि
ड्कि
ढ्कि
द्कि
ड़्कि
ङ्कि
ट्‍कि
ट्रयही
ड्रयही
ष्ट्रयही
ट्रयदि
SPEC
  ) || exit 1
  rm -rf "$pages"
  {
    echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
    echo "# features: $features; script deva, left to right, language en"
    echo "# columns: path, face index, fnv64 of the file, features, codepoints, hb-shape"
    echo "# noto-cache/: $noto_url, sha256 $noto_sha"
    for spec in "noto-cache/NotoSansDevanagari-Regular.ttf|0" \
      "/System/Library/Fonts/Kohinoor.ttc|0" \
      "/System/Library/Fonts/Supplemental/ITFDevanagari.ttc|0" \
      "/System/Library/Fonts/Supplemental/Shree714.ttc|0" \
      "/System/Library/Fonts/Supplemental/Arial Unicode.ttf|0" \
      "baud-app/fonts/built/Regular.ttf|0" \
      "baud-app/fonts/built/Bold.ttf|0" \
      "baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf|0"; do
      IFS='|' read -r named index <<<"$spec"
      face=${named/#noto-cache/$cache}
      [ -f "$face" ] || {
        echo "missing $face" >&2
        continue
      }
      fnv=$(helper fnv64 "$face")
      while IFS= read -r cps; do
        [ -z "$cps" ] && continue
        shaped=$(hb-shape --no-glyph-names --face-index="$index" --features="$features" --script=deva --direction=ltr --language=en -u "$cps" "$face")
        printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$named" "$index" "$fnv" "$features" "$cps" "$shaped"
      done <<<"$texts"
    done
  } >"$out"
  wc -l "$out"
}
# Noto Sans Devanagari's variable file, read narrower on its `wdth` axis as
# a Devanagari word too wide for its columns is drawn (baud-render's
# word.rs): every Noto line of devanagari.txt shaped again by hb-shape at
# wdth 81.25 and at 62.5, the narrowest, with the same features, and kept
# with the variations in a column of their own. The file is pinned by its
# release tag and sha256 and fetched once into the same cache. Run alone
# with `scripts/gen-shape-fixtures.sh devanagari-wdth`, after devanagari.txt.
devanagari_wdth_fixtures() {
  local out=baud-font/tests/shape/devanagari-wdth.txt
  local features="-liga,-clig,-curs"
  local var_url='https://raw.githubusercontent.com/notofonts/notofonts.github.io/noto-monthly-release-2026.10.01/fonts/NotoSansDevanagari/unhinted/variable-ttf/NotoSansDevanagari%5Bwdth,wght%5D.ttf'
  local var_sha=9f82923465ef82fb86c6b99cbdd4947ba832fe3335948de815b18adfd75947c7
  local cache=${NOTO_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/baud-readme}
  local named='noto-cache/NotoSansDevanagari[wdth,wght].ttf'
  local face="$cache/NotoSansDevanagari[wdth,wght].ttf"
  if [ ! -e "$face" ]; then
    mkdir -p "$cache"
    curl -sSfL -o "$face.part" "$var_url" || exit 1
    mv "$face.part" "$face"
  fi
  [ "$(shasum -a 256 "$face" | cut -d' ' -f1)" = "$var_sha" ] || {
    echo "$face does not hash to its pin" >&2
    exit 1
  }
  local fnv
  fnv=$(helper fnv64 "$face")
  {
    echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
    echo "# features: $features; script deva, left to right, language en"
    echo "# columns: path, face index, fnv64 of the file, features, variations, codepoints, hb-shape"
    echo "# noto-cache/: $var_url, sha256 $var_sha"
    for wdth in 81.25 62.5; do
      grep '^noto-cache/NotoSansDevanagari-Regular.ttf' baud-font/tests/shape/devanagari.txt | cut -f5 |
        while IFS= read -r cps; do
          shaped=$(hb-shape --no-glyph-names --variations="wdth=$wdth" --features="$features" --script=deva --direction=ltr --language=en -u "$cps" "$face")
          printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$named" 0 "$fnv" "$features" "wdth=$wdth" "$cps" "$shaped"
        done
    done
  } >"$out"
  wc -l "$out"
}
# Every line of devanagari.txt shaped again by hb-shape at cluster level 2,
# HarfBuzz's CHARACTERS (`hb-buffer.h`), where no cluster value is merged:
# what a syllable whose cells are in two renditions is shaped at
# (`baud_font::ClusterLevel::Characters`), on devanagari.txt's faces and the
# built Italic and Bold Italic, the faces the renderer draws such a syllable
# from, with the same features. Run alone with
# `scripts/gen-shape-fixtures.sh devanagari-characters`, after devanagari.txt.
devanagari_characters_fixtures() {
  local out=baud-font/tests/shape/devanagari-characters.txt
  local features="-liga,-clig,-curs"
  local cache=${NOTO_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/baud-readme}
  {
    echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
    echo "# features: $features; script deva, left to right, language en; --cluster-level=2"
    echo "# columns: path, face index, fnv64 of the file, features, codepoints, hb-shape"
    grep '^# noto-cache/' baud-font/tests/shape/devanagari.txt
    for spec in "noto-cache/NotoSansDevanagari-Regular.ttf|0" \
      "/System/Library/Fonts/Kohinoor.ttc|0" \
      "/System/Library/Fonts/Supplemental/ITFDevanagari.ttc|0" \
      "/System/Library/Fonts/Supplemental/Shree714.ttc|0" \
      "/System/Library/Fonts/Supplemental/Arial Unicode.ttf|0" \
      "baud-app/fonts/built/Regular.ttf|0" \
      "baud-app/fonts/built/Bold.ttf|0" \
      "baud-app/fonts/built/Italic.ttf|0" \
      "baud-app/fonts/built/BoldItalic.ttf|0" \
      "baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf|0"; do
      IFS='|' read -r named index <<<"$spec"
      face=${named/#noto-cache/$cache}
      [ -f "$face" ] || {
        echo "missing $face" >&2
        continue
      }
      fnv=$(helper fnv64 "$face")
      grep '^baud-app/fonts/built/Regular.ttf' baud-font/tests/shape/devanagari.txt | cut -f5 |
        while IFS= read -r cps; do
          shaped=$(hb-shape --no-glyph-names --cluster-level=2 --face-index="$index" --features="$features" --script=deva --direction=ltr --language=en -u "$cps" "$face")
          printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$named" "$index" "$fnv" "$features" "$cps" "$shaped"
        done
    done
  } >"$out"
  wc -l "$out"
}
if [ "${1:-}" = devanagari ]; then
  devanagari_fixtures
  exit
fi
if [ "${1:-}" = devanagari-characters ]; then
  devanagari_characters_fixtures
  exit
fi
if [ "${1:-}" = devanagari-wdth ]; then
  devanagari_wdth_fixtures
  exit
fi
FEATURES="-liga,-clig,-kern,-locl,-rclt,-mark,-mkmk,-dlig,-hlig"
OUT=baud-font/tests/shape/gsub.txt
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1)"
  echo "# features: $FEATURES"
  for face in FiraCode-Regular.ttf JetBrainsMono-Regular.ttf BungeeTint-Regular.ttf; do
    while IFS= read -r text; do
      [ -z "$text" ] && continue
      cps=$(helper cps "$text")
      shaped=$(hb-shape --no-positions --no-glyph-names --features="$FEATURES" "baud-font/tests/fonts/$face" --text="$text")
      printf '%s\t%s\t%s\n' "$face" "$cps" "$shaped"
    done <<'TEXTS'
->
=>
!=
!==
===
==
<=
>=
<=>
<-
<--
-->
<!--
www
:=
::
:::
/*
*/
//
///
<<
>>
||
&&
++
--
**
!!
??
?.
?:
|>
<|
~>
<~
#!
#[
]#
%%
..
...
..<
.=
=~
=/=
<$>
<*>
>>=
=<<
a->b != c
fn main() -> Result<(), E> { x >= y && z <= w; }
let f = |x| x * 2;
if a == b || c != d { /* comment */ }
Fl fi fl ffi ffl Ff ft st
é ä ñ ô
éñü
x‍y
a️b
0xFF 1e10 3.14
i18n l10n
=>=>=>
->->
TEXTS
  done
} >"$OUT"
wc -l "$OUT"

# Mark positioning: the same oracle with the mark features on and the
# positions kept -- `[gid=cluster@dx,dy+ax|gid=cluster+ax]`, offsets in font
# units from the glyph's own pen position. The test translates HarfBuzz's
# frame (after the base's advance) into baud's (the cell origin).
POS_FEATURES="+mark,+mkmk,-liga,-clig,-calt,-kern,-locl,-rclt,-dlig,-hlig"
POS_OUT=baud-font/tests/shape/gpos.txt
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1)"
  echo "# features: $POS_FEATURES"
  for face in FiraCode-Regular.ttf JetBrainsMono-Regular.ttf BungeeTint-Regular.ttf; do
    while IFS= read -r text; do
      [ -z "$text" ] && continue
      cps=$(helper cps "$text")
      shaped=$(hb-shape --no-glyph-names --features="$POS_FEATURES" "baud-font/tests/fonts/$face" --text="$text")
      printf '%s\t%s\t%s\n' "$face" "$cps" "$shaped"
    done <<'TEXTS'
é
ä
ñ
ô
ǖ
ạ́
ố
ē
ǐ
ç
š ť
ạ̀
é̄
x̄ȳ
TEXTS
  done
} >"$POS_OUT"
wc -l "$POS_OUT"

# Fallback mark placement: the same faces with their `GPOS` table renamed
# away, so HarfBuzz takes its geometric fallback. `GPOZ` keeps the table
# directory sorted -- HarfBuzz binary-searches it, and an out-of-order tag
# loses `GSUB` as well, which a first cut of this block found the hard way -- the oracle for
# `Shaper::fallback_marks`. The face column names the committed file; the
# test renames the tag in memory the same way. Sequences are given as
# codepoints because most have no precomposed form on purpose.
FB_OUT=baud-font/tests/shape/fallback.txt
FB_DIR=$(mktemp -d)
trap 'rm -rf "$FB_DIR"' EXIT
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1)"
  echo "# features: $POS_FEATURES; GPOS renamed to GPOZ before shaping"
  for face in FiraCode-Regular.ttf JetBrainsMono-Regular.ttf; do
    helper rename-table "baud-font/tests/fonts/$face" "$FB_DIR/$face" GPOS GPOZ
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --features="$POS_FEATURES" -u "$cps" "$FB_DIR/$face")
      printf '%s\t%s\t%s\n' "$face" "$cps" "$shaped"
    done <<'SEQS'
0078 0301
0078 0323
0078 0327
0078 0308 0301
006D 0304 0301
0078 0323 0331
0058 0308
0061 0300 0323
0078 0315
0078 0316
006F 031B
0078 0361 0079
0078 035C 0079
0078 0345
0078 0334
0071 0301
006A 0323
0065 0301
0078 0302 0323 0301
0078 0320
0078 0306 0300
0067 0327
0078 0338
0078 0483
0430 0301
03B1 0301
0057 0302 0323
0069 0307 0301
SEQS
  done
} >"$FB_OUT"
wc -l "$FB_OUT"

# `morx`: the AAT faces macOS ships, which have no GSUB, shaped by HarfBuzz's
# AAT path. baud runs each chain's default flags and nothing else. HarfBuzz
# reads `feat` and requests `liga`, `clig`, `calt` and `rlig` by default,
# and a request moves the flags off the defaults in either direction --
# Menlo's common ligatures are off by default and `+liga` turns them on,
# Helvetica's are on and `-liga` would turn them off. So the flags HarfBuzz
# would reach are simulated per face from the chain's own feature table
# (`Chain::compile_flags`), and only the requests that move them are
# cancelled; `kern` is always off, because `kerx` kerning is not shaping.
# Each line names the file, the face index, an FNV-1a digest of the file --
# so the test refuses to compare against a font the OS has since replaced
# -- and the features passed. The chains and `feat` are read by
# `shape-helpers morx-features`, which reads no font through baud-font.
# The `--features` that leave HarfBuzz at a face's `morx` chain defaults:
# its default requests that would move a flag cancelled, `kern` off, and
# a failure if a request moves one either way.
morx_features() {
  helper morx-features "$1" "$2"
}

MX_OUT=baud-font/tests/shape/morx.txt
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "# columns: path, face index, fnv64 of the file, hb-shape features, codepoints, hb-shape"
  shape_mx() {
    face="$1"
    index="$2"
    direction="${3:-}"
    [ -f "$face" ] || {
      echo "missing $face" >&2
      return
    }
    fnv=$(helper fnv64 "$face")
    features=$(morx_features "$face" "$index") || exit 1
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --shapers=ot --face-index="$index" --features="$features" ${direction:+"$direction"} -u "$cps" "$face")
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$face" "$index" "$fnv" "$features${direction:+ $direction}" "$cps" "$shaped"
    done
  }
  shape_mx "/System/Library/Fonts/Apple Color Emoji.ttc" 0 <<'SEQS'
1F468 200D 1F469 200D 1F467
1F468 200D 1F469 200D 1F467 200D 1F466
1F469 200D 1F469 200D 1F466
1F468 200D 1F468 200D 1F467 200D 1F467
1F1EC 1F1E7
1F1FA 1F1F8
1F1EF 1F1F5
1F1E8 1F1F3 1F1E9 1F1EA
1F3F4 E0067 E0062 E0065 E006E E0067 E007F
1F3F4 E0067 E0062 E0073 E0063 E0074 E007F
1F44D 1F3FD
1F44D 1F3FB
1F44B 1F3FF
0031 FE0F 20E3
0023 FE0F 20E3
002A 20E3
0031 20E3
2764 FE0F
2764
00A9 FE0F
1F469 1F3FB 200D 1F91D 200D 1F468 1F3FF
1F9D1 200D 1F91D 200D 1F9D1
1F9D1 1F3FC 200D 1F91D 200D 1F9D1 1F3FE
1F468 1F3FB
1F600
1F3F3 FE0F 200D 1F308
1F3F3 FE0F 200D 26A7 FE0F
1F3F4 200D 2620 FE0F
1F468 200D 1F469
1F468 200D 2764 FE0F 200D 1F468
1F469 200D 2764 FE0F 200D 1F48B 200D 1F468
1F9D1 200D 1F9B0
1F469 200D 1F9B1
1F3C3 200D 2640 FE0F
1F3C3 1F3FD 200D 2642 FE0F
1F468 200D 1F4BB
1F469 1F3FE 200D 1F52C
1F441 FE0F 200D 1F5E8 FE0F
1F415 200D 1F9BA
1F636 200D 1F32B FE0F
1F62E 200D 1F4A8
1F468 200D 1F469 200D 1F467 1F600
1F600 1F601 1F602
0041 1F600 0042
1F46A
1F46B 1F3FB
1F9D1 200D 1F373
1F468 1F3FB 200D 1F9B3
26A0 FE0F
SEQS
  # A right-to-left run on the same face: a Hebrew letter leads, so
  # hb-shape guesses Hebrew and right to left, and a subtable in layout
  # order walks the run reversed while one in logical order (coverage bit
  # 28; chain subtables 1, 2 and 5 to 15 here) does not
  # (`[[source: apple-ttrm-morx]]`). The letter has no glyph here; it sets
  # the run's script and direction, as the cell before an emoji does.
  shape_mx "/System/Library/Fonts/Apple Color Emoji.ttc" 0 <<'SEQS'
05D0 1F468 200D 1F469 200D 1F467
05D0 1F468 200D 1F469 200D 1F467 200D 1F466
05D0 1F1EC 1F1E7
05D0 1F3F4 E0067 E0062 E0073 E0063 E0074 E007F
05D0 1F44B 1F3FD
05D0 0031 FE0F 20E3
05D0 1F3F3 FE0F 200D 1F308
05D0 1F3C3 1F3FD 200D 2642 FE0F
SEQS
  # Hebrew with no letter -- a maqaf -- and a number: a digit or a
  # regional indicator is laid out left to right (UAX #9's levels), which
  # is baud's rule for such a run. HarfBuzz applies it only when its caller
  # asks for left to right (`hb_ensure_native_direction`), so these are
  # shaped with `--direction=ltr`, which the features column records. The
  # flag pair walked the other way is another flag.
  shape_mx "/System/Library/Fonts/Apple Color Emoji.ttc" 0 --direction=ltr <<'SEQS'
05BE 1F1EC 1F1E7
05BE 1F1FA 1F1F8
05BE 0031 FE0F 20E3
SEQS
  for spec in "/System/Library/Fonts/Menlo.ttc 0" "/System/Library/Fonts/Monaco.ttf 0" "/System/Library/Fonts/Courier.ttc 0" "/System/Library/Fonts/Helvetica.ttc 0" "/System/Library/Fonts/Geneva.ttf 0" "/System/Library/Fonts/Times.ttc 0"; do
    shape_mx "${spec% *}" "${spec##* }" <<'SEQS'
0066 0069
0066 0066 0069
0066 006C
0065 0301
0078 0301
0061 0323 0301
0041 030A
0031 002F 0032
0033 002F 0034
0031 2044 0034
0054 004D
0028 0063 0029
002E 002E 002E
002D 002D
0061 0062 0063
0049 004A
0074 0068
0073 0074
0063 0327
0067 0327
006F 0308 0301
0061 0300 0323
0031 0032 0033
0071 0075
SEQS
  done
} >"$MX_OUT"
wc -l "$MX_OUT"

# Script selection: text in a script the face has a Script table for is
# shaped with that table, else `DFLT` (`[[source: ms-opentype-chapter2]]`,
# ScriptList table). hb-shape is left to guess the script and the direction
# from the text, as it does for a line: the first character that names a
# script decides both (`guess_segment_properties`), so a Hebrew line is
# shaped `hebr` and right to left and printed in visual order, which the test
# reverses into the logical order baud draws in. Every line is one script
# once UAX #24 settles its Common characters, so hb-shape's one guess and
# baud's itemisation agree on the run. The features are the grid set --
# `ccmp`, `calt`, `mark` and `mkmk` -- with every other default HarfBuzz
# applies turned off. Columns as `morx.txt`: path (relative to the
# repository, or a macOS system face), face index, fnv64 of the file,
# codepoints, hb-shape. A cluster carries at most one mark; two or more
# Hebrew points in one cluster are `order.txt`'s, below. No line holds a bracket:
# HarfBuzz mirrors one in a right-to-left run, which is bidi (UAX #9 L4),
# not script selection, and baud has no bidi; a pair's run is `brackets.txt`'s,
# at the end. Nor does one hold a programming
# ligature inside Hebrew: HarfBuzz shapes the whole line right to left, so
# `->` meets JetBrains Mono's `calt` reversed and takes another glyph (883
# where a left-to-right run takes 881), which is direction, not script.
SC_OUT=baud-font/tests/shape/script.txt
SC_FEATURES="-abvm,-blwm,-locl,-rlig,-clig,-curs,-dist,-kern,-liga,-rclt,-rvrn,-dlig,-hlig"
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "# features: $SC_FEATURES"
  echo "# columns: path, face index, fnv64 of the file, codepoints, hb-shape (script and direction guessed)"
  for face in "baud-app/fonts/built/Regular.ttf" "/System/Library/Fonts/Supplemental/Courier New.ttf"; do
    [ -f "$face" ] || {
      echo "missing $face" >&2
      continue
    }
    fnv=$(helper fnv64 "$face")
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --features="$SC_FEATURES" -u "$cps" "$face")
      printf '%s\t0\t%s\t%s\t%s\n' "$face" "$fnv" "$cps" "$shaped"
    done <<'SEQS'
05E9 05DC 05D5 05DD
05E2 05D1 05E8 05D9 05EA
05D1 05BC
05D1 05B8
05E9 05C1
05E9 05C2
05D4 05B4
05DC 05B9
05D0 05B7
05DB 05B0
05D5 05BC
05E9 05B8 05DC 05D5 05B9 05DD
05E9 05DC 05D5 05DD 002C 0020 0031 0032 0033
002D 003E
003D 003E
0021 003D
0031 0032 0033
0066 0069
0061 0062 0063
0061 0020 002D 003E 0020 0062
SEQS
  done
} >"$SC_OUT"
wc -l "$SC_OUT"

# Reverse chaining (`GSUB` type 8, "processed in reverse order",
# `[[source: ms-opentype-gsub]]`) and contexts: Noto Sans Coptic's `ccmp`
# carries a combining overline over a word, and an overline that a
# capital's follows takes the capital's height, from the end of the word
# back to its start, which only a reverse chain can carry. Each letter and
# its overline are one cell. Columns and features as `script.txt`'s; the
# built Regular, with no Coptic, is the control.
RV_OUT=baud-font/tests/shape/reverse.txt
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "# features: $SC_FEATURES"
  echo "# columns: path, face index, fnv64 of the file, codepoints, hb-shape (script and direction guessed)"
  for face in "baud-app/fonts/built/Regular.ttf" "/System/Library/Fonts/Supplemental/NotoSansCoptic-Regular.ttf"; do
    [ -f "$face" ] || {
      echo "missing $face" >&2
      continue
    }
    fnv=$(helper fnv64 "$face")
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --features="$SC_FEATURES" -u "$cps" "$face")
      printf '%s\t0\t%s\t%s\t%s\n' "$face" "$fnv" "$cps" "$shaped"
    done <<'SEQS'
2C81 0305
2C80 0305
2C81 0305 2C83 0305
2C80 0305 2C81 0305
2C81 0305 2C80 0305
2C81 0305 2C81 0305 2C80 0305
2C81 0305 2C83 0305 2C85 0305 2C80 0305
2C81 0305 2C83 0305 2C85 0305 2C87 0305 2C89 0305
2C81 033F 2C80 033F
2C81 0305 0020 2C80 0305
SEQS
  done
} >"$RV_OUT"
wc -l "$RV_OUT"

# Mark order: two or more Hebrew points in one cluster, which baud sorts
# by HarfBuzz's modified combining classes
# (`[[source: harfbuzz-hb-unicode]]`) before the face sees them. The
# clusters are the thirteen of the hand-typed Genesis 1:1-5, listed below,
# whose canonical order is not HarfBuzz's, and shin + qamats + shin dot,
# the cluster Courier New's `ccmp` was found to need in this order;
# each in its canonical spelling and then in HarfBuzz's, the two orders a
# text arrives in, which are canonically equivalent and shape alike.
# Columns and features as `script.txt`.
OR_OUT=baud-font/tests/shape/order.txt
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "# features: $SC_FEATURES"
  echo "# columns: path, face index, fnv64 of the file, codepoints, hb-shape (script and direction guessed)"
  for face in "baud-app/fonts/built/Regular.ttf" "/System/Library/Fonts/Supplemental/Courier New.ttf"; do
    [ -f "$face" ] || {
      echo "missing $face" >&2
      continue
    }
    fnv=$(helper fnv64 "$face")
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --features="$SC_FEATURES" -u "$cps" "$face")
      printf '%s\t0\t%s\t%s\t%s\n' "$face" "$fnv" "$cps" "$shaped"
    done <<'SEQS'
05E9 05B8 05C1
05E9 05C1 05B8
05E9 05B6 05C1
05E9 05C1 05B6
05E4 05B0 05BC
05E4 05BC 05B0
05D9 05B7 05BC
05D9 05BC 05B7
05D1 05B0 05BC
05D1 05BC 05B0
05E9 05B4 05C1
05E9 05C1 05B4
05D1 05B8 05BC
05D1 05BC 05B8
05E9 05B8 05BC 05C1
05E9 05C1 05BC 05B8
05DE 05B8 05BC
05DE 05BC 05B8
05D9 05B9 05BC
05D9 05BC 05B9
05DB 05B4 05BC
05DB 05BC 05B4
05D3 05B5 05BC
05D3 05BC 05B5
05D1 05B5 05BC
05D1 05BC 05B5
05D9 05B4 05BC
05D9 05BC 05B4
SEQS
  done
} >"$OR_OUT"
wc -l "$OR_OUT"

# Hebrew points placed by the fallback: on a face with no `GPOS`, HarfBuzz
# places each point by the class `recategorize_combining_class` gives it
# (`[[source: harfbuzz-hb-ot-shape-fallback]]`) -- the vowels and meteg
# below, rafe attached above, shin dot above right, sin dot and holam above
# left, varika above, dagesh where it is. The built Regular has its `GPOS`
# renamed to `GPOZ` as `fallback.txt`'s faces do, and the test makes the
# same rename. Raanana, New Peninim MT, Lucida Grande and Corsiva Hebrew
# carry no `GPOS` at all and shape through `morx`, so they are shaped at
# their chains' default flags, as `morx.txt`'s faces are, and the run stops
# if HarfBuzz's defaults would move one: `SC_FEATURES`' `-rlig` turns
# Raanana's default-on flag 0x2 off and `-liga` Lucida Grande's, and baud
# runs the defaults. Their ligature subtables set coverage bit 30 and not
# bit 28, reverse *layout* order (`[[source: apple-ttrm-morx]]`), which on a
# right-to-left run is logical order. Every class 10-26 on its
# own, a rafe over lamed's ascender and a sheva under final pe's descender,
# then points that share a placement class (sheva and meteg below, sin dot
# and holam above left) and the Genesis clusters of `order.txt`, in
# canonical order. Columns and features as `script.txt`; the fnv64 is of
# the file as committed, before a rename.
HF_OUT=baud-font/tests/shape/fallback-hebrew.txt
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "# features: $SC_FEATURES, a repository face's GPOS renamed to GPOZ before shaping; a system face -kern, its morx chains at their default flags"
  echo "# columns: path, face index, fnv64 of the file, codepoints, hb-shape (script and direction guessed)"
  for spec in "baud-app/fonts/built/Regular.ttf:0" \
    "/System/Library/Fonts/Supplemental/Raanana.ttc:0" \
    "/System/Library/Fonts/Supplemental/NewPeninimMT.ttc:0" \
    "/System/Library/Fonts/LucidaGrande.ttc:0" \
    "/System/Library/Fonts/LucidaGrande.ttc:1" \
    "/System/Library/Fonts/Supplemental/Corsiva.ttc:0" \
    "/System/Library/Fonts/Supplemental/Corsiva.ttc:1"; do
    face=${spec%:*}
    index=${spec##*:}
    [ -f "$face" ] || {
      echo "missing $face" >&2
      continue
    }
    fnv=$(helper fnv64 "$face")
    shaped_face=$face
    features=$SC_FEATURES
    case $face in
    /*)
      features=$(morx_features "$face" "$index") || exit 1
      [ "$features" = "-kern" ] || {
        echo "$face $index: HarfBuzz's defaults move a flag ($features)" >&2
        exit 1
      }
      ;;
    *)
      shaped_face="$FB_DIR/hebrew-$(basename "$face")"
      helper rename-table "$face" "$shaped_face" GPOS GPOZ
      ;;
    esac
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --features="$features" --face-index="$index" -u "$cps" "$shaped_face")
      printf '%s\t%s\t%s\t%s\t%s\n' "$face" "$index" "$fnv" "$cps" "$shaped"
    done <<'SEQS'
05D1 05B0
05D1 05B1
05D1 05B2
05D1 05B3
05D1 05B4
05D1 05B5
05D1 05B6
05D1 05B7
05D1 05B8
05D1 05C7
05D1 05B9
05D5 05BA
05D1 05BB
05D1 05BC
05D1 05BD
05D1 05BF
05E9 05C1
05E9 05C2
05D1 FB1E
05DC 05BF
05E3 05B0
05D1 05B0 05BD
05D1 05B8 05BD
05E9 05C2 05B9
05E9 05B8 05C1
05E9 05B6 05C1
05E4 05B0 05BC
05D9 05B7 05BC
05D1 05B0 05BC
05E9 05B4 05C1
05D1 05B8 05BC
05E9 05B8 05BC 05C1
05DE 05B8 05BC
05D9 05B9 05BC
05DB 05B4 05BC
05D3 05B5 05BC
05D1 05B5 05BC
05D9 05B4 05BC
SEQS
  done
} >"$HF_OUT"
wc -l "$HF_OUT"

# Hebrew on three more macOS faces, every distinct cluster of the pointed
# Genesis 1:1-5 and Pango corpora (`baud-font/tests/shape/hebrew-clusters.txt`): Arial Unicode MS, whose `GPOS` has no Script table
# a Hebrew run selects, so HarfBuzz's Hebrew shaper turns `GPOS` off
# (`gpos_tag`, `[[source: harfbuzz-hb-ot-shaper-hebrew]]`), places the
# points by its fallback and composes the presentation forms; Courier New,
# whose `hebr` `mark` feature runs single, pair and chained-context
# positioning; and Arial Hebrew, which carries `GSUB` and `morx` both and
# which HarfBuzz shapes through `morx` (`_hb_apply_morx()`,
# `[[source: harfbuzz-hb-ot-shape]]`). The OpenType faces take
# `SC_FEATURES`; Arial Hebrew is shaped at its chains' default flags, as
# `fallback-hebrew.txt`'s `morx` faces are. The same clusters on each of the
# four built faces follow, on every platform. Columns as `script.txt`.
HX_OUT=baud-font/tests/shape/hebrew-faces.txt
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "# features: $SC_FEATURES for an OpenType face; -kern for a face HarfBuzz shapes through morx, its chains at their default flags"
  echo "# columns: path, face index, fnv64 of the file, codepoints, hb-shape (script and direction guessed)"
  for spec in "/System/Library/Fonts/Supplemental/Arial Unicode.ttf:0" \
    "/System/Library/Fonts/Supplemental/Courier New.ttf:0" \
    "/System/Library/Fonts/ArialHB.ttc:0"; do
    face=${spec%:*}
    index=${spec##*:}
    [ -f "$face" ] || {
      echo "missing $face" >&2
      continue
    }
    fnv=$(helper fnv64 "$face")
    features=$SC_FEATURES
    case $face in
    *ArialHB.ttc)
      features=$(morx_features "$face" "$index") || exit 1
      [ "$features" = "-kern" ] || {
        echo "$face $index: HarfBuzz's defaults move a flag ($features)" >&2
        exit 1
      }
      ;;
    esac
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --features="$features" --face-index="$index" -u "$cps" "$face")
      printf '%s\t%s\t%s\t%s\t%s\n' "$face" "$index" "$fnv" "$cps" "$shaped"
    done <baud-font/tests/shape/hebrew-clusters.txt
  done
  # And the built Regular, on every platform: a Latin mark on a Hebrew
  # letter, which no lookup of the face places, so it sits where the pen
  # leaves it -- right to left, on its letter's left edge, not a cell right.
  face=baud-app/fonts/built/Regular.ttf
  fnv=$(helper fnv64 "$face")
  while IFS= read -r cps; do
    [ -z "$cps" ] && continue
    shaped=$(hb-shape --no-glyph-names --features="$SC_FEATURES" -u "$cps" "$face")
    printf '%s\t0\t%s\t%s\t%s\n' "$face" "$fnv" "$cps" "$shaped"
  done <<'SEQS'
05D0 0301
05D0 0308
05E9 0323
05D0 05B8 0301
SEQS
  # And every built face, on every platform: every cluster of the same
  # corpora, the pointed Genesis 1:1-5 among them, on the upright, bold and
  # italic Hebrew. The 99 clusters that carry a cantillation mark, which
  # the built faces do not map, shape it to the face's `.notdef`, which
  # keeps its advance in HarfBuzz as in baud's shaper: right to left it
  # stands a cell left of its letter in the shaper's output.
  for face in baud-app/fonts/built/Regular.ttf baud-app/fonts/built/Bold.ttf baud-app/fonts/built/Italic.ttf baud-app/fonts/built/BoldItalic.ttf; do
    fnv=$(helper fnv64 "$face")
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --features="$SC_FEATURES" -u "$cps" "$face")
      printf '%s\t0\t%s\t%s\t%s\n' "$face" "$fnv" "$cps" "$shaped"
    done <baud-font/tests/shape/hebrew-clusters.txt
    # And the characters baud draws itself (build/cover.py), alone and
    # carrying a point: placed by the build's own anchor search, so held to
    # HarfBuzz like the letters; the presentation forms U+FB1D-FB4F, each
    # alone, and the decomposition of each canonical one, whose drawing its
    # composite must equal; varika on the letters it is read with; and each
    # cantillation mark on bet, each below one also beside qamats, and a
    # mark beside meteg, after holam and beside the shin dot.
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --features="$SC_FEATURES" -u "$cps" "$face")
      printf '%s\t0\t%s\t%s\t%s\n' "$face" "$fnv" "$cps" "$shaped"
    done <<'COVER'
20AA
05C6
05EF
05F0
05F1
05F2
05EF 05B4
05F0 05B8
05F1 05B7
05F2 05B7
05F2 05B8 05BD
FB1D
FB1E
FB1F
FB20
FB21
FB22
FB23
FB24
FB25
FB26
FB27
FB28
FB29
FB2A
FB2B
FB2C
FB2D
FB2E
FB2F
FB30
FB31
FB32
FB33
FB34
FB35
FB36
FB38
FB39
FB3A
FB3B
FB3C
FB3E
FB40
FB41
FB43
FB44
FB46
FB47
FB48
FB49
FB4A
FB4B
FB4C
FB4D
FB4E
FB4F
05D9 05B4
05F2 05B7
05E9 05C1
05E9 05C2
05E9 05BC 05C1
05E9 05BC 05C2
05D0 05B7
05D0 05B8
05D0 05BC
05D1 05BC
05D2 05BC
05D3 05BC
05D4 05BC
05D5 05BC
05D6 05BC
05D8 05BC
05D9 05BC
05DA 05BC
05DB 05BC
05DC 05BC
05DE 05BC
05E0 05BC
05E1 05BC
05E3 05BC
05E4 05BC
05E6 05BC
05E7 05BC
05E8 05BC
05E9 05BC
05EA 05BC
05D5 05B9
05D1 05BF
05DB 05BF
05E4 05BF
05D1 FB1E
05DB FB1E
05E4 FB1E
05D1 0591
05D1 0592
05D1 0593
05D1 0594
05D1 0595
05D1 0596
05D1 0597
05D1 0598
05D1 0599
05D1 059A
05D1 059B
05D1 059C
05D1 059D
05D1 059E
05D1 059F
05D1 05A0
05D1 05A1
05D1 05A2
05D1 05A3
05D1 05A4
05D1 05A5
05D1 05A6
05D1 05A7
05D1 05A8
05D1 05A9
05D1 05AA
05D1 05AB
05D1 05AC
05D1 05AD
05D1 05AE
05D1 05AF
05D1 05B8 0591
05D1 05B8 0596
05D1 05B8 059A
05D1 05B8 059B
05D1 05B8 05A2
05D1 05B8 05A3
05D1 05B8 05A4
05D1 05B8 05A5
05D1 05B8 05A6
05D1 05B8 05A7
05D1 05B8 05AA
05D1 05B8 05AD
05D1 05B8 05BD 05A5
05D0 05B9 0599
05E9 05C1 05A0
COVER
  done
} >"$HX_OUT"
wc -l "$HX_OUT"

# Bracket pairs: a pair takes the script of the text enclosing it
# (`[[source: uax24]]` 5.1, "paired punctuation ... should therefore match
# the script of the enclosing text"), matched as UAX #9's BD16 matches one
# (`[[source: uax9-bidi]]`), so a run is cut on both sides of what the pair
# holds. Each line is written as the runs UAX #24 gives it, `|` between
# them, with each run's script; hb-shape shapes one run at a time with that
# script given, left to right, which is the run baud shapes when its
# itemisation agrees. hb-shape does no itemising of its own -- one script a
# buffer (`guess_segment_properties`, `[[source: harfbuzz-hb-buffer]]`) -- so
# the runs are this file's and the test checks baud's against them. STIX Two Math selects
# different lookups under `latn`, `grek` and `cyrl`, so every cut here is a
# cut on it. No run is right to left: HarfBuzz mirrors a bracket there,
# which is bidi (UAX #9 L4), as `script.txt` says. Columns: path, face
# index, fnv64, the runs' codepoints, the runs' scripts, hb-shape per run.
BR_OUT=baud-font/tests/shape/brackets.txt
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1)"
  echo "# features: $SC_FEATURES"
  echo "# columns: path, face index, fnv64 of the file, codepoints by run, run scripts, hb-shape per run"
  face=baud-app/fonts/stix-two-math/STIXTwoMath-Regular.otf
  fnv=$(helper fnv64 "$face")
  while IFS=$'\t' read -r runs scripts; do
    [ -z "$runs" ] && continue
    shaped=""
    i=1
    IFS='|' read -ra parts <<<"$runs"
    for part in "${parts[@]}"; do
      script=$(echo "$scripts" | cut -d' ' -f"$i")
      cps=$(echo "$part" | xargs | tr ' ' ',')
      out=$(hb-shape --no-glyph-names --features="$SC_FEATURES" --script="$script" --direction=ltr -u "$cps" "$face")
      shaped="$shaped${shaped:+ }$out"
      i=$((i + 1))
    done
    printf '%s\t0\t%s\t%s\t%s\t%s\n' "$face" "$fnv" "$runs" "$scripts" "$shaped"
  done <<'RUNS'
0061 0028|03B1|0029 0062	Latn Grek Latn
0061 0028|03B1|0029	Latn Grek Latn
03B1 0028|0061|0029 03B2	Grek Latn Grek
0061 005B|03B1 0028 03B2 0029 03B3|005D 0062	Latn Grek Latn
0061 0028|03B1 005B 03B2|0029 0062	Latn Grek Latn
0434 007B|0061|007D 0436	Cyrl Latn Cyrl
0028 03B1 0020|0061|0029|0062	Grek Latn Grek Latn
0067 0061 006D 006D 0061 0020 0028|03B3|0029 0020 0069 0073	Latn Grek Latn
RUNS
} >"$BR_OUT"
wc -l "$BR_OUT"

# Arabic joining: each word in logical order, shaped by hb-shape with its
# script and direction guessed (Arabic, right to left), so HarfBuzz's Arabic
# shaper picks each letter's form from its neighbours and forms the lam-alef
# ligature (`[[source: unicode-ch9]]` 9.2.2 and 9.2.3). baud joins the same
# word by its own rules and shapes each letter alone in the form they give
# it, the lam-alef pair together, and the test compares the two. The
# features are the grid set with the joining forms and `rlig` on, and
# everything else HarfBuzz's Arabic shaper adds off. The faces: Courier
# New, the face macOS gives Arabic beside JetBrains Mono, Arial and Tahoma,
# whose GSUB carries `isol` too; Courier New again with its GSUB renamed
# GSUZ, so HarfBuzz falls back to the presentation forms its `cmap` maps
# (`arabic_fallback_shape()`); Geeza Pro, which joins through `morx` and
# which baud draws through the presentation forms, compared by outline; and
# the built Regular, which has no Arabic, as the control. The features
# column carries `gsub=renamed` for the renamed face. Columns as
# `script.txt`'s, with the features after the digest.
AR_OUT=baud-font/tests/shape/arabic.txt
AR_FEATURES="-liga,-clig,-kern,-locl,-rclt,-dlig,-hlig,-mset,-curs,-stch"
AR_DIR=$(mktemp -d)
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "# features: $AR_FEATURES"
  echo "# columns: path, face index, fnv64 of the file, features, codepoints, hb-shape (script and direction guessed)"
  courier="/System/Library/Fonts/Supplemental/Courier New.ttf"
  helper rename-table "$courier" "$AR_DIR/courier-gsuz.ttf" GSUB GSUZ
  for spec in "$courier|0|" "/System/Library/Fonts/Supplemental/Arial.ttf|0|" "/System/Library/Fonts/Supplemental/Tahoma.ttf|0|" "$AR_DIR/courier-gsuz.ttf|0|gsub=renamed" "/System/Library/Fonts/GeezaPro.ttc|0|" "baud-app/fonts/built/Regular.ttf|0|"; do
    IFS='|' read -r face index note <<<"$spec"
    named="$face"
    [ "$note" = "gsub=renamed" ] && named="$courier"
    [ -f "$face" ] || {
      echo "missing $face" >&2
      continue
    }
    fnv=$(helper fnv64 "$named")
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --face-index="$index" --features="$AR_FEATURES" -u "$cps" "$face")
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$named" "$index" "$fnv" "$AR_FEATURES${note:+ $note}" "$cps" "$shaped"
    done <<'SEQS'
0628
0627
0628 0628
0628 0628 0628
0628 064A 062A
0645 062D 0645 062F
0645 0631 062D 0628 0627
0643 062A 0627 0628
0633 0644 0627 0645
0644 0627
0628 0644 0627
0644 0623
0644 0625
0644 0622
0628 0644 0623 0628
0627 0644 0644 0647
0641 0627 0631 0633 06CC
067E 0646 062C 0631 0647
06AF 0644
0686 0627 06CC
0628 064E 0628 064E
0645 0651 0644
0628 0640 0628
0640 0628 0640
0628 200D
0628 200C 0628
0645 06CC 200C 062E 0648 0627 0647 0645
0628 0627 0628
0621 0628
0629 0628 0629
0644 064E 0627
0644 0644 0627
0628 0020 0628
SEQS
  done
  # Noto Sans Arabic 2.013, whose letters are wider than a cell and whose
  # dotted letters its `ccmp` takes apart into a skeleton and a dot placed
  # by `GPOS`: every word of the README's line, and the lone letters. Pinned
  # by tag and sha256 and fetched once into the cache devanagari.txt's Noto
  # face is read from, named `noto-cache/` in the path column.
  noto_url=https://raw.githubusercontent.com/notofonts/notofonts.github.io/noto-monthly-release-2026.10.01/fonts/NotoSansArabic/unhinted/ttf/NotoSansArabic-Regular.ttf
  noto_sha=bd86ca02f087d7f3c3788ba458fb6b73744c7639ed276b8d870dba6def6c40d0
  cache=${NOTO_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/baud-readme}
  noto=$cache/NotoSansArabic-Regular.ttf
  if [ ! -e "$noto" ]; then
    mkdir -p "$cache"
    curl -sSfL -o "$noto.part" "$noto_url" || exit 1
    mv "$noto.part" "$noto"
  fi
  [ "$(shasum -a 256 "$noto" | cut -d' ' -f1)" = "$noto_sha" ] || {
    echo "$noto does not hash to its pin" >&2
    exit 1
  }
  fnv=$(helper fnv64 "$noto")
  while IFS= read -r cps; do
    shaped=$(hb-shape --no-glyph-names --features="$AR_FEATURES" -u "$cps" "$noto")
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "noto-cache/NotoSansArabic-Regular.ttf" 0 "$fnv" "$AR_FEATURES" "$cps" "$shaped"
  done <<'SEQS'
0627 0644 0637 0631 0641 064A 0629
062A 0636 0639
0643 0644
062D 0631 0641
0641 064A
0627 0644 0634 0628 0643 0629 002E
0636
0635
0641
062D 0631
0631 0641
SEQS
} >"$AR_OUT"
rm -rf "$AR_DIR"
wc -l "$AR_OUT"

# Joining's contextual edges: Syriac alaph's three forms of its own (fin2,
# fin3, med2; `[[source: unicode-ch9]]` 9.3, S1-S3) on Noto Sans Syriac,
# whose `rlig` also picks an alaph's variant from the letter before it and
# whose `ccmp` takes U+0732 apart (a multiple substitution); and SF Arabic,
# the system's Arabic face, whose required contextual alternates (`rclt`)
# take a dotted letter apart into its skeleton and its dots and pick
# variants from the letters around it. Shaped with `rclt` on, which every
# shaper applies, and `calt` on, as the grid has it; the rest of what
# HarfBuzz's Arabic shaper adds off, as `arabic.txt`'s. baud joins each
# word by its own rules and shapes each cell with the cells around it in
# the word as context, keeping that cell's glyphs, as a renderer draws it;
# the built Regular, with neither script, is the control. Columns as
# `arabic.txt`'s.
CX_OUT=baud-font/tests/shape/contextual.txt
CX_FEATURES="-liga,-clig,-kern,-locl,-dlig,-hlig,-mset,-curs,-stch"
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "# features: $CX_FEATURES"
  echo "# columns: path, face index, fnv64 of the file, features, codepoints, hb-shape (script and direction guessed)"
  for spec in "/System/Library/Fonts/Supplemental/NotoSansSyriac-Regular.ttf|syriac" "/System/Library/Fonts/SFArabic.ttf|arabic" "baud-app/fonts/built/Regular.ttf|syriac" "baud-app/fonts/built/Regular.ttf|arabic"; do
    IFS='|' read -r face words <<<"$spec"
    [ -f "$face" ] || {
      echo "missing $face" >&2
      continue
    }
    fnv=$(helper fnv64 "$face")
    case "$words" in
    syriac)
      list='0710
0712 0710
0720 0710
071D 0710
0717 0710
0718 0710
0710 0710
0715 0710
072A 0710
0716 0710
072F 0710
0715 0730 0710
0712 0710 0712
0720 0710 0712
0712 0710 0710
0717 0710 0712
0715 0710 0712
0712 0640 0710
0712 0710 200D
0712 0710 200C 0712
0712 0020 0710
0712 200C 0710
0710 0712
072B 0720 0721 0710
0710 0720 0717 0710
0721 0720 071F 0710
0725 0715 072C 0710
0712 072A 0710
0710 0712 0710
071F 072C 0712 0710
0710 0715 0710
0712 0732
072A 0308
0712 0730 0710'
      ;;
    arabic)
      list='0628
0628 0628
0628 0628 0628
0628 0647
0646 0647
0628 062D
0628 0631
0628 0649
0641 0649
0646 0649
0641 064A
0628 064A 062A
0628 064A 0646
0646 062C 0645
064A 062C 0628
0642 0644 0628
0643 062A 0627 0628
0639 0631 0628 064A
0641 0627 0631 0633 06CC
067E 0646 062C 0631 0647
0628 064E 0628 064E
0628 0644 0627
0633 0644 0627 0645
0628 200C 0628
0628 0020 0628
0627 0644 0645 0633 062A 0634 0641 0649
0645 062D 0645 062F'
      ;;
    esac
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --features="$CX_FEATURES" -u "$cps" "$face")
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$face" 0 "$fnv" "$CX_FEATURES" "$cps" "$shaped"
    done <<<"$list"
  done
} >"$CX_OUT"
wc -l "$CX_OUT"

# Mongolian and Phags-pa joined, the two scripts of ArabicShaping.txt that
# run left to right on a horizontal line (Unicode 13.5 and 14.4), on the
# macOS faces that carry them, and on the built Regular as the control.
# Columns and features as `arabic.txt`'s; hb-shape guesses the script and
# lays each word out left to right. Words whose letters form no ligature
# and carry no variation selector: baud runs no `calt` or `rclt`, and
# `-rclt` keeps hb-shape from running it.
MO_OUT=baud-font/tests/shape/mongolian.txt
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "# features: $AR_FEATURES"
  echo "# columns: path, face index, fnv64 of the file, features, codepoints, hb-shape (script and direction guessed)"
  for face in /System/Library/Fonts/Supplemental/NotoSansMongolian-Regular.ttf /System/Library/Fonts/Supplemental/NotoSansPhagsPa-Regular.ttf baud-app/fonts/built/Regular.ttf; do
    [ -f "$face" ] || {
      echo "missing $face" >&2
      continue
    }
    fnv=$(helper fnv64 "$face")
    while IFS= read -r cps; do
      [ -z "$cps" ] && continue
      shaped=$(hb-shape --no-glyph-names --features="$AR_FEATURES" -u "$cps" "$face")
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$face" 0 "$fnv" "$AR_FEATURES" "$cps" "$shaped"
    done <<'SEQS'
1820
1820 1821
1820 1821 1822
1828 1820 182E
1822 182F 1822
1834 1823 1837
1828 1821 1836 1821 1837
1820 1821 0020 1820 1821
A840
A840 A841
A840 A841 A842
A84F A85C A848
A856 A85D A84F
A872 A840
SEQS
  done
} >"$MO_OUT"
wc -l "$MO_OUT"

# Meteg on the built faces: every letter with every vowel meteg may follow
# (sheva, the hatafs, hiriq, tsere, segol, patah, qamats, qubuts, qamats
# qatan), spelled as the Unicode Standard (9.1) gives each side -- <vowel,
# meteg> left of the vowel, <meteg, CGJ, vowel> right of it -- on all four
# faces; and on the Regular and the Bold, the same on bet, qof, final nun
# and shin with its dot, with a dagesh after the vowel and before it, which
# the lookup's mark filtering set steps over. Where the vowel's own place
# leaves meteg out of the cell, the faces' chained context places the
# vowel again and meteg beside it (build/meteg.py), so these lines hold
# that lookup, and its order among the mark lookups, to HarfBuzz. Columns
# and features as `script.txt`.
MT_OUT=baud-font/tests/shape/meteg.txt
METEG_SEQS=$(helper meteg)
{
  echo "# generated by scripts/gen-shape-fixtures.sh with $(hb-shape --version | head -1) on $(sw_vers -productVersion 2>/dev/null || uname -r)"
  echo "# features: $SC_FEATURES"
  echo "# columns: path, face index, fnv64 of the file, codepoints, hb-shape (script and direction guessed)"
  for face in baud-app/fonts/built/Regular.ttf baud-app/fonts/built/Bold.ttf baud-app/fonts/built/Italic.ttf baud-app/fonts/built/BoldItalic.ttf; do
    fnv=$(helper fnv64 "$face")
    while IFS=$'\t' read -r which cps; do
      [ -z "$cps" ] && continue
      case $which:$face in
      upright:*Italic.ttf) continue ;;
      esac
      shaped=$(hb-shape --no-glyph-names --features="$SC_FEATURES" -u "$cps" "$face")
      printf '%s\t0\t%s\t%s\t%s\n' "$face" "$fnv" "$cps" "$shaped"
    done <<<"$METEG_SEQS"
  done
} >"$MT_OUT"
wc -l "$MT_OUT"

devanagari_fixtures
devanagari_wdth_fixtures
devanagari_characters_fixtures
