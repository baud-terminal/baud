#!/usr/bin/env python3
"""Draw baud's brand pictures from JetBrains Mono's own outlines.

    python3 scripts/make-brand-pictures.py          write every file below
    python3 scripts/make-brand-pictures.py --check  draw them again in memory and
                                                    fail unless each equals the
                                                    committed file byte for byte,
                                                    and unless both GitHub
                                                    pictures meet GitHub's limits;
                                                    writes nothing

What it writes, each from the code below and nothing else:

  assets/logo/baud-github-org.svg, .png  the organisation picture: "baud"
      stacked over the mark, #15171B on #F4A32C, 500 by 500, the ground edge to
      edge so GitHub's own rounded corners apply
  assets/readme/social.svg, .png  the repository's link picture, 1280 by 640:
      the lockup and the README's tagline on #15171B, over a strip of amber
      tape punched "baud" in the five-unit code of ITU-T S.1, with its feed holes
  assets/logo/baud-icon-dock.svg  the Dock icon (scripts/bundle-macos.sh
      rasterises it; baud-app/src/app.rs embeds it)
  assets/logo/baud-mark-amber-tight.svg  the titlebar mark (app.rs embeds it)

Every letter is a path traced from the font with fontTools, so no renderer can
draw it in another face: before this, the Dock icon's ❯ named the font and
every renderer that drew it fell back to a system face. 500 for the wordmark
and 700 for the ❯ are the brand sheet's (assets/logo/README.md); the
tagline is 400, as on the card before it, since the sheet names no weight
for it. 400 and 700 are the repository's own copies
(baud-app/fonts/jetbrains-mono/), held here to the hashes its SOURCE pins.
500 is in no committed file: it is read out of the JetBrains Mono 2.304
release archive that SOURCE names ([[source: jbm-2304-release]]), fetched by
URL and refused unless the archive and the face hash as pinned, and nothing
of it is kept.

GitHub's limits, from its help pages: the organisation picture is held to the
profile picture's, "less than 1 MB in size and smaller than 3000 by 3000
pixels ... about 500 by 500 pixels" ([[source: github-docs-profile-picture]];
GitHub states no limit for an organisation, so the profile's is applied); the
link picture to "under 1 MB ... at least 640 by 320 pixels (1280 by 640
pixels for best display)", with a solid background where unsure
([[source: github-docs-social-preview]]). 1 MB is read as 1,000,000 bytes, the
smaller of its two meanings. Opaque is by construction: each PNG is RGB
(colour type 2, no alpha channel) with no chunk but IHDR, IDAT and IEND, and
every pixel resvg drew is checked opaque before the alpha is dropped.

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
the tools TOOLS names. Measured on aarch64-darwin only: whether resvg draws
the same bytes on another system is unmeasured, which is why --check runs by
hand and not in CI (it also needs the network, for the archive).
baud-app/tests/brand_pictures.rs holds the committed files to the parts that
need neither: the limits, and outlines in place of text.
"""

import hashlib
import io
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

# The brand sheet's colours (assets/logo/README.md).
GROUND = "#15171B"
TYPE_DARK = "#F3F4F6"
AMBER = "#F4A32C"

# The README's tagline (line 8, between its **), a card line for each part.
LINES = ["The terminal with character:", "its own font,", "its own cursor effects,", "and the speed to keep up."]
TAGLINE = " ".join(LINES)

ORG = ("assets/logo/baud-github-org", 500)
CARD = ("assets/readme/social", 1280, 640)
DOCK = "assets/logo/baud-icon-dock.svg"
TIGHT = "assets/logo/baud-mark-amber-tight.svg"
MB = 1_000_000

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


# The lockup, assets/logo/baud-lockup-dark.svg's geometry at 300 by 130: the
# word at x 40, baseline 88, size 62, letter-spacing -2.8; the ❯ at x 192,
# baseline 85, size 54; the cursor 22 by 46 at (238, 45).
CURSOR_W = 22.0


def lockup(t, ox, oy, s, word_fill, mark_fill):
    """The lockup with its 300-wide box's origin at (ox, oy), scaled by s."""
    w, wb = t.text("baud", ox + 40 * s, oy + 88 * s, 62 * s, 500, -2.8 * s)
    m, mb = t.text("❯", ox + 192 * s, oy + 85 * s, 54 * s, 700)
    body = path(w, word_fill) + path(m, mark_fill) + rect(ox + 238 * s, oy + 45 * s, 22 * s, 46 * s, mark_fill)
    box = (min(wb[0], mb[0]), min(wb[1], mb[1], oy + 45 * s), ox + 260 * s, max(wb[3], mb[3], oy + 91 * s))
    return body, box


def stacked(t, size, word_size, fill):
    """'baud' over the lockup's own mark row, both centred, three quarters of
    a cursor width between them (the candidate the owner chose)."""
    s = word_size / 62
    wx = t.ink("baud", word_size, 500, -2.8 * s)
    mx = t.ink("❯", 54 * s, 700)
    # The mark row in lockup units: the ❯'s ink from x 192, the cursor to 260.
    mx0, my0, mx1, my1 = 192 + mx[0] / s, 45.0, 260.0, 91.0
    ww, wh = wx[2] - wx[0], wx[3] - wx[1]
    mw, mh = (mx1 - mx0) * s, (my1 - my0) * s
    gap = 0.75 * CURSOR_W * s
    top = (size - (wh + gap + mh)) / 2
    w, wb = t.text("baud", size / 2 - ww / 2 - wx[0], top - wx[1], word_size, 500, -2.8 * s)
    ox = size / 2 - mw / 2 - mx0 * s
    oy = top + wh + gap - my0 * s
    m, mb = t.text("❯", ox + 192 * s, oy + 85 * s, 54 * s, 700)
    body = path(w, fill) + path(m, fill) + rect(ox + 238 * s, oy + 45 * s, 22 * s, 46 * s, fill)
    box = (min(wb[0], mb[0]), wb[1], max(wb[2], ox + mx1 * s), max(mb[3], oy + my1 * s))
    return body, box, CURSOR_W * s


