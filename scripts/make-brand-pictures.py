#!/usr/bin/env python3
"""Draw every baud logo file from one mark, baud's own drawing of "❯" with
the cursor in the next cell.

    python3 scripts/make-brand-pictures.py          write every file below
    python3 scripts/make-brand-pictures.py --check  draw them again in memory and
                                                    fail unless each equals the
                                                    committed file byte for byte,
                                                    they are every file in
                                                    assets/logo, no SVG names a
                                                    font, and both GitHub
                                                    pictures meet GitHub's
                                                    limits; writes no tracked
                                                    file (the plasma example
                                                    still builds under cargo's
                                                    target directory)

**The mark is how baud draws it**: "❯" with its block cursor in the very
next cell, as a baud window shows them. So the ❯ is JetBrains Mono Bold's
own outline placed in a cell as
baud's renderer places a glyph -- its origin at the cell's left, its
baseline BASELINE below the cell's top, at FONT_PX -- and the cursor is the
next cell, whole, with no gap. The cell is CELL_W by CELL_H: baud's metrics
at 200 px, width round(0.6 px), height round(1.32 px) and baseline
round(1.02 px). The plasma example reports them from the renderer, and this
refuses to run if they differ; baud-app/tests/brand_pictures.rs holds every
file's ❯ and cursor to the renderer's own drawing of them. Every file
scales that one drawing to its cell; no file draws the ❯ any other way.

What it writes, each from the code below and nothing else:

  assets/logo/baud-lockup-{dark,light,mono-black,mono-white}.svg  "baud" and
      the mark on one line, 300 by 130: the word where it always was, the ❯'s
      cell from x 192 on the old cursor's rows, the whole ink then centred
      across the box; the light forms' amber, here and in the marks and the
      title bar's lockup below, is baud's cursor on the light theme's page
      (BaudCursor::light, which the plasma example reports), so it is the
      amber the light theme's cursor and launch logo draw
  assets/logo/baud-lockup-{dark,light}-blinking.svg  their fixed twins' bytes,
      under the names this sheet and THIRD_PARTY.md give them; the README's
      header is scripts/make-readme-logo.py's
  assets/logo/baud-mark-{dark,light,amber}.svg (and -dark-, -light-blinking)
      the mark alone at 512, transparent
  assets/logo/baud-icon-512-{dark,light,amber}.svg (and -dark-, -light-blinking),
      baud-icon-32-{dark,light}.svg  the icons
  assets/logo/baud-icon-dock.svg, baud-icon-dock-<N>.png  the Dock icon and its
      drawings at the iconset's sizes, which scripts/bundle-macos.sh packs into
      baud.icns; baud-app/src/app.rs embeds the SVG
  assets/logo/baud-mark-amber-tight.svg  the mark alone, its box 300 by 204, as
      the title bar showed it before the lockups
  assets/logo/baud-lockup-{dark,light}-tight.svg  the title bar's lockups
      (baud-app/src/mark.rs embeds them): baud-lockup-dark's and -light's word
      and mark, their cell the 168 of the mark above, on no ground, the box
      snug to the ink across and centred on the cell's rows
  assets/logo/baud-github-org.svg, .png  the organisation picture: "baud" over
      the mark, #F3F4F6 and #F4A32C on #15171B, 500 by 500, the ground edge to
      edge so GitHub's own rounded corners apply; the PNG adds baud's plasma
  assets/readme/social.svg, .png  the repository's link picture, 1280 by 640:
      the lockup and the README's tagline on #15171B, over a strip of amber
      tape punched "baud" in the five-unit code of ITU-T S.1, with its feed holes

In every mark but the Dock's the cell is as tall as the cursor the file had
before (46 at the lockup's 300, 168 at 512, 10.5 at 32), on the same rows,
and the mark's ink is centred across the icon or under the word. **The Dock
icon's mark is a prompt at the start of a terminal's first line**: in the
tile's upper left, in a terminal's own margin, 1.2 times the 512 icons'
cell so it reads at the Dock's 32 pt, the ❯'s cell corner one of their cell
widths (76.36 units) in from the tile's left and top and the mark scaled
about it. The title bar's lockups keep the 512 icons' cell, 168 units, so
the mark beside the traffic lights keeps its size with the word added
before it. **Every PNG is its
SVG drawn by the pinned resvg**, but the organisation picture's: that
is its SVG drawn by resvg with two lights of baud's plasma laid over it,
on the word and on the mark, each drawn by baud's
CPU renderer (baud-render/examples/brand_plasma.rs, run through cargo):

  the word's  the launch splash's first pass at 0.0999 s, its seed BA0D276F,
      its box ending where the word does, placed by the similarity that takes
      the lockup's word onto the picture's, in the word's colour (the splash's
      rule: the light is the colour of the glyph it is over), under the glyphs
  the mark's  the cursor's plasma trail 0.12 s into a move from the ❯'s cell
      into the cursor's, seed 3, drawn at 200 px and scaled to the picture's
      cell (Lanczos), in the amber, over the glyphs

Each light is taken out of a frame drawn alone on the ground as each pixel's
share of the way to the amber, which is how the CPU path blends it
(Blend::Native, source-over in encoded sRGB). With both lights at zero the
picture is its SVG's drawing to the pixel; the run checks that before it
writes, and prints the lights' provenance.

Every letter is a path traced from the font with fontTools, so no renderer can
draw it in another face: before this, every lockup and icon named the font,
and a viewer without it drew the ❯ from a fallback face of another width and
height. 500 for the wordmark and 700 for the ❯ are the brand sheet's
(assets/logo/README.md); the tagline is 400, as on the card before it, since
the sheet names no weight for it. 400 and 700 are the repository's own copies
(baud-app/fonts/jetbrains-mono/), held here to the hashes its SOURCE pins.
500 is in no committed file: it is read out of the JetBrains Mono 2.304
release archive that SOURCE names ([[source: jbm-2304-release]]), fetched by
URL (or BRAND_ARCHIVE, a copy of it) and refused unless the archive and the
face hash as pinned, and nothing of it is kept.

GitHub's limits, from its help pages: the organisation picture is held to the
profile picture's, "less than 1 MB in size and smaller than 3000 by 3000
pixels ... about 500 by 500 pixels" ([[source: github-docs-profile-picture]];
GitHub states no limit for an organisation, so the profile's is applied); the
link picture to "under 1 MB ... at least 640 by 320 pixels (1280 by 640
pixels for best display)", with a solid background where unsure
([[source: github-docs-social-preview]]). 1 MB is read as 1,000,000 bytes, the
smaller of its two meanings. Opaque is by construction: each PNG is RGB
(colour type 2, no alpha channel) with no chunk but IHDR, IDAT and IEND, and
every pixel resvg drew is checked opaque before the alpha is dropped. The
Dock PNGs keep their alpha: the icon's padding is transparent.

The tape: S.1's Table 1 gives each letter's five elements, and its clause
3.2 "Condition Z corresponds to ... perforation in paper tape"
([[source: itu-t-s1]]), so a Z is a hole. Element 1 is drawn on the top track (S.1 gives no geometry). The
holes are on one pitch across the tape, the feed holes 0.046/0.072 of a code
hole, the two diameters ANSI X3.18 gives for one-inch tape
([[source: ansi-x3-18-1967]]).
The feed track is drawn between elements 2 and 3, as five-hole tape is
usually drawn; no text fetched here places it (S.1 gives no geometry, and
X3.18 is the eight-track tape, whose feed track has three code tracks on one
side). The check reads the tape back off the PNG it wrote.

**The environment is pinned whole, or the bytes move.** The script builds
Python and resvg from one nixpkgs revision fetched by hash (build-fonts.sh's),
runs itself again under that Python with -I, and refuses any other version of
the tools TOOLS names; the plasma comes from the repository's own renderer,
built by the cargo on PATH. Measured on aarch64-darwin only: whether resvg
draws the same bytes on another system is unmeasured, which is why --check
runs by hand and not in CI (it also needs the network, for the archive).
baud-app/tests/brand_pictures.rs holds the committed files to the parts that
need neither: the limits, outlines in place of text, and the mark against
baud's renderer.
"""

