#!/usr/bin/env bash
# kitty's own colour parser, run on the values in a file, one a line:
# prints `value<TAB>rrggbb`, or `value<TAB>-` where kitty reads no colour.
# An `oklch(` or `lab(` value kitty reads gets a third column, its case
# (note AR, O14): `edge` (L at or past 0 or 1, or not finite), `grey`
# (chroma under 1e-5), `in` (linear sRGB inside [0, 1] before any clip)
# or `out`; an `out` row a fourth, what kitty's Go code -- its themes
# kitten's preview -- draws for the numbers kitty's C read.
#
# A value may hold a tab, so a row is read from its right. The values
# themselves are the rows less their columns:
#
#   awk -F'\t' -v OFS='\t' '{ n = NF - ($(NF-1) == "out" ? 3 : $NF ~ /^(edge|grey|in)$/ ? 2 : 1)
#     s = $1; for (i = 2; i <= n; i++) s = s OFS $i; print s }' \
#     baud-domain/tests/kitty-colours.txt > VALUES
#   bash scripts/kitty-colour-oracle.sh VALUES > baud-domain/tests/kitty-colours.txt
#
# kitty is GPL-3.0 and is not in this tree: the three files are fetched at
# the commit the manifest pins (kitty-colors-1e1486e,
# kitty-color-names-h-1e1486e, kitty-colorspaces-go-1e1486e), checked against its SHA-256, and kitty's
# own functions -- hexchar_to_int to trim_view, colors.c 1129-1497 -- are
# compiled unchanged. The driver below is parse_color (colors.c 1500-1540)
# with the Python object calls taken out: the value is lower-cased byte by
# byte, as str.lower() does for ASCII, and nothing else differs. `note`
# reads an `oklch(`/`lab(` value as parse_oklch/parse_lab do (1433-1482) and
# names its case from the same numbers. tools/utils/style/colorspaces.go
# (kitty-colorspaces-go-1e1486e) is fetched the same way and run by Go.
# Go fuses a multiply-add on arm64 and not on x86-64; on 2026-10-17 the
# file was the same either way (GOARCH=amd64 under Rosetta, and arm64),
# and the same with FP_CONTRACT=on.
set -euo pipefail

values=$1
rev=1e1486ebbae6e66a35205dbde23f1e15a1fbc72d
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fetch() {
  curl -sfL -o "$work/$2" "https://raw.githubusercontent.com/kovidgoyal/kitty/$rev/$1"
  echo "$3  $work/$2" | shasum -a 256 -c --quiet
}
fetch kitty/colors.c colors.c a13cac7e7162951ed7332c9c9ab972bf2ff55feba238c1cf59f8ef91f5928f0f
fetch kitty/color-names.h color-names.h 3be7edfc24aa866c12e80ccc43db0954311587d308aa7f7ce6d01db7c3c2968c
mkdir -p "$work/go/style"
fetch tools/utils/style/colorspaces.go go/style/colorspaces.go 316a4dc08e0c5ef818a1f063357c9c36302074a64b4e35ddba923f2aa19c980f

sed -n '1129,1497p' "$work/colors.c" >"$work/kitty-parse.inc"

cat >"$work/oracle.c" <<'EOF'
#include <ctype.h>
#include <errno.h>
#include <math.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <xlocale.h>

typedef long Py_ssize_t;
typedef struct { unsigned char r, g, b; } Rgb;
typedef Rgb PyObject;
static Rgb out;
static PyObject *alloc_color(unsigned char r, unsigned char g, unsigned char b, unsigned a) {
    (void)a;
    out.r = r; out.g = g; out.b = b;
    return &out;
}
#define Py_RETURN_NONE return NULL
#define START_ALLOW_CASE_RANGE
#define END_ALLOW_CASE_RANGE
#define MAX(x, y) __extension__({ const __typeof__(x) __a__ = (x); const __typeof__(y) __b__ = (y); __a__ > __b__ ? __a__ : __b__; })
#define MIN(x, y) __extension__({ const __typeof__(x) __a__ = (x); const __typeof__(y) __b__ = (y); __a__ < __b__ ? __a__ : __b__; })
static locale_t get_c_locale(void) {
    static locale_t c;
    if (!c) c = newlocale(LC_ALL_MASK, "C", NULL);
    return c;
}
#include "color-names.h"
#include "kitty-parse.inc"