def organisation(t):
    name, n = ORG
    body, box, cw = stacked(t, n, 160, GROUND)
    clear = min(box[0], box[1], n - box[2], n - box[3])
    if clear < cw:
        fail(f"the organisation picture's logo has {clear:.1f} px to an edge, under a cursor width ({cw:.1f})")
    return svg(n, n, rect(0, 0, n, n, AMBER) + body)


def card(t):
    name, W, H = CARD
    p = TAPE
    band_h = (len(TRACKS) - 1) * p["pitch_y"] + 2 * p["margin"]
    band_top = p["bottom"] - band_h
    mid = band_top / 2
    # The lockup 440 wide with its box at x 60, the tagline at x 560, both on mid.
    s = 440 / 300
    _, probe = lockup(t, 0, 0, s, GROUND, GROUND)
    lb, lbox = lockup(t, 60, mid - (probe[1] + probe[3]) / 2, s, TYPE_DARK, AMBER)
    size, pitch, cap = 38, 56, 0.73 * 38
    tb, tbox = "", [float("inf")] * 2 + [float("-inf")] * 2
    for i, line in enumerate(LINES):
        d, b = t.text(line, 560, mid + (i - (len(LINES) - 1) / 2) * pitch + cap / 2, size, 400)
        tb += path(d, TYPE_DARK)
        tbox = [min(tbox[0], b[0]), min(tbox[1], b[1]), max(tbox[2], b[2]), max(tbox[3], b[3])]
    cw = CURSOR_W * s
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


def render(resvg, text, tmp, name):
    """resvg's drawing of the SVG, as an RGB PNG with no chunk but IHDR, IDAT and IEND."""
    from PIL import Image

    s, p = os.path.join(tmp, name + ".svg"), os.path.join(tmp, name + ".rgba.png")
    open(s, "w", encoding="utf-8").write(text)
    subprocess.run([resvg, "--skip-system-fonts", s, p], check=True)
    im = Image.open(p)
    im.load()
    if im.mode != "RGBA" or im.getchannel("A").getextrema() != (255, 255):
        fail(f"{name}: resvg drew a pixel that is not opaque")
    rgb = Image.frombytes("RGB", im.size, im.convert("RGB").tobytes())
    buf = io.BytesIO()
    rgb.save(buf, "PNG", optimize=True)
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


# ---------------------------------------------------------------- the run


def elements_only(name, text, allowed=("svg", "g", "path", "rect", "circle")):
    tags = set(re.findall(r"<([A-Za-z][\w:-]*)", re.sub(r"<!--.*?-->", "", text, flags=re.S)))
    if not tags <= set(allowed):
        fail(f"{name}: elements {sorted(tags - set(allowed))}: an outline source holds paths and shapes alone")


def draw(resvg, tmp):
    from fontTools.ttLib import TTFont  # noqa: F401  (imported here so a missing one fails first)

    readme = open(os.path.join(ROOT, "README.md"), encoding="utf-8").read().splitlines()
    if readme[7] != f"**{TAGLINE}**":
        fail(f"README.md line 8 is not the card's tagline: {readme[7]!r}")
    t = Type(fonts(tmp))
    out = {}
    out[ORG[0] + ".svg"] = organisation(t).encode()
    out[CARD[0] + ".svg"] = card(t).encode()

    # The Dock icon and the titlebar mark: assets/logo/baud-icon-512-dark.svg's
    # ❯ (x 130, baseline 314, size 200, weight 700) and cursor, unchanged but
    # for the ❯, which is the font's outline in place of text naming the font.
    m, mbox = t.text("❯", 130, 314, 200, 700)
    dock = (
        '  <!-- macOS app-icon grid: the rounded square is ~80% of the canvas with\n'
        "       transparent padding, so it sits in the Dock like every other app.\n"
        "       Dark squircle, amber mark. -->\n"
        '  <g transform="translate(50,50) scale(0.8047)">\n'
        '    <rect width="512" height="512" rx="115" fill="#15171B"/>\n'
        f'  {path(m, AMBER)}'
        '    <rect x="306" y="172" width="76" height="168" fill="#F4A32C"/>\n'
        "  </g>\n"
    )
    out[DOCK] = svg(512, 512, dock).encode()
    # The titlebar's box keeps its 300 by 204 and its y, and centres the
    # mark's ink across it: the ❯'s ink to the cursor's right edge, 382.
    tight = path(m, AMBER) + '  <rect x="306" y="172" width="76" height="168" fill="#F4A32C"/>\n'
    out[TIGHT] = svg(300, 204, tight, view=f"{num((mbox[0] + 382) / 2 - 150)} 154 300 204").encode()

    for name, b in out.items():
        elements_only(name, b.decode())
    for base in (ORG[0], CARD[0]):
        out[base + ".png"] = render(resvg, out[base + ".svg"].decode(), tmp, os.path.basename(base))
    spelled, feed = read_tape(out[CARD[0] + ".png"])
    if spelled != STREAM[:COLUMNS] or feed != COLUMNS:
        fail(f"the tape reads {spelled!r} with {feed} feed holes, not {STREAM[:COLUMNS]!r} with {COLUMNS}")
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
    print("ok: the files are the generator's, and both pictures meet GitHub's limits" if check else "ok")


if __name__ == "__main__":
    main(sys.argv[1:])