import glob
import hashlib
import io
import json
import math
import os
import re
import struct
import subprocess
import sys
import tempfile
import zipfile

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

NIXPKGS_REV = "7dd199b0e2993e37b4775ed66b1c291699608c9f"
NIXPKGS_SHA256 = "08kxnni9l87i089hy7p9a7yr64w6h9x5m2cc7k2hn455hidm9mm3"
TOOLS = {"python": "3.14.7", "fontTools": "4.63.0", "Pillow": "12.3.0", "zlib": "1.3.2", "resvg": "0.48.1"}

JBM = "baud-app/fonts/jetbrains-mono"
# weight: (committed file, bytes, sha256), as baud-app/fonts/jetbrains-mono/SOURCE
COMMITTED = {
    400: ("JetBrainsMono-Regular.ttf", 273900, "a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f"),
    700: ("JetBrainsMono-Bold.ttf", 277828, "5590990c82e097397517f275f430af4546e1c45cff408bde4255dad142479dcb"),
}
ARCHIVE_URL = "https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip"
ARCHIVE = (5622857, "6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf")
MEDIUM = ("fonts/ttf/JetBrainsMono-Medium.ttf", 273860, "31c92d01a8a08528b718a43addf0ad3df0af2ca4b7b3290a452f70f358e14d3d")

# baud's cell at 200 px: width round(0.6 px), height round(1.32 px), the
# baseline round(1.02 px) below the cell's top.
FONT_PX, CELL_W, CELL_H, BASELINE = 200, 120, 264, 204

# The brand sheet's colours (assets/logo/README.md).
GROUND = "#15171B"
TYPE_DARK = "#F3F4F6"
TYPE_LIGHT = "#1E2128"
AMBER = "#F4A32C"
# The light forms' amber is no literal: it is baud's cursor on the light
# theme's page, BaudCursor::light() (baud-render/src/color.rs), which the
# plasma example reports (plasma()), so one amber is drawn on light.

# The README's tagline (line 8, between its **), a card line for each part.
LINES = ["The terminal with character:", "its own font,", "its own cursor effects,", "and the speed to keep up."]
TAGLINE = " ".join(LINES)

LOGO = "assets/logo/"
ORG = (LOGO + "baud-github-org", 500)
CARD = ("assets/readme/social", 1280, 640)
DOCK = LOGO + "baud-icon-dock"
# The iconset's sizes, icon_16x16 to icon_512x512@2x (bundle-macos.sh).
DOCK_SIZES = (16, 32, 64, 128, 256, 512, 1024)
# The Dock's mark: 1.2 times the 512 icons' cell, scaled about the ❯'s
# cell corner, which is one of their cell widths (168 * 120 / 264, 76.36
# units of the 512 tile) in from the tile's left and top.
DOCK_SCALE = 1.2
DOCK_CORNER = CELL_W * 168.0 / CELL_H
TIGHT = LOGO + "baud-mark-amber-tight.svg"
# The title bar's lockups, by the appearance each is for.
TITLEBAR = {"dark": LOGO + "baud-lockup-dark-tight.svg", "light": LOGO + "baud-lockup-light-tight.svg"}
MB = 1_000_000

