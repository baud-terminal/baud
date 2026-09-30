"""Variable fonts built to reach what HVAR and the phantom-point advance do
in rules the real fonts never test: baud-font's step 3 of note Z.

Writes baud-font/tests/var/hvar/*.ttf. Each is 16 glyphs (.notdef, g1..g15)
of one square, advance 600, with gvar deltas on the phantom points that
disagree with HVAR, so a font whose HVAR is read gives HVAR's answer and one
whose HVAR is not gives gvar's. Axes: wght 100-400-900, wdth 50-100-200.

cases.ttf -- the HVAR table, written byte for byte here rather than by
fontTools, so every field is what the test needs:
  regions  6: r0 wght peak 1 (wdth peak 0: factor 1), r1 wght 0.2/0.5/0.8,
           r2 wdth -1, r3 wght start > peak (factor 1), r4 wght -0.5/0.5/1
           (straddles 0: factor 1), r5 wght and wdth peak 1
  data 0   word rows: two 16-bit columns then three 8-bit, regions
           0, 1, 6 (past the list: 0), 3, 4
  data 1   LONG_WORDS: one 32-bit column then two 16-bit, regions 0, 2, 5
  data 2   no regions: every item 0
  data 3   one item
  advMap   format 0, two-byte entries, four inner bits, twelve entries; g12
           to g15 read the last. Items reach: a 0.5 delta (rounds to 1), a
           -0.5 delta (rounds to -1, where rounding the sum would give the
           hmtx advance), a sum floored at 0, an outer past the store, an
           inner past a set.
map-*.ttf -- the same store under other maps: none (the glyph id), format 1
  with three-byte entries and twelve inner bits, one-byte entries with one
  inner bit, four-byte entries with sixteen, and format 7 (the identity).
bad-*.ttf -- the same table with one thing broken: the map's offset past
  the table, the store's offset past it, data set 1's offset past it,
  majorVersion 2, regionCount's top bit set (HarfBuzz's HBUINT15 does not
  mask it -- a TODO in hb-open-type.hh -- so the count is 16 bits and the
  region list runs past the table), the side-bearing map's offset past the
  table, and a data set in range with more wide columns than regions.
  HarfBuzz refuses each whole table, so the advance is gvar's.
null-*.ttf -- an offset of 0, which is not malformed: a null store, a null
  region list, and a null data set 1. The table is read and those deltas
  are 0, so the advance is hmtx's -- where a refused table would be gvar's.
phantom.ttf -- no HVAR, so advances come from gvar's phantom points:
  lsb != xMin, a -0.5 extent, a USE_MY_METRICS composite whose own phantom
  deltas it replaces, one with two such components (the last wins), one
  nested two deep, a composite whose USE_MY_METRICS child is truncated
  (HarfBuzz's upem / 2) and one whose truncated child is not USE_MY_METRICS
  (not walked, so an ordinary advance), a composite with no USE_MY_METRICS
  component whose own phantom deltas stand, and USE_MY_METRICS fan-outs at
  2,049 and 2,050 visits (HarfBuzz's graph-edge bound).

    nix shell --impure --expr 'with import (builtins.getFlake "nixpkgs") {}; \
      python3.withPackages (p: [ p.fonttools ])' -c python3 scripts/hvar-cases-font.py
"""
import os
import struct
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables.DefaultTable import DefaultTable
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.ttLib.tables._g_l_y_f import Glyph, GlyphComponent

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "baud-font/tests/var/hvar")
os.makedirs(OUT, exist_ok=True)
W, D = "wght", "wdth"


def f2(v):
    return struct.pack(">h", round(v * 16384))


def regions(top_bit=False):
    axes = [
        [(0, 1, 1), (0, 0, 0)],
        [(0.2, 0.5, 0.8), (0, 0, 0)],
        [(0, 0, 0), (-1, -1, 0)],
        [(0.8, 0.5, 1), (0, 0, 0)],
        [(-0.5, 0.5, 1), (0, 0, 0)],
        [(0, 1, 1), (0, 1, 1)],
    ]
    out = struct.pack(">HH", 2, (0x8000 if top_bit else 0) | len(axes))
    for region in axes:
        for s, p, e in region:
            out += f2(s) + f2(p) + f2(e)
    return out


def var_data(items, word_count, long_words, region_indices):
    """Rows of deltas: the first word_count columns wide, the rest narrow."""
    packed = word_count | (0x8000 if long_words else 0)
    out = struct.pack(">HHH", len(items), packed, len(region_indices))
    out += b"".join(struct.pack(">H", r) for r in region_indices)
    wide, narrow = (">i", ">h") if long_words else (">h", ">b")
    for row in items:
        for k, v in enumerate(row):
            out += struct.pack(wide if k < word_count else narrow, v)
    return out