/* The case of an oklch( or lab( value, and for `out` the numbers kitty's C
   read: L, C and h (h as fmod leaves it), or L, a and b. */
static char note_case[8];
static char note_kind;
static double note_v[3];

static bool in01(const double v[3]) {
    for (int i = 0; i < 3; i++)
        if (!(0. <= v[i] && v[i] <= 1.)) return false;
    return true;
}

static void note(const char *spec, size_t len, char kind) {
    const char *head = kind == 'o' ? "klch(" : "ab(";
    size_t hl = strlen(head), min = kind == 'o' ? 10 : 8;
    note_case[0] = 0;
    if (len < min || spec[len - 1] != ')' || strncmp(spec, head, hl)) return;
    char buf[256];
    size_t n = len - 1 - hl;
    if (n >= sizeof(buf)) return;
    memcpy(buf, spec + hl, n);
    buf[n] = 0;
    double v[3], lin[3];
    char *tok;
    for (int i = 0; i < 3; i++)
        if (!(tok = strtok(i ? NULL : buf, " ,")) || !parse_double_intensity(tok, &v[i], kind == 'o' ? 100 : 1)) return;
    double l, c, h;
    note_kind = kind;
    if (kind == 'o') {
        l = clamp(v[0]);
        c = MAX(0.f, v[1]);
        h = fmod(v[2], 360);
        note_v[0] = l; note_v[1] = c; note_v[2] = h;
        if (isfinite(l) && isfinite(c) && isfinite(h)) {
            /* oklch_to_srgb before its clamp */
            const double hr = degrees_to_radians(h), a = c * cos(hr), lb = c * sin(hr);
            const double l_ = l + 0.3963377774 * a + 0.2158037573 * lb;
            const double m_ = l - 0.1055613458 * a - 0.0638541728 * lb;
            const double s_ = l - 0.0894841775 * a - 1.2914855480 * lb;
            const double L3 = l_ * l_ * l_, M3 = m_ * m_ * m_, S3 = s_ * s_ * s_;
            lin[0] = +4.0767416621 * L3 - 3.3077115913 * M3 + 0.2309699292 * S3;
            lin[1] = -1.2684380046 * L3 + 2.6097574011 * M3 - 0.3413193965 * S3;
            lin[2] = -0.0041960863 * L3 - 0.7034186147 * M3 + 1.7076147010 * S3;
        }
    } else {
        const double L = MAX(0., MIN(v[0], 100.));
        note_v[0] = L; note_v[1] = v[1]; note_v[2] = v[2];
        /* lab_to_oklch before its clip */
        const double y = (L + 16.) / 116., x = v[1] / 500. + y, z = y - v[2] / 200.;
        const double X = 0.95047 * f_inv(x), Y = f_inv(y), Z = 1.08883 * f_inv(z);
        lin[0] = +3.2404542 * X - 1.5371385 * Y - 0.4985314 * Z;
        lin[1] = -0.9692660 * X + 1.8760108 * Y + 0.0415560 * Z;
        lin[2] = +0.0556434 * X - 0.2040259 * Y + 1.0572252 * Z;
        lab_to_oklch(L, v[1], v[2], &l, &c, &h);
    }
    if (!isfinite(l) || !isfinite(c) || !isfinite(h) || l <= 0 || l >= 1) strcpy(note_case, "edge");
    else if (c < 0.00001) strcpy(note_case, "grey");
    else strcpy(note_case, in01(lin) ? "in" : "out");
}

static PyObject *parse_color(char *spec) {
    for (char *p = spec; *p; p++) *p = (char)tolower((unsigned char)*p);
    Py_ssize_t len = (Py_ssize_t)strlen(spec);
    spec = (char *)trim_view(spec, &len);
    if (len < 2) Py_RETURN_NONE;
    switch (spec[0]) {
        case '#': {
            const char *s = strchr(spec, ' ');
            if (s) len = s - spec;
        } break;
        default: {
            const char *s = strchr(spec, '#');
            if (s) {
                len = s - spec;
                spec = (char *)trim_view(spec, &len);
            }
        } break;
    }
    const struct Keyword *k = in_color_name_set(spec, len);
    if (k) return alloc_color((k->value >> 16) & 0xff, (k->value >> 8) & 0xff, k->value & 0xff, 0);
    if (len < 4) Py_RETURN_NONE;
    switch (spec[0]) {
        case '#': return parse_sharp(spec + 1, len - 1);
        case 'r':
            if (spec[1] != 'g' || spec[2] != 'b' || len < 6) Py_RETURN_NONE;
            switch (spec[3]) {
                case ':': return parse_rgb(spec + 4, len - 4);
                case 'i':
                    if (spec[4] == 'i' && spec[5] == ':') return parse_rgbi(spec + 5, len - 5);
            }
            Py_RETURN_NONE;
        case 'o': note(spec + 1, len - 1, 'o'); return parse_oklch(spec + 1, len - 1);
        case 'l': note(spec + 1, len - 1, 'l'); return parse_lab(spec + 1, len - 1);
    }
    Py_RETURN_NONE;
}