# The lockup, 300 by 130: the word at x 40, baseline 88, size 62,
# letter-spacing -2.8; the ❯'s cell from x 192, on the rows of the cursor
# it had before (45 to 91).
LOCKUP = (300, 130)
WORD = (40.0, 88.0, 62.0, -2.8)
LOCKUP_CELL = (192.0, 45.0, 46.0)
# The organisation picture: the word at 160, and the word and mark rows as
# they were, three quarters of the old 22-unit cursor apart.
ORG_WORD = 160.0
ORG_GAP = 0.75 * 22.0

# ITU-T S.1 Table 1, letter case: combination 2 B, 1 A, 21 U, 4 D, 31 Space.
S1 = {"b": "ZAAZZ", "a": "ZZAAA", "u": "ZZZAA", "d": "ZAAZA", " ": "AAZAA"}
STREAM = "baud " * 9
COLUMNS = 44  # nine words, the first and the last whole
TAPE = dict(pitch_x=28, pitch_y=21, r=6.5, margin=20, bottom=616)
TAPE["r_feed"] = TAPE["r"] * 0.046 / 0.072
# Tracks from the top: elements 1 and 2, the feed track, elements 3, 4 and 5.
TRACKS = ["1", "2", "feed", "3", "4", "5"]


def fail(msg):
    raise SystemExit(f"make-brand-pictures: {msg}")


def sha256(b):
    return hashlib.sha256(b).hexdigest()


# ---------------------------------------------------------------- the pinned run


def outer(args):
    expr = (
        "let pkgs = import (builtins.fetchTarball {"
        f' url = "https://github.com/NixOS/nixpkgs/archive/{NIXPKGS_REV}.tar.gz";'
        f' sha256 = "{NIXPKGS_SHA256}"; }}) {{ config = {{}}; overlays = []; }};'
        " in [ (pkgs.python3.withPackages (p: [ p.fonttools p.pillow ])) pkgs.resvg ]"
    )
    print(f"tools: nixpkgs {NIXPKGS_REV}", file=sys.stderr)
    out = subprocess.run(["nix-build", "--no-out-link", "-E", expr], check=True, stdout=subprocess.PIPE, text=True)
    py, resvg = out.stdout.split()
    py, resvg = f"{py}/bin/python3", f"{resvg}/bin/resvg"
    os.execv(py, [py, "-I", os.path.abspath(__file__), "--pinned", resvg, *args])


def tool_versions(resvg):
    import zlib

    import fontTools
    import PIL

    have = {
        "python": "%d.%d.%d" % sys.version_info[:3],
        "fontTools": fontTools.version,
        "Pillow": PIL.__version__,
        "zlib": zlib.ZLIB_RUNTIME_VERSION,
        "resvg": subprocess.run([resvg, "--version"], check=True, stdout=subprocess.PIPE, text=True).stdout.strip(),
    }
    if have != TOOLS:
        fail(f"the tools are not the pinned ones: {have}, not {TOOLS}")


# ---------------------------------------------------------------- the fonts


def fonts(tmp):
    """{weight: TTFont}: 400 and 700 committed, 500 out of the release archive."""
    from fontTools.ttLib import TTFont

    out = {}
    for weight, (name, size, digest) in COMMITTED.items():
        b = open(os.path.join(ROOT, JBM, name), "rb").read()
        if (len(b), sha256(b)) != (size, digest):
            fail(f"{JBM}/{name} is not the file SOURCE pins")
        out[weight] = TTFont(io.BytesIO(b))
    archive = os.environ.get("BRAND_ARCHIVE")
    if not archive:
        archive = os.path.join(tmp, "JetBrainsMono-2.304.zip")
        subprocess.run(["curl", "-fsSL", "-o", archive, ARCHIVE_URL], check=True)
    b = open(archive, "rb").read()
    if (len(b), sha256(b)) != ARCHIVE:
        fail(f"{archive} is not the release archive pinned ({ARCHIVE_URL})")
    with zipfile.ZipFile(io.BytesIO(b)) as z:
        m = z.read(MEDIUM[0])
    if (len(m), sha256(m)) != MEDIUM[1:]:
        fail(f"{MEDIUM[0]} in the archive is not the face pinned")
    out[500] = TTFont(io.BytesIO(m))
    return out


def num(v):
    s = f"{v:.2f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def r2(v):
    return round(v, 2)


def down2(v):
    """v to two decimals, never above it: a box edge that cannot cut ink."""
    return math.floor(round(v * 100, 6)) / 100


def up2(v):
    return math.ceil(round(v * 100, 6)) / 100


class Type:
    """Text to SVG path data, positioned as SVG text is: x the first glyph's
    origin, y the baseline, letter-spacing after every glyph."""

    def __init__(self, faces):
        self.faces = {w: (f, f.getGlyphSet(), f.getBestCmap(), f["head"].unitsPerEm) for w, f in faces.items()}

    def text(self, s, x, y, size, weight, ls=0.0):
        from fontTools.pens.boundsPen import BoundsPen
        from fontTools.pens.svgPathPen import SVGPathPen
        from fontTools.pens.transformPen import TransformPen

        _, gs, cmap, upm = self.faces[weight]
        k = size / upm
        pen = SVGPathPen(gs, ntos=num)
        box = [float("inf"), float("inf"), float("-inf"), float("-inf")]
        cx = x
        for ch in s:
            if ord(ch) not in cmap:
                fail(f"U+{ord(ch):04X} is not in JetBrains Mono at {weight}")
            g = gs[cmap[ord(ch)]]
            g.draw(TransformPen(pen, (k, 0, 0, -k, cx, y)))
            bp = BoundsPen(gs)
            g.draw(bp)
            if bp.bounds:
                a, b, c, d = bp.bounds
                box = [min(box[0], cx + a * k), min(box[1], y - d * k), max(box[2], cx + c * k), max(box[3], y - b * k)]
            cx += g.width * k + ls
        return pen.getCommands(), tuple(box)

    def ink(self, s, size, weight, ls=0.0):
        return self.text(s, 0, 0, size, weight, ls)[1]