DATA = [
    var_data(
        [
            [100, 0, 0, 0, 0],
            [1, 0, 0, 0, 0],
            [-1, 0, 0, 0, 0],
            [0, 40, 50, -7, 9],
            [-2000, 0, 0, 0, 0],
            [3, -3, 0, 0, 0],
        ],
        2, False, [0, 1, 6, 3, 4],
    ),
    var_data([[30000, -300, 200], [-70000, 5, 5]], 1, True, [0, 2, 5]),
    var_data([[], [], []], 0, False, []),
    var_data([[11, 0]], 1, False, [0, 5]),
]

# (outer, inner) per glyph for the twelve map entries; g12..g15 read the last.
ENTRIES = [
    (0, 0), (0, 1), (0, 2), (0, 3), (0, 4), (1, 0), (1, 1),
    (2, 0), (3, 5), (7, 0), (0, 9), (0, 5),
]


def store(bad_data=None, top_bit=False, bad_words=False, null_regions=False, null_data=None):
    region_list = regions(top_bit)
    data = list(DATA)
    if bad_words:
        # More wide columns than regions: in range, and malformed.
        data[3] = struct.pack(">HHHH", 1, 2, 1, 0) + struct.pack(">hh", 11, 0)
    count = len(data)
    head = 2 + 4 + 2 + 4 * count
    offsets, blobs, at = [], b"", head + len(region_list)
    for k, d in enumerate(data):
        offsets.append(0xFFFF0 if k == bad_data else 0 if k == null_data else at)
        blobs += d
        at += len(d)
    out = struct.pack(">HIH", 1, 0 if null_regions else head, count)
    out += b"".join(struct.pack(">I", o) for o in offsets)
    return out + region_list + blobs


def adv_map(fmt, width, inner_bits, entries=ENTRIES):
    entry_format = ((width - 1) << 4) | (inner_bits - 1)
    if fmt == 1:
        out = struct.pack(">BBI", 1, entry_format, len(entries))
    elif fmt == 0:
        out = struct.pack(">BBH", 0, entry_format, len(entries))
    else:
        return struct.pack(">BB", fmt, entry_format)
    for outer, inner in entries:
        u = (outer << inner_bits) | inner
        out += u.to_bytes(width, "big")
    return out


def hvar(map_bytes, major=1, bad_store=False, bad_map=False, bad_data=None, top_bit=False,
         bad_lsb=False, bad_words=False, null_store=False, null_regions=False, null_data=None):
    s = store(bad_data, top_bit, bad_words, null_regions, null_data)
    store_at = 20
    map_at = store_at + len(s) if map_bytes is not None else 0
    body = s + (map_bytes or b"")
    if bad_store:
        store_at = 0xFFFF0
    if null_store:
        store_at = 0
    if bad_map:
        map_at = 0xFFFF0
    # A side-bearing map nothing reads still takes the table with it.
    lsb_at = 0xFFFF0 if bad_lsb else 0
    return struct.pack(">HHIIII", major, 0, store_at, map_at, lsb_at, 0) + body


def square(x0=50, x1=550):
    pen = TTGlyphPen(None)
    pen.moveTo((x0, 0))
    pen.lineTo((x1, 0))
    pen.lineTo((x1, 700))
    pen.lineTo((x0, 700))
    pen.closePath()
    return pen.glyph()


def base(names, glyphs, metrics, variations, components=None):
    fb = FontBuilder(1000, isTTF=True)
    fb.setupGlyphOrder(names)
    fb.setupCharacterMap({0x41 + i: n for i, n in enumerate(names[1:])})
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=900, descent=-100)
    fb.setupNameTable({"familyName": "baud hvar cases", "styleName": "Regular"})
    fb.setupOS2()
    fb.setupPost()
    fb.setupHead(unitsPerEm=1000, created=0, modified=0)
    fb.setupFvar(
        axes=[("wght", 100, 400, 900, "Weight"), ("wdth", 50, 100, 200, "Width")],
        instances=[],
    )
    fb.setupGvar(variations)
    return fb


def phantom_tuple(axes, left, right):
    # A square is four points, then the phantoms: left, right, top, bottom.
    return TupleVariation(axes, [None] * 4 + [(left, 0), (right, 0), None, None])


def hvar_font(name, table):
    names = [".notdef"] + [f"g{i}" for i in range(1, 16)]
    glyphs = {n: square() for n in names}
    metrics = {n: (600, 50) for n in names}
    # gvar moves every right phantom by 77 at wght 900: an advance HVAR
    # does not override would show it.
    variations = {n: [phantom_tuple({W: (0, 1, 1)}, 0, 77)] for n in names}
    fb = base(names, glyphs, metrics, variations)
    t = DefaultTable("HVAR")
    t.data = table
    fb.font["HVAR"] = t
    fb.font.save(os.path.join(OUT, name))
    print(name)


