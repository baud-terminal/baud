#!/usr/bin/env bash
# kitty's own colour parser, run on the values in a file, one a line:
# prints `value<TAB>rrggbb`, or `value<TAB>-` where kitty reads no colour.
#
#   bash scripts/kitty-colour-oracle.sh VALUES > baud-domain/tests/kitty-colours.txt
#
# kitty is GPL-3.0 and is not in this tree: the two files are fetched at
# the commit the manifest pins (kitty-colors-1e1486e,
# kitty-color-names-h-1e1486e), checked against its SHA-256, and kitty's
# own functions -- hexchar_to_int to trim_view, colors.c 1129-1497 -- are
# compiled unchanged. The driver below is parse_color (colors.c 1500-1540)
# with the Python object calls taken out: the value is lower-cased byte by
# byte, as str.lower() does for ASCII, and nothing else differs.
set -euo pipefail

values=$1
rev=1e1486ebbae6e66a35205dbde23f1e15a1fbc72d
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fetch() {
  curl -sfL -o "$work/$2" "https://raw.githubusercontent.com/kovidgoyal/kitty/$rev/kitty/$1"
  echo "$3  $work/$2" | shasum -a 256 -c --quiet
}
fetch colors.c colors.c a13cac7e7162951ed7332c9c9ab972bf2ff55feba238c1cf59f8ef91f5928f0f
fetch color-names.h color-names.h 3be7edfc24aa866c12e80ccc43db0954311587d308aa7f7ce6d01db7c3c2968c

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
        case 'o': return parse_oklch(spec + 1, len - 1);
        case 'l': return parse_lab(spec + 1, len - 1);
    }
    Py_RETURN_NONE;
}

int main(int argc, char **argv) {
    FILE *f = fopen(argv[1], "r");
    char line[4096], copy[4096];
    while (fgets(line, sizeof line, f)) {
        line[strcspn(line, "\n")] = 0;
        strcpy(copy, line);
        PyObject *c = parse_color(copy);
        if (c) printf("%s\t%02x%02x%02x\n", line, c->r, c->g, c->b);
        else printf("%s\t-\n", line);
    }
    return argc - 2;
}
EOF
# kitty's own flags (setup.py 628, 650: -O3, no fast-math), with no
# fused multiply-add, as ISO C evaluates each operation (GCC's -std=c11
# default; clang fuses unless told not to). FP_CONTRACT=on in the
# environment runs it as clang builds kitty on macOS.
cc -O3 -fwrapv -ffp-contract="${FP_CONTRACT:-off}" -w -I"$work" -o "$work/oracle" "$work/oracle.c" -lm
"$work/oracle" "$values"