# ---------------------------------------------------------------- SVG


def path(d, fill):
    return f'  <path d="{d}" fill="{fill}"/>\n'


def rect(x, y, w, h, fill, extra=""):
    return f'  <rect x="{num(x)}" y="{num(y)}" width="{num(w)}" height="{num(h)}" fill="{fill}"{extra}/>\n'


def square(n, rx, fill):
    return f'  <rect width="{n}" height="{n}" rx="{rx}" fill="{fill}"/>\n'


def circle(cx, cy, r, fill):
    return f'  <circle cx="{num(cx)}" cy="{num(cy)}" r="{num(r)}" fill="{fill}"/>\n'


NOTE = (
    "  <!-- Drawn by scripts/make-brand-pictures.py: edit the script, not this file. The\n"
    "       glyphs are JetBrains Mono 2.304's own outlines, under its OFL\n"
    "       (JetBrainsMono-OFL.txt beside this file). -->\n"
)


def svg(w, h, body, label="baud", view=None):
    view = view or f"0 0 {w} {h}"
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="{view}" '
        f'role="img" aria-label="{label}">\n{NOTE}{body}</svg>\n'
    )


# ---------------------------------------------------------------- the mark


def mark(t, cell_x, cell_top, h, fill):
    """baud's "❯" in the cell at (cell_x, cell_top), h tall, and the cursor
    in the next cell: the ❯ as the renderer places a glyph, the cursor the
    cell whole. The ❯'s path comes right before the cursor's rect, in one
    fill, which is how brand_pictures.rs finds them. The cell's edges are
    rounded to the SVG's two decimals first, so the cursor's edges are the
    cell's as written. Returns (body, the ❯'s ink box, the cursor's box)."""
    k = h / CELL_H
    top, bottom = r2(cell_top), r2(cell_top + h)
    x1, x2 = r2(cell_x + CELL_W * k), r2(cell_x + 2 * CELL_W * k)
    d, ink = t.text("❯", cell_x, top + BASELINE * k, FONT_PX * k, 700)
    return path(d, fill) + rect(x1, top, x2 - x1, bottom - top, fill), ink, (x1, top, x2, bottom)


def span(ink, cursor):
    """The mark's ink, left to right: the ❯'s left to the cursor's right."""
    return min(ink[0], cursor[0]), max(ink[2], cursor[2])


def centred(t, cx, cell_top, h, fill):
    """The mark, h tall from cell_top, its ink centred on x = cx."""
    _, ink, cur = mark(t, 0.0, 0.0, h, fill)
    left, right = span(ink, cur)
    return mark(t, cx - (left + right) / 2, cell_top, h, fill)


def lockup_dx(t):
    """How far the lockup's glyphs move to centre their ink across its box."""
    _, wb = t.text("baud", WORD[0], WORD[1], WORD[2], 500, WORD[3])
    _, ink, cur = mark(t, *LOCKUP_CELL, GROUND)
    return LOCKUP[0] / 2 - (min(wb[0], ink[0]) + max(wb[2], cur[2])) / 2


def lockup(t, ox, oy, s, word_fill, mark_fill):
    """The lockup with its 300-wide box's origin at (ox, oy), scaled by s:
    the body, its ink box, and the cursor's width."""
    dx = lockup_dx(t)
    x, y, size, ls = WORD
    w, wb = t.text("baud", ox + (x + dx) * s, oy + y * s, size * s, 500, ls * s)
    cx, cy, h = LOCKUP_CELL
    m, ink, cur = mark(t, ox + (cx + dx) * s, oy + cy * s, h * s, mark_fill)
    box = (min(wb[0], ink[0]), min(wb[1], ink[1], cur[1]), max(wb[2], cur[2]), max(wb[3], ink[3], cur[3]))
    return path(w, word_fill) + m, box, cur[2] - cur[0]


# ---------------------------------------------------------------- the files