hvar_font("cases.ttf", hvar(adv_map(0, 2, 4)))
hvar_font("map-none.ttf", hvar(None))
hvar_font("map-f1-w3-i12.ttf", hvar(adv_map(1, 3, 12)))
hvar_font("map-w1-i1.ttf", hvar(adv_map(0, 1, 1, [(0, 1), (1, 1), (0, 0), (1, 0), (2, 1)])))
hvar_font("map-w4-i16.ttf", hvar(adv_map(0, 4, 16)))
hvar_font("map-f7.ttf", hvar(adv_map(7, 2, 4)))
hvar_font("bad-map.ttf", hvar(adv_map(0, 2, 4), bad_map=True))
hvar_font("bad-store.ttf", hvar(adv_map(0, 2, 4), bad_store=True))
hvar_font("bad-data.ttf", hvar(adv_map(0, 2, 4), bad_data=1))
hvar_font("bad-version.ttf", hvar(adv_map(0, 2, 4), major=2))
hvar_font("bad-regions.ttf", hvar(adv_map(0, 2, 4), top_bit=True))
hvar_font("bad-lsb-map.ttf", hvar(adv_map(0, 2, 4), bad_lsb=True))
hvar_font("bad-words.ttf", hvar(adv_map(0, 2, 4), bad_words=True))
hvar_font("null-store.ttf", hvar(adv_map(0, 2, 4), null_store=True))
hvar_font("null-regions.ttf", hvar(adv_map(0, 2, 4), null_regions=True))
hvar_font("null-data.ttf", hvar(adv_map(0, 2, 4), null_data=1))


def component(name, flags=0, x=0):
    c = GlyphComponent()
    c.glyphName = name
    c.x, c.y = x, 0
    c.flags = flags
    return c


def composite(*components):
    g = TTGlyphPen(None).glyph()
    g.numberOfContours = -1
    g.components = list(components)
    return g


USE_MY_METRICS = 0x0200
names = [".notdef", "a", "b", "half", "mine", "two", "deep", "broken", "badchild", "skip",
         "own", "fan64", "edge2049", "edge2050"]
glyphs = {
    ".notdef": square(),
    "a": square(),
    "b": square(80, 500),
    "half": square(),
    "mine": composite(component("a", USE_MY_METRICS), component("b", 0, 300)),
    "two": composite(component("a", USE_MY_METRICS), component("b", USE_MY_METRICS)),
    "deep": composite(component("mine", USE_MY_METRICS)),
    "broken": square(),
    "badchild": composite(component("broken", USE_MY_METRICS)),
    "skip": composite(component("a", USE_MY_METRICS), component("broken")),
    # No USE_MY_METRICS component: the composite's own phantoms stand.
    "own": composite(component("b")),
    # HarfBuzz's visit bound on the USE_MY_METRICS walk: 1 + 31 * 65 + 33
    # visits is 2,049, which it walks; one more is refused (upem / 2).
    "fan64": composite(*[component("a", USE_MY_METRICS) for _ in range(64)]),
    "edge2049": composite(*[component("fan64", USE_MY_METRICS) for _ in range(31)],
                          *[component("a", USE_MY_METRICS) for _ in range(33)]),
    "edge2050": composite(*[component("fan64", USE_MY_METRICS) for _ in range(31)],
                          *[component("a", USE_MY_METRICS) for _ in range(34)]),
}
# lsb differs from xMin on every glyph: HarfBuzz starts the phantoms at
# xMin - lsb, so the extent is the same and only the start moves.
metrics = {n: (600 + 10 * i, 20 + i) for i, n in enumerate(names)}
variations = {
    "a": [phantom_tuple({W: (0, 1, 1)}, 10, 55), phantom_tuple({D: (0, 1, 1)}, -7, 13)],
    "b": [phantom_tuple({W: (0, 1, 1)}, -20, 90)],
    # At wght 650 (0.5): 0 - 1 * 0.5 is a -0.5 extent, which rounds away.
    "half": [phantom_tuple({W: (0, 1, 1)}, 0, -1)],
    # The composites' own tuples move their phantoms; USE_MY_METRICS then
    # replaces them. Two component offsets, then the phantoms.
    "mine": [TupleVariation({W: (0, 1, 1)}, [(5, 0), (5, 0), (300, 0), (900, 0), None, None])],
    "skip": [TupleVariation({W: (0, 1, 1)}, [(0, 0), (0, 0), (0, 0), (41, 0), None, None])],
    "own": [TupleVariation({W: (0, 1, 1)}, [(0, 0), (12, 0), (60, 0), None, None])],
}
fb = base(names, glyphs, metrics, variations)
# "broken" claims three contours and carries two bytes of them.
fb.font["glyf"].glyphs["broken"] = Glyph(struct.pack(">hhhhh", 3, 50, 0, 550, 700) + b"\x00\x02")
fb.font.recalcBBoxes = False
fb.font.save(os.path.join(OUT, "phantom.ttf"))
print("phantom.ttf")