int main(int argc, char **argv) {
    FILE *f = fopen(argv[1], "r");
    char line[4096], copy[4096];
    while (fgets(line, sizeof line, f)) {
        line[strcspn(line, "\n")] = 0;
        strcpy(copy, line);
        note_case[0] = 0;
        PyObject *c = parse_color(copy);
        if (!c) printf("%s\t-\n", line);
        else if (!note_case[0]) printf("%s\t%02x%02x%02x\n", line, c->r, c->g, c->b);
        else if (strcmp(note_case, "out")) printf("%s\t%02x%02x%02x\t%s\n", line, c->r, c->g, c->b, note_case);
        else printf("%s\t%02x%02x%02x\tout\t%c\t%a\t%a\t%a\n", line, c->r, c->g, c->b, note_kind, note_v[0], note_v[1], note_v[2]);
    }
    return argc - 2;
}
EOF
# kitty's own flags (setup.py 628, 650: -O3, no fast-math), with no
# fused multiply-add, as ISO C evaluates each operation (GCC's -std=c11
# default; clang fuses unless told not to). FP_CONTRACT=on in the
# environment runs it as clang builds kitty on macOS.
cc -O3 -fwrapv -ffp-contract="${FP_CONTRACT:-off}" -w -I"$work" -o "$work/oracle" "$work/oracle.c" -lm
"$work/oracle" "$values" >"$work/c.txt"

# kitty's Go: oklchToSrgbGamutMap after parseOklch's own hue wrap, and
# labToOklch then the same, on the numbers kitty's C read.
cat >"$work/go/go.mod" <<'GOMOD'
module oracle

go 1.22
GOMOD
cat >"$work/go/style/oracle_test.go" <<'GOEOF'
package style

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"testing"
)

type RGBA struct{ Red, Green, Blue, Inverse_alpha uint8 }

func TestOracle(t *testing.T) {
	in, err := os.ReadFile(os.Getenv("IN"))
	if err != nil {
		t.Fatal(err)
	}
	out, err := os.Create(os.Getenv("OUT"))
	if err != nil {
		t.Fatal(err)
	}
	defer out.Close()
	for _, line := range strings.Split(strings.TrimRight(string(in), "\n"), "\n") {
		f := strings.Split(line, "\t")
		n := len(f)
		if n < 7 || f[n-5] != "out" {
			continue
		}
		var v [3]float64
		for i := range v {
			if v[i], err = strconv.ParseFloat(f[n-3+i], 64); err != nil {
				t.Fatal(line, err)
			}
		}
		var r, g, b float64
		if f[n-4] == "o" {
			h := v[2]
			if h < 0 {
				h += 360
			}
			r, g, b = oklchToSrgbGamutMap(v[0], v[1], h)
		} else {
			l, c, h := labToOklch(v[0], v[1], v[2])
			r, g, b = oklchToSrgbGamutMap(l, c, h)
		}
		fmt.Fprintf(out, "%02x%02x%02x\n", as8bit(r), as8bit(g), as8bit(b))
	}
}
GOEOF
(cd "$work/go" && GOTOOLCHAIN=local GOFLAGS=-mod=mod GOCACHE="$work/gocache" GOPATH="$work/gopath" \
  IN="$work/c.txt" OUT="$work/go.txt" "${GO:-go}" test ./style -run TestOracle -count=1 >/dev/null)

awk -F'\t' -v OFS='\t' 'NR == FNR { go[++n] = $0; next }
  NF >= 7 && $(NF-4) == "out" { s = $1; for (i = 2; i <= NF - 4; i++) s = s OFS $i; print s, go[++k]; next }
  { print }' "$work/go.txt" "$work/c.txt"