def logo_svgs(t, amber_light):
    """Every SVG in assets/logo but the organisation picture's, the light
    forms in `amber_light`."""
    out = {}
    w, h = LOCKUP
    for name, word, amber, ground in (
        ("dark", TYPE_DARK, AMBER, GROUND),
        ("light", TYPE_LIGHT, amber_light, None),
        ("mono-black", "#000000", "#000000", None),
        ("mono-white", "#FFFFFF", "#FFFFFF", None),
    ):
        body, _, _ = lockup(t, 0.0, 0.0, 1.0, word, amber)
        out[f"{LOGO}baud-lockup-{name}.svg"] = svg(w, h, (rect(0, 0, w, h, ground) if ground else "") + body)

    # At 512 the cell is 168 tall from 172, as the cursor was; at 32, 10.5
    # from 10.75. Light is the dark one reversed, and amber is light's.
    big, small = (256.0, 172.0, 168.0), (16.0, 10.75, 10.5)
    for name, fill in (("dark", AMBER), ("light", amber_light), ("amber", AMBER)):
        m, _, _ = centred(t, *big, fill)
        out[f"{LOGO}baud-mark-{name}.svg"] = svg(512, 512, m)
    for name, sq, fill in (("dark", GROUND, AMBER), ("light", AMBER, GROUND), ("amber", AMBER, GROUND)):
        m, _, _ = centred(t, *big, fill)
        out[f"{LOGO}baud-icon-512-{name}.svg"] = svg(512, 512, square(512, 112, sq) + m)
    for name, sq, fill in (("dark", GROUND, AMBER), ("light", AMBER, GROUND)):
        m, _, _ = centred(t, *small, fill)
        out[f"{LOGO}baud-icon-32-{name}.svg"] = svg(32, 32, square(32, 7, sq) + m)
    for kind in ("lockup", "mark", "icon-512"):
        for name in ("dark", "light"):
            out[f"{LOGO}baud-{kind}-{name}-blinking.svg"] = out[f"{LOGO}baud-{kind}-{name}.svg"]

    # The Dock icon: the dark tile at 80% of the canvas, on the macOS
    # app-icon grid, the mark in its upper left at DOCK_SCALE times the
    # 512 icons' cell, the ❯'s cell corner DOCK_CORNER in from its left and
    # top: a prompt at the start of a terminal's first line, in a
    # terminal's own margin, and big enough to read at the Dock's 32 pt.
    d, _, _ = mark(t, DOCK_CORNER, DOCK_CORNER, big[2] * DOCK_SCALE, AMBER)
    inner = "".join("  " + line + "\n" for line in d.rstrip("\n").split("\n"))
    dock = (
        "  <!-- macOS app-icon grid: the rounded square is ~80% of the canvas with\n"
        "       transparent padding, so it sits in the Dock like every other app.\n"
        "       Dark squircle, amber mark. -->\n"
        '  <g transform="translate(50,50) scale(0.8047)">\n'
        '    <rect width="512" height="512" rx="115" fill="#15171B"/>\n'
        f"{inner}  </g>\n"
    )
    out[DOCK + ".svg"] = svg(512, 512, dock)
    # The titlebar's box keeps its 300 by 204 and its y, and centres the
    # mark's ink across it: the 512 icons' mark, centred.
    m, ink, cur = centred(t, *big, AMBER)
    left, right = span(ink, cur)
    out[TIGHT] = svg(300, 204, m, view=f"{num((left + right) / 2 - 150)} 154 300 204")

    # The title bar's lockups: the dark and light lockups' word and mark,
    # scaled so the cell is the mark's 168 above, which keeps the mark the
    # size it was beside the traffic lights, and on no ground, since the
    # strip is the ground. The box is snug to the ink across, the word's
    # left to the cursor's right, so the image ends where the ink does; up
    # and down it is centred on the cell's rows, so a view the image's
    # shape centres the mark in the strip where the mark alone was.
    s = big[2] / LOCKUP_CELL[2]
    for name, word, amber in (("dark", TYPE_DARK, AMBER), ("light", TYPE_LIGHT, amber_light)):
        body, box, _ = lockup(t, 0.0, 0.0, s, word, amber)
        top = r2(LOCKUP_CELL[1] * s)
        centre = top + (r2(top + big[2]) - top) / 2
        half = max(centre - box[1], box[3] - centre)
        x0, x1, y0, y1 = down2(box[0]), up2(box[2]), down2(centre - half), up2(centre + half)
        w, h = num(x1 - x0), num(y1 - y0)
        out[TITLEBAR[name]] = svg(w, h, body, view=f"{num(x0)} {num(y0)} {w} {h}")
    return out


def organisation(t):
    """The organisation picture's SVG, its word's pen, the word's scale
    from the lockup's, and its mark's cells: the ❯'s left, the top, the
    cursor's left and right, the bottom."""
    _, n = ORG
    s = ORG_WORD / WORD[2]
    wx = t.ink("baud", ORG_WORD, 500, WORD[3] * s)
    ww, wh = wx[2] - wx[0], wx[3] - wx[1]
    h = LOCKUP_CELL[2] * s
    gap = ORG_GAP * s
    top = (n - (wh + gap + h)) / 2
    pen = (n / 2 - ww / 2 - wx[0], top - wx[1])
    w, wb = t.text("baud", *pen, ORG_WORD, 500, WORD[3] * s)
    m, ink, cur = centred(t, n / 2, top + wh + gap, h, AMBER)
    box = (min(wb[0], ink[0]), wb[1], max(wb[2], cur[2]), max(ink[3], cur[3]))
    cw = cur[2] - cur[0]
    clear = min(box[0], box[1], n - box[2], n - box[3])
    if clear < cw:
        fail(f"the organisation picture's logo has {clear:.1f} px to an edge, under a cursor width ({cw:.1f})")
    cells = (cur[0] - cw, cur[1], cur[0], cur[2], cur[3])
    return svg(n, n, rect(0, 0, n, n, GROUND) + path(w, TYPE_DARK) + m), pen, s, cells


def card(t):
    _, W, H = CARD
    p = TAPE
    band_h = (len(TRACKS) - 1) * p["pitch_y"] + 2 * p["margin"]
    band_top = p["bottom"] - band_h
    mid = band_top / 2
    # The lockup 440 wide with its box at x 60, the tagline at x 560, both on mid.
    s = 440 / LOCKUP[0]
    _, probe, _ = lockup(t, 0, 0, s, GROUND, GROUND)
    lb, lbox, cw = lockup(t, 60, mid - (probe[1] + probe[3]) / 2, s, TYPE_DARK, AMBER)
    size, pitch, cap = 38, 56, 0.73 * 38
    tb, tbox = "", [float("inf")] * 2 + [float("-inf")] * 2
    for i, line in enumerate(LINES):
        d, b = t.text(line, 560, mid + (i - (len(LINES) - 1) / 2) * pitch + cap / 2, size, 400)
        tb += path(d, TYPE_DARK)
        tbox = [min(tbox[0], b[0]), min(tbox[1], b[1]), max(tbox[2], b[2]), max(tbox[3], b[3])]
    if lbox[2] + cw > tbox[0] or max(lbox[3], tbox[3]) + cw > band_top or min(lbox[1], tbox[1]) < cw:
        fail("the card's lockup or tagline is inside the lockup's clear space of another element")
    if tbox[2] > W - 29:
        fail("the tagline reaches the 29 px a 1.91:1 crop of the card would trim")
    tape = rect(-1, band_top, W + 2, band_h, AMBER)
    x0 = (W - (COLUMNS - 1) * p["pitch_x"]) / 2
    for c in range(COLUMNS):
        code = S1[STREAM[c]]
        cx = x0 + c * p["pitch_x"]
        for row, track in enumerate(TRACKS):
            cy = band_top + p["margin"] + row * p["pitch_y"]
            if track == "feed":
                tape += circle(cx, cy, p["r_feed"], GROUND)
            elif code[int(track) - 1] == "Z":
                tape += circle(cx, cy, p["r"], GROUND)
    label = f"baud: {TAGLINE}"
    return svg(W, H, rect(0, 0, W, H, GROUND) + lb + tb + tape, label=label)


def read_tape(png):
    """The letters the card's tape spells, read back off its pixels with S.1."""
    from PIL import Image

    im = Image.open(io.BytesIO(png)).convert("RGB")
    p = TAPE
    band_top = p["bottom"] - ((len(TRACKS) - 1) * p["pitch_y"] + 2 * p["margin"])
    x0 = (im.width - (COLUMNS - 1) * p["pitch_x"]) / 2
    letters = {v: k for k, v in S1.items()}

    def hole(x, y):
        return sum(im.getpixel((round(x), round(y)))) < 200  # the ground shows through

    out, feed = "", 0
    for c in range(COLUMNS):
        cx = x0 + c * p["pitch_x"]
        code = ""
        for row, track in enumerate(TRACKS):
            cy = band_top + p["margin"] + row * p["pitch_y"]
            if track == "feed":
                # a hole at the column and tape on both sides of it, at the
                # code hole's radius: a feed hole, smaller than a code hole
                feed += hole(cx, cy) and not hole(cx - p["r"] + 1, cy) and not hole(cx + p["r"] - 1, cy)
            else:
                code += "Z" if hole(cx, cy) else "A"
        out += letters.get(code, "?")
    return out, feed


# ---------------------------------------------------------------- PNG


def drawn(resvg, text, tmp, name, size=None):
    """resvg's drawing of the SVG, as a Pillow RGBA image."""
    from PIL import Image

    s, p = os.path.join(tmp, name + ".svg"), os.path.join(tmp, name + ".rgba.png")
    open(s, "w", encoding="utf-8").write(text)
    scale = ["-w", str(size), "-h", str(size)] if size else []
    subprocess.run([resvg, "--skip-system-fonts", *scale, s, p], check=True)
    im = Image.open(p)
    im.load()
    if im.mode != "RGBA":
        fail(f"{name}: resvg drew {im.mode}, not RGBA")
    return im


def opaque(im, name):
    """The image as RGB, refused if any pixel resvg drew is not opaque."""
    from PIL import Image

    if im.getchannel("A").getextrema() != (255, 255):
        fail(f"{name}: resvg drew a pixel that is not opaque")
    return Image.frombytes("RGB", im.size, im.convert("RGB").tobytes())


def png(im):
    """The PNG bytes, with no chunk but IHDR, IDAT and IEND."""
    buf = io.BytesIO()
    im.save(buf, "PNG", optimize=True)
    return buf.getvalue()


def chunks(b):
    if b[:8] != b"\x89PNG\r\n\x1a\n":
        return None, []
    i, out, ihdr = 8, [], None
    while i + 8 <= len(b):
        n = int.from_bytes(b[i : i + 4], "big")
        kind = b[i + 4 : i + 8].decode("latin-1")
        if kind == "IHDR":
            ihdr = struct.unpack(">IIBB", b[i + 8 : i + 18])
        out.append(kind)
        i += 12 + n
    return ihdr, out


def github_limits(name, b, square):
    """What is wrong with the PNG as GitHub's picture, or nothing."""
    ihdr, kinds = chunks(b)
    if ihdr is None:
        return [f"{name}: not a PNG"]
    w, h, depth, colour = ihdr
    bad = []
    if len(b) >= MB:
        bad.append(f"{name}: {len(b)} bytes, not under 1 MB")
    if square and not (w == h == ORG[1]):
        bad.append(f"{name}: {w}x{h}, not the {ORG[1]} square GitHub recommends")
    if square and max(w, h) >= 3000:
        bad.append(f"{name}: {w}x{h}, not smaller than 3000 by 3000")
    if not square and (w, h) != CARD[1:]:
        bad.append(f"{name}: {w}x{h}, not the 1280 by 640 GitHub recommends")
    if (depth, colour) != (8, 2):
        bad.append(f"{name}: bit depth {depth}, colour type {colour}: not 8-bit RGB, which has no alpha")
    if [k for k in kinds if k not in ("IHDR", "IDAT", "IEND")] or kinds[:1] != ["IHDR"] or kinds[-1:] != ["IEND"]:
        bad.append(f"{name}: chunks {sorted(set(kinds))}, not IHDR, IDAT and IEND alone")
    return bad


# ---------------------------------------------------------------- the plasma


def rgb(hexa):
    return tuple(int(hexa[i : i + 2], 16) for i in (1, 3, 5))


def plasma(t, tmp, pen, s):
    """Run the plasma example, the word's light placed so the lockup's word
    (its pen at WORD's x moved by lockup_dx, and its baseline, as the
    splash draws it) lands on the picture's (its pen at `pen`, s times the
    size). Refuses a splash whose word pen or block is not the lockup's.
    Returns its report and the frames' folder."""
    out = os.path.join(tmp, "plasma")
    os.makedirs(out)
    dx = lockup_dx(t)
    x0 = WORD[0] + dx
    at = (pen[0] - x0 * s, pen[1] - WORD[1] * s)
    cmd = ["cargo", "run", "--quiet", "--release", "--locked", "-p", "baud-render", "--example", "brand_plasma", "--"]
    subprocess.run([*cmd, out, repr(s), repr(at[0]), repr(at[1])], check=True, cwd=ROOT)
    rep = json.load(open(os.path.join(out, "plasma.json")))
    _, _, cur = mark(t, LOCKUP_CELL[0] + dx, LOCKUP_CELL[1], LOCKUP_CELL[2], GROUND)
    w = rep["word"]
    if abs(w["pen"][0] - x0) > 1e-3 or w["pen"][1] != WORD[1]:
        fail(f"the splash's word pen is {w['pen']}, not the lockup's ({x0:.3f}, {WORD[1]})")
    if max(abs(a - b) for a, b in zip(w["hole"], cur)) > 0.01:
        fail(f"the splash's block is {w['hole']}, not the lockup's cursor {cur}")
    m = rep["mark"]
    if (m["px"], m["cell_w"], m["cell_h"], m["baseline"]) != (FONT_PX, CELL_W, CELL_H, BASELINE):
        fail(f"baud's renderer draws a {m['cell_w']} by {m['cell_h']} cell, baseline {m['baseline']}, at {m['px']} px")
    a = rep["amber"]
    if a["dark"] != AMBER:
        fail(f"baud's cursor on the dark page is {a['dark']}, not the brand's {AMBER}")
    if not re.fullmatch(r"#[0-9A-F]{6}", a["light"]):
        fail(f"baud's cursor on the light page reads {a['light']!r}, not a colour")
    return rep, out


def frame(d, name, w, h):
    from PIL import Image

    b = open(os.path.join(d, name + ".raw"), "rb").read()
    return Image.frombytes("RGBA", (w, h), b, "raw", "BGRA").convert("RGB")


def org_picture(base, rep, d, cells):
    """The organisation picture: its SVG's drawing (base, RGB) with the
    word's light under the glyphs and the mark's over them, each taken from
    its frame in `d`. Returns the picture, the lights' peak, and how many
    pixels they changed."""
    from PIL import Image

    G, C, T = rgb(GROUND), rgb(AMBER), rgb(TYPE_DARK)
    D = [C[i] - G[i] for i in range(3)]
    N2 = sum(v * v for v in D)

    def cover(p):
        return max(0.0, min(1.0, sum((p[i] - G[i]) * D[i] for i in range(3)) / N2))

    n = base.width
    # The mark's light: the trail's frame against the frame without it,
    # none where the glyph or the cursor is all but whole, scaled from the
    # 200 px cell to the picture's.
    m = rep["mark"]
    W, H = m["width"], m["height"]
    bp, tp = frame(d, "mark-base", W, H).load(), frame(d, "mark-trail", W, H).load()
    alpha = Image.new("F", (W, H), 0.0)
    ap = alpha.load()
    for y in range(H):
        for x in range(W):
            cb, ct = cover(bp[x, y]), cover(tp[x, y])
            ap[x, y] = 0.0 if cb > 0.98 else max(0.0, min(1.0, (ct - cb) / (1 - cb)))
    cell_x, cell_top, _, _, cell_bottom = cells
    f = (cell_bottom - cell_top) / CELL_H
    lay = alpha.resize((max(1, round(W * f)), max(1, round(H * f))), Image.LANCZOS)
    ox, oy = round(cell_x - m["col"] * CELL_W * f), round(cell_top - m["row"] * CELL_H * f)
    am = [[0.0] * n for _ in range(n)]
    lp = lay.load()
    for y in range(lay.height):
        for x in range(lay.width):
            a = lp[x, y]
            if a > 0 and 0 <= ox + x < n and 0 <= oy + y < n:
                am[oy + y][ox + x] = min(1.0, a)
    # The word's light, under each pixel's glyph coverage: its share of the
    # way to its glyph's colour, the word's above the picture's middle and
    # the amber below.
    w = rep["word"]
    wp = frame(d, "word", w["width"], w["height"]).load()
    op = base.load()
    out = Image.new("RGB", (n, n))
    pp = out.load()
    peak, lit = 0.0, 0
    for y in range(n):
        col = T if y < n / 2 else C
        dc = [col[i] - G[i] for i in range(3)]
        n2 = sum(v * v for v in dc)
        for x in range(n):
            b = op[x, y]
            aw = cover(wp[x, y])
            cg = max(0.0, min(1.0, sum((b[i] - G[i]) * dc[i] for i in range(3)) / n2))
            a = am[y][x]
            v = [round(b[i] + ((1.0 - cg) * aw) * (T[i] - G[i])) for i in range(3)]
            v = tuple(min(255, max(0, round(v[i] * (1.0 - a) + C[i] * a))) for i in range(3))
            pp[x, y] = v
            peak, lit = max(peak, aw, a), lit + (v != b)
    return out, peak, lit


# ---------------------------------------------------------------- the run


def outlines_only(name, text, allowed=("svg", "g", "path", "rect", "circle")):
    bare = re.sub(r"<!--.*?-->", "", text, flags=re.S)
    tags = set(re.findall(r"<([A-Za-z][\w:-]*)", bare))
    if not tags <= set(allowed) or "path" not in tags:
        fail(f"{name}: elements {sorted(tags)}: an outline source holds paths and shapes alone")
    # A font named in an attribute or a style: font-family="...", font: ...
    if re.search(r"font(-[a-z]+)?\s*[=:]", bare, re.I):
        fail(f"{name}: names a font")


def draw(resvg, tmp):
    readme = open(os.path.join(ROOT, "README.md"), encoding="utf-8").read().splitlines()
    if readme[7] != f"**{TAGLINE}**":
        fail(f"README.md line 8 is not the card's tagline: {readme[7]!r}")
    t = Type(fonts(tmp))
    org, pen, s, cells = organisation(t)
    rep, d = plasma(t, tmp, pen, s)
    text = logo_svgs(t, rep["amber"]["light"])
    text[ORG[0] + ".svg"] = org
    text[CARD[0] + ".svg"] = card(t)
    for name, body in text.items():
        outlines_only(name, body)
    out = {name: body.encode() for name, body in text.items()}

    for size in DOCK_SIZES:
        im = drawn(resvg, text[DOCK + ".svg"], tmp, f"dock-{size}", size)
        if im.size != (size, size):
            fail(f"the Dock icon drew {im.size} at {size}")
        out[f"{DOCK}-{size}.png"] = png(im)
    card_png = png(opaque(drawn(resvg, text[CARD[0] + ".svg"], tmp, "social"), "social"))
    spelled, feed = read_tape(card_png)
    if spelled != STREAM[:COLUMNS] or feed != COLUMNS:
        fail(f"the tape reads {spelled!r} with {feed} feed holes, not {STREAM[:COLUMNS]!r} with {COLUMNS}")
    out[CARD[0] + ".png"] = card_png

    base = opaque(drawn(resvg, org, tmp, "org"), "org")
    pic, peak, lit = org_picture(base, rep, d, cells)
    # The control: with both lights at zero, every frame the bare ground,
    # the same blend draws the SVG's drawing, every pixel.
    dark = os.path.join(tmp, "dark")
    os.makedirs(dark)
    ground = bytes([*reversed(rgb(GROUND)), 0xFF])
    for name, key in (("word", "word"), ("mark-base", "mark"), ("mark-trail", "mark")):
        open(os.path.join(dark, name + ".raw"), "wb").write(ground * (rep[key]["width"] * rep[key]["height"]))
    zero, _, moved = org_picture(base, rep, dark, cells)
    if moved or zero.tobytes() != base.tobytes():
        fail(f"with no light the organisation picture is not its SVG's drawing: {moved} pixels differ")
    if not lit:
        fail("the plasma lit no pixel of the organisation picture")
    out[ORG[0] + ".png"] = png(pic)
    w, m = rep["word"], rep["mark"]
    print(
        f"plasma: the word's light is the splash's first pass at {w['t']} s, seed {w['seed']}, its box {w['box']}"
        f" and hole {w['hole']} at scale {w['scale']} from {w['origin']}, the splash's own over"
        f" {w['gate_columns']} columns; the mark's is the cursor's trail at {m['age']} s, seed {m['seed']}, at"
        f" {m['px']} px; {lit} pixels lit, peak {peak:.3f}; with no light, the SVG's drawing to the pixel;"
        f" the light forms' amber is baud's cursor on the light page, {rep['amber']['light']}"
    )
    return out


def main(argv):
    if argv[:1] != ["--pinned"]:
        outer(argv)
    resvg, args = argv[1], argv[2:]
    if args not in ([], ["--check"]):
        fail("usage: make-brand-pictures.py [--check]")
    check = args == ["--check"]
    tool_versions(resvg)
    with tempfile.TemporaryDirectory() as tmp:
        out = draw(resvg, tmp)
    bad = []
    there = {os.path.relpath(p, ROOT) for p in glob.glob(os.path.join(ROOT, LOGO, "*.svg"))}
    there |= {os.path.relpath(p, ROOT) for p in glob.glob(os.path.join(ROOT, LOGO, "*.png"))}
    there |= {CARD[0] + ".svg", CARD[0] + ".png"}
    for name in sorted(there - set(out)):
        bad.append(f"{name} is a logo file the generator does not draw")
    for name, b in out.items():
        at = os.path.join(ROOT, name)
        if check:
            have = open(at, "rb").read() if os.path.exists(at) else None
            same = have == b
            print(f"{'same   ' if same else 'DIFFERS'} {name} ({len(b)} bytes, sha256 {sha256(b)[:16]})")
            if not same:
                bad.append(f"{name} is not what the generator draws")
        else:
            open(at, "wb").write(b)
            print(f"wrote {name} ({len(b)} bytes)")
    for base, square in ((ORG[0], True), (CARD[0], False)):
        at = os.path.join(ROOT, base + ".png")
        bad += github_limits(base + ".png", open(at, "rb").read() if os.path.exists(at) else b"", square)
    for line in bad:
        print(f"FAIL {line}")
    if bad:
        sys.exit(1)
    print("ok: every logo file is the generator's, and both pictures meet GitHub's limits" if check else "ok")


if __name__ == "__main__":
    main(sys.argv[1:])
