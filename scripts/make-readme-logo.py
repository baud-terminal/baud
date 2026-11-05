#!/usr/bin/env python3
"""Film the README's header logo with baud's own renderer and encode it.

    python3 scripts/make-readme-logo.py          write assets/readme/logo-dark.gif,
                                                 logo-light.gif, logo-dark.svg
                                                 and logo-light.svg
    python3 scripts/make-readme-logo.py --check  film and encode all four again in
                                                 memory and fail unless each
                                                 equals the committed file byte
                                                 for byte; writes no tracked file

**What it shows**: the lockup, `baud ❯ █`, as the launch splash holds it
and the logo files draw it, with baud's white plasma moving round and
through it as if each glyph were a solid wall -- in front of the letters
or behind them, crossing only through the counters of b, a and d and the
gaps between the letters, hidden behind a letter but for what shows
through its holes, squashed flat and run along a stroke it meets, a
keyline cut where it passes in front -- carried by a swirling current
with a short tail of wisps, its path a seeded random chain of moves. The
block blinks as baud's cursor blinks by default, the app's own caret
asked for each frame. It moves for the most whole blink cycles that end
before five seconds, nine at the default 250 ms shown and 250 hidden,
4.5 s, fading in from the bare logo and out to it, then rests on the
logo with the block shown. baud-app/src/logo_film.rs draws each frame;
scripts/readme-logo/plasma.py decides what the plasma does.

**Why it plays once and stops**: WCAG 2.2's 2.2.2 Pause, Stop, Hide
(Level A) asks that moving or blinking content which starts by itself,
lasts more than five seconds and is shown beside other content can be
paused, stopped or hidden ([[source: wcag22]]). A README carries no
control of its own, and GitHub's setting to pause animated images does
not reach an image inside `<picture>`, so the logo moves for under five
seconds and rests.

**How it is made**, for each half, on GitHub's page colour, #0d1117 for
the dark theme and #ffffff for the light:
  1. baud-app's logo film draws the bare logo, the block shown;
  2. the dark half's bare logo is held to `assets/readme/logo-walls.png`,
     the drawing the plasma's path was chosen against, which `WALLS_SHA256`
     pins: today's renderer must draw it within `WALLS_OFF`, or the lockup
     has moved and the path must be chosen again against it;
  3. scripts/readme-logo/geometry.py reads the walls' signed distance
     field, normals and counters off it, and scripts/readme-logo/plasma.py
     writes each frame's plasma strands against them, each run with the
     platform's sin, exp and the rest taken away (nolibm.py), so a run
     that finishes is the same on every platform;
  4. the film draws each frame through the renderer's splash layers and
     slabs, the block at the caret's phase, in release's optimisation
     without LTO (a test, not an example: the blink is the app's, and the
     app is a binary whose modules no example reaches);
  5. every pixel within `SNAP` levels of the page is set to it, so the
     encoder's error cannot haze the page round the logo;
  6. gifski encodes the GIF at `QUALITY`, its motion and lossy passes
     whole, the page a fixed colour of its palette, played once; every
     palette entry within `PALETTE_SNAP` levels of the page is then set to
     it, and each frame's delay to the film's, which gifski's frame rate
     cannot say for frames of two lengths;
  7. the SVG is the bare logo as a PNG with each frame over it as a WebP
     of what it changes, shown and hidden by SMIL `<set>` at the GIF's own
     times, all hidden at 4.5 s so it rests on the logo.

**What it checks before it writes**, each from the files it made:
  - gifski and the SVG's encoder, run twice on the same frames, write the
    same bytes;
  - the GIF lasts the film's length, 840 by 580, no loop, under `LIMIT`
    bytes; each of the SVG's frame images ends where the next frame starts;
  - the haze: of the pixels the film drew as the page, the share the
    decoded GIF shows off it, and the worst level;
  - the word's contrast against the ground round it, every frame of both
    files, at least `CONTRAST` to one;
  - WCAG 2.3.1's census of the SVG's frames as Pillow decodes them, at
    100 samples a second and 2 pixels a point, its last frame held 3 s:
    both verdicts pass (baud-bench's flash_census example).
Then, on the files in the tree -- written, or with --check the committed
ones -- it runs the tests that hold them, and prints what they read:
  - baud-app's logo_film::tests::the_readme_logos_move_under_five_seconds_and_rest_shown
    and logo_film::tests::the_readme_svgs_play_the_gifs_times_and_rest_shown;
  - baud-bench's readme_logo_flashes: the census of the GIFs.

**The environment is pinned whole.** Python with Pillow and NumPy comes
from nixpkgs 7dd199b0, tools/shell.nix's pin for the pictures,
fetched by hash, and gifski from readme-film.sh's; the script runs
itself again under that Python with -I and refuses other versions. The
frames come from the repository's renderer, built by the cargo on PATH.
The generator's arithmetic is the same on every platform
(scripts/readme-logo/det.py);
the files were made on aarch64-darwin, and --check runs by hand, not in CI.
"""

import base64
import hashlib
import io
import json
import os
import re
import struct
import subprocess
import sys
import tempfile

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
HERE = os.path.join(ROOT, "scripts", "readme-logo")

NIXPKGS_REV = "7dd199b0e2993e37b4775ed66b1c291699608c9f"
NIXPKGS_SHA256 = "08kxnni9l87i089hy7p9a7yr64w6h9x5m2cc7k2hn455hidm9mm3"
GIFSKI_NIXPKGS = "github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a"
TOOLS = {"python": "3.14.7", "Pillow": "12.3.0", "numpy": "2.5.2", "gifski": "gifski 1.34.0"}

# The GIF: gifski at 90, where 80 left the page round the logo off by up to
# 5 levels in its frames, its motion and lossy passes whole.
QUALITY, MOTION, LOSSY = 90, 100, 100
# Pixels this many levels of the page or nearer are set to it before the
# encoders see them: 1.2% of white, which no eye sees over the page.
SNAP = 3
# GIF palette entries this near the page are set to it after encoding.
PALETTE_SNAP = 6
# The SVG's frames: WebP at this quality, their alpha lossless.
WEBP_QUALITY = 90
# The word against the ground round it, the worst frame: WCAG 1.4.3 asks
# nothing of a logotype ("Text that is part of a logo or brand name has no
# contrast requirement", [[source: wcag22]]); the floor borrows the 4.5 to
# 1 it asks of text, so the plasma never washes the word out.
CONTRAST = 4.5

OUT = {
    "dark": ("assets/readme/logo-dark.gif", "assets/readme/logo-dark.svg"),
    "light": ("assets/readme/logo-light.gif", "assets/readme/logo-light.svg"),
}
PAGE = {"dark": (0x0D, 0x11, 0x17), "light": (0xFF, 0xFF, 0xFF)}
WORD = {"dark": (0xF3, 0xF4, 0xF6), "light": (0x1E, 0x21, 0x28)}
SIZE = (840, 580)
# A README loads its header on every visit: 4.5 s of plasma at 50 frames a
# second is about 3 MB a GIF.
LIMIT = 5_000_000

WALLS = "assets/readme/logo-walls.png"
WALLS_SHA256 = "931c0a901594ee8834194315ce1e36094e4a7dc00a24ea3a4670f48c6dab8638"
# How far today's bare logo may be from the walls, each at the count
# measured when they were frozen: pixels differing and the most a channel
# moves, pixels of the ink's half-coverage mask differing, and the most
# the signed distance moves, in pixels.
WALLS_OFF = {"pixels": 3930, "levels": 2, "mask": 3, "sdf": 1.0}

FAST = [
    "--config",
    'profile.fast.inherits="release"',
    "--config",
    "profile.fast.lto=false",
    "--config",
    "profile.fast.codegen-units=16",
    "--profile",
    "fast",
]
TESTS = [
    [
        "-p",
        "baud",
        "--bin",
        "baud",
        "--",
        "logo_film::tests::the_readme_logos_move_under_five_seconds_and_rest_shown",
        "logo_film::tests::the_readme_svgs_play_the_gifs_times_and_rest_shown",
    ],
    ["-p", "baud-bench", "--test", "readme_logo_flashes"],
]


def fail(msg):
    raise SystemExit(f"make-readme-logo: {msg}")


# The generator's scripts run with the platform's transcendentals taken
# away (scripts/readme-logo/nolibm.py), so a run that finishes is the
# same on every platform.
NOLIBM = [sys.executable, "-I", os.path.join(HERE, "nolibm.py")]


# ---------------------------------------------------------------- the pinned run


def outer(args):
    expr = (
        "let pkgs = import (builtins.fetchTarball {"
        f' url = "https://github.com/NixOS/nixpkgs/archive/{NIXPKGS_REV}.tar.gz";'
        f' sha256 = "{NIXPKGS_SHA256}"; }}) {{ config = {{}}; overlays = []; }};'
        " in pkgs.python3.withPackages (p: [ p.pillow p.numpy ])"
    )
    print(f"tools: nixpkgs {NIXPKGS_REV}, gifski from {GIFSKI_NIXPKGS}", file=sys.stderr)
    run = dict(check=True, stdout=subprocess.PIPE, text=True)
    py = subprocess.run(["nix-build", "--no-out-link", "-E", expr], **run).stdout.strip() + "/bin/python3"
    gif = subprocess.run(["nix", "build", "--no-link", "--print-out-paths", GIFSKI_NIXPKGS + "#gifski"], **run)
    gifski = gif.stdout.strip() + "/bin/gifski"
    os.execv(py, [py, "-I", os.path.abspath(__file__), "--pinned", gifski, *args])


def tool_versions(gifski):
    import numpy
    import PIL
    from PIL import features

    have = {
        "python": "%d.%d.%d" % sys.version_info[:3],
        "Pillow": PIL.__version__,
        "numpy": numpy.__version__,
        "gifski": subprocess.run([gifski, "--version"], check=True, stdout=subprocess.PIPE, text=True).stdout.strip(),
    }
    if have != TOOLS:
        fail(f"the tools are not the pinned ones: {have}, not {TOOLS}")
    if not features.check("webp"):
        fail("the pinned Pillow has no WebP")


# ---------------------------------------------------------------- the film


def cargo_test(args, env):
    cmd = ["cargo", "test", "--quiet", "--locked", "-p", "baud", "--bin", "baud", *FAST, *args]
    subprocess.run(cmd, check=True, cwd=ROOT, env=dict(os.environ, **env), stdout=subprocess.DEVNULL)


def canvas(path):
    import numpy

    raw = open(path, "rb").read()
    if len(raw) != SIZE[0] * SIZE[1] * 3:
        fail(f"{path} is not a canvas")
    return numpy.frombuffer(raw, numpy.uint8).reshape(SIZE[1], SIZE[0], 3)


def bare(half, d):
    """The bare logo, the block shown, and the blink's phases."""
    cargo_test(["logo_film::film_the_readme_logo_bare", "--", "--ignored", "--exact"], {"BAUD_LOGO_OUT": d, "BAUD_LOGO_HALF": half})
    m = json.load(open(os.path.join(d, "bare.json")))
    if (m["width"], m["height"]) != SIZE or m["half"] != half:
        fail(f"{half}: the bare canvas is {m['width']}x{m['height']} of {m['half']}")
    return canvas(os.path.join(d, "bare.rgb")), m


def walls_hold(today, tmp):
    """Hold today's dark bare logo to the walls the path was chosen
    against, by `WALLS_OFF`; the walls' geometry."""
    import numpy
    from PIL import Image

    data = open(os.path.join(ROOT, WALLS), "rb").read()
    if hashlib.sha256(data).hexdigest() != WALLS_SHA256:
        fail(f"{WALLS} is not the walls the path was chosen against")
    walls = numpy.asarray(Image.open(io.BytesIO(data)).convert("RGB"))
    d = numpy.abs(walls.astype(numpy.int16) - today.astype(numpy.int16)).max(axis=2)
    off = {"pixels": int((d > 0).sum()), "levels": int(d.max())}
    g = {}
    for name, img in (("walls", walls), ("today", today)):
        p = os.path.join(tmp, f"{name}.png")
        Image.fromarray(img).save(p)
        g[name] = os.path.join(tmp, f"geom-{name}")
        subprocess.run([*NOLIBM, os.path.join(HERE, "geometry.py"), p, g[name]], check=True)
    sd = {k: numpy.fromfile(os.path.join(v, "sdf.f32"), "<f4") for k, v in g.items()}
    off["mask"] = int(((sd["walls"] < 0) != (sd["today"] < 0)).sum())
    off["sdf"] = round(float(numpy.abs(sd["walls"] - sd["today"]).max()), 3)
    print(f"walls: today's bare logo is {off} off them, at most {WALLS_OFF}")
    if any(off[k] > WALLS_OFF[k] for k in WALLS_OFF):
        fail("the lockup has moved off the walls the plasma's path was chosen against: choose the path again")
    return g["walls"]


def strands(geom, half, m, d):
    """The generator's strands for each frame, and its report."""
    out = os.path.join(d, "strands.txt")
    cmd = [*NOLIBM, os.path.join(HERE, "plasma.py"), geom, half, str(m["on_ms"]), str(m["off_ms"]), out]
    report = json.loads(subprocess.run(cmd, check=True, stdout=subprocess.PIPE, text=True).stdout)
    return out, report


def film(half, d, prims, geom):
    """Film one half into `d`; its report."""
    env = {"BAUD_LOGO_OUT": d, "BAUD_LOGO_HALF": half, "BAUD_LOGO_PRIMS": prims, "BAUD_LOGO_GEOM": geom}
    cargo_test(["logo_film::film_the_readme_logo", "--", "--ignored", "--exact"], env)
    m = json.load(open(os.path.join(d, "film.json")))
    if (m["width"], m["height"]) != SIZE or m["half"] != half:
        fail(f"{half}: the film is {m['width']}x{m['height']} of {m['half']}")
    if m["frames"] != len(m["delays_cs"]):
        fail(f"{half}: {m['frames']} frames and {len(m['delays_cs'])} delays")
    return m


def snapped(d, m, half):
    """The film's frames, every pixel within `SNAP` of the page set to it."""
    import numpy

    n = SIZE[0] * SIZE[1] * 3
    page = numpy.array(PAGE[half], numpy.int16)
    out = []
    with open(os.path.join(d, "frames.rgb"), "rb") as f:
        for _ in range(m["frames"]):
            a = numpy.frombuffer(f.read(n), numpy.uint8).reshape(SIZE[1], SIZE[0], 3).copy()
            a[numpy.abs(a.astype(numpy.int16) - page).max(axis=2) <= SNAP] = page.astype(numpy.uint8)
            out.append(a)
        if f.read(1):
            fail("frames.rgb holds more than the film's frames")
    return out


# ---------------------------------------------------------------- the GIF


def gif_blocks(b):
    """The GIF's colour tables and Graphic Control Extensions, in order:
    ("table", its offset, its entries) and ("gce", its delay's offset, 0)."""
    i, flags, out = 13, b[10], []
    if flags & 0x80:
        out.append(("table", 13, 1 << ((flags & 7) + 1)))
        i += 3 * (1 << ((flags & 7) + 1))
    while i < len(b):
        c = b[i]
        if c == 0x3B:
            return out
        if c == 0x21:
            j = i + 2
            if b[i + 1] == 0xF9:
                out.append(("gce", j + 2, 0))
            while b[j]:
                j += b[j] + 1
            i = j + 1
        elif c == 0x2C:
            j = i + 10
            if b[i + 9] & 0x80:
                k = 1 << ((b[i + 9] & 7) + 1)
                out.append(("table", j, k))
                j += 3 * k
            j += 1
            while b[j]:
                j += b[j] + 1
            i = j + 1
        else:
            fail(f"a GIF block {c:#x} at {i}")
    fail("a GIF with no trailer")


def encode(gifski, d, m, names, half):
    """gifski's GIF of the frames, played once: the page a fixed colour of
    its palette, every entry within `PALETTE_SNAP` of it set to it, and
    each frame's delay the film's. gifski at 50 frames a second merges a
    run of equal frames into one of their summed length, which the delays
    are read back through."""
    out = os.path.join(d, "logo.gif")
    fixed = "%02X%02X%02X" % PAGE[half]
    cmd = [gifski, "--quiet", "--fps", "50", "--quality", str(QUALITY), "--width", str(SIZE[0])]
    cmd += ["--motion-quality", str(MOTION), "--lossy-quality", str(LOSSY), "--fixed-color", fixed]
    subprocess.run(cmd + ["--repeat", "-1", "-o", out, *names], check=True)
    b = bytearray(open(out, "rb").read())
    starts = [0]
    for cs in m["delays_cs"]:
        starts.append(starts[-1] + cs)
    k = 0
    page = PAGE[half]
    for kind, o, entries in gif_blocks(b):
        if kind == "table":
            for e in range(entries):
                c = b[o + 3 * e : o + 3 * e + 3]
                if max(abs(c[j] - page[j]) for j in range(3)) <= PALETTE_SNAP:
                    b[o + 3 * e : o + 3 * e + 3] = bytes(page)
            continue
        span = struct.unpack("<H", b[o : o + 2])[0] // 2
        b[o : o + 2] = struct.pack("<H", starts[k + span] - starts[k])
        k += span
    if k != len(m["delays_cs"]):
        fail(f"{half}: gifski's frames span {k} of the film's {len(m['delays_cs'])}")
    return bytes(b)


def decoded(b):
    """The GIF's frames as RGB arrays with their delays in hundredths,
    whether it loops, and its size."""
    import numpy
    from PIL import Image, ImageSequence

    im = Image.open(io.BytesIO(b))
    loops = "loop" in im.info
    frames = [(numpy.asarray(f.convert("RGB")), round(f.info["duration"] / 10)) for f in ImageSequence.Iterator(im)]
    return frames, loops, im.size


def haze(rendered, got, page, delays):
    """Of the pixels the film drew as the page, the share the decoded GIF
    shows off it, and the worst level; a decoded frame stands for each of
    the film's frames it merged."""
    import numpy

    pg = numpy.array(page, numpy.int16)
    n = off = worst = i = 0
    for g, cs in got:
        dv = numpy.abs(g.astype(numpy.int16) - pg).max(axis=2)
        span = 0
        while span < cs:
            at = (rendered[i].astype(numpy.int16) == pg).all(axis=2)
            o = at & (dv >= 1)
            n += int(at.sum())
            off += int(o.sum())
            if o.any():
                worst = max(worst, int(dv[o].max()))
            span += delays[i]
            i += 1
    return {"share_pct": round(100.0 * off / max(n, 1), 4), "worst_levels": worst}


def contrast(frames, bare, m, half):
    """The word's worst and median contrast over `frames`: the mean
    luminance of the pixels the bare logo draws in the word's own ink
    against the ring 2 to 7 pixels round its ink, WCAG's (L1 + 0.05) /
    (L2 + 0.05)."""
    import numpy

    x0, y0, x1, y1 = (int(v) for v in m["word"])
    region = bare[y0:y1, x0:x1].astype(numpy.int16)
    page = numpy.array(PAGE[half], numpy.int16)
    ink = numpy.abs(region - page).max(axis=2) > 0
    solid = (region == numpy.array(WORD[half], numpy.int16)).all(axis=2)

    def grown(mask, r):
        h, w = mask.shape
        pad = numpy.pad(mask, r)
        out = numpy.zeros_like(mask)
        for dy in range(2 * r + 1):
            for dx in range(2 * r + 1):
                out |= pad[dy : dy + h, dx : dx + w]
        return out

    ring = grown(ink, 7) & ~grown(ink, 2)

    def lum(rgb):
        c = rgb.astype(numpy.float64) / 255.0
        lin = numpy.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)
        return lin[..., 0] * 0.2126 + lin[..., 1] * 0.7152 + lin[..., 2] * 0.0722

    rs = []
    for f in frames:
        lit = lum(f[y0:y1, x0:x1])
        a, b = float(lit[solid].mean()), float(lit[ring].mean())
        rs.append((max(a, b) + 0.05) / (min(a, b) + 0.05))
    rs.sort()
    return round(rs[0], 2), round(rs[len(rs) // 2], 2)


# ---------------------------------------------------------------- the SVG


def svg(frames, m):
    """The SVG: the bare logo, the film's last frame, as a PNG, and each
    frame that differs from it over it as a WebP of the box it changes and
    two pixels round, its unchanged pixels clear, shown from its start to
    the next frame's by SMIL."""
    import numpy
    from PIL import Image

    base = frames[-1]
    w, h = SIZE

    def b64(data):
        return base64.b64encode(data).decode("ascii")

    def secs(cs):
        return f"{cs // 100}.{cs % 100:02d}s"

    png = io.BytesIO()
    Image.fromarray(base).save(png, format="PNG", optimize=True)
    starts = [0]
    for cs in m["delays_cs"]:
        starts.append(starts[-1] + cs)
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">',
        "<title>baud</title>",
        f'<image width="{w}" height="{h}" href="data:image/png;base64,{b64(png.getvalue())}"/>',
        "<g>",
    ]
    for k, f in enumerate(frames[:-1]):
        changed = (f != base).any(axis=2)
        if not changed.any():
            continue
        ys, xs = numpy.nonzero(changed)
        x0, y0 = max(int(xs.min()) - 2, 0), max(int(ys.min()) - 2, 0)
        x1, y1 = min(int(xs.max()) + 3, w), min(int(ys.max()) + 3, h)
        alpha = numpy.where(changed[y0:y1, x0:x1], 255, 0).astype(numpy.uint8)
        rgba = numpy.dstack([f[y0:y1, x0:x1], alpha])
        webp = io.BytesIO()
        Image.fromarray(rgba, "RGBA").save(webp, format="WEBP", quality=WEBP_QUALITY, alpha_quality=100, method=6)
        out.append(
            f'<image x="{x0}" y="{y0}" width="{x1 - x0}" height="{y1 - y0}" visibility="hidden"'
            f' href="data:image/webp;base64,{b64(webp.getvalue())}">'
            f'<set attributeName="visibility" to="visible" begin="{secs(starts[k])}"/>'
            f'<set attributeName="visibility" to="hidden" begin="{secs(starts[k + 1])}"/></image>'
        )
    out += ["</g>", "</svg>", ""]
    return "\n".join(out).encode("utf-8")


FRAME = re.compile(
    r'<image x="(\d+)" y="(\d+)" width="\d+" height="\d+" visibility="hidden"'
    r' href="data:image/webp;base64,([^"]+)">'
    r'<set attributeName="visibility" to="visible" begin="(\d+)\.(\d\d)s"/>'
    r'<set attributeName="visibility" to="hidden" begin="(\d+)\.(\d\d)s"/></image>'
)
BASE = re.compile(r'<image width="\d+" height="\d+" href="data:image/png;base64,([^"]+)"/>')


def svg_frames(b, m):
    """The SVG's picture at each of the film's frame times as a browser
    composes it: the base, and over it the frame image shown then."""
    import numpy
    from PIL import Image

    text = b.decode("utf-8")
    base = BASE.search(text)
    ground = numpy.asarray(Image.open(io.BytesIO(base64.b64decode(base.group(1)))).convert("RGB")).astype(numpy.float64)
    shown = {}
    for x, y, data, s0, c0, s1, c1 in FRAME.findall(text):
        shown[int(s0) * 100 + int(c0)] = (int(x), int(y), data, int(s1) * 100 + int(c1))
    out, at = [], 0
    for cs in m["delays_cs"]:
        f = ground.copy()
        if at in shown:
            x, y, data, end = shown.pop(at)
            if end != at + cs:
                fail(f"the SVG's frame at {at} cs ends at {end}, not {at + cs}")
            im = numpy.asarray(Image.open(io.BytesIO(base64.b64decode(data))).convert("RGBA")).astype(numpy.float64)
            a = im[..., 3:4] / 255.0
            hh, ww = im.shape[:2]
            f[y : y + hh, x : x + ww] = im[..., :3] * a + f[y : y + hh, x : x + ww] * (1 - a)
        out.append(numpy.round(f).astype(numpy.uint8))
        at += cs
    if shown:
        fail(f"the SVG shows frames at no frame's start: {sorted(shown)}")
    return out


def census(frames, delays, d, name):
    """WCAG 2.3.1's census of `frames`, each held its hundredths at 100
    samples a second and 2 pixels a point, the last 3 s more: the
    flash_census example's report."""
    raw, reps = os.path.join(d, f"{name}.rgb"), os.path.join(d, f"{name}.repeats")
    with open(raw, "wb") as f:
        for fr in frames:
            f.write(fr.tobytes())
    open(reps, "w").write("".join(f"{cs}\n" for cs in delays))
    cmd = ["cargo", "run", "--quiet", "--locked", *FAST, "-p", "baud-bench", "--example", "flash_census", "--"]
    cmd += [raw, reps, str(SIZE[0]), str(SIZE[1]), "1", "300", "100", "2"]
    r = json.loads(subprocess.run(cmd, check=True, cwd=ROOT, stdout=subprocess.PIPE, text=True).stdout)
    os.remove(raw)
    return r


# ---------------------------------------------------------------- the run


def make(gifski, half, tmp, geom, blink):
    """One half's GIF and SVG bytes, checked, and the lines that report
    them."""
    from PIL import Image

    d = os.path.join(tmp, half)
    prims, report = strands(geom, half, blink, d)
    m = film(half, d, prims, geom)
    frames = snapped(d, m, half)
    names = []
    for i, f in enumerate(frames):
        p = os.path.join(d, f"{i:05d}.png")
        Image.fromarray(f).save(p, compress_level=1)
        names.append(p)
    b = encode(gifski, d, m, names, half)
    if encode(gifski, d, m, names, half) != b:
        fail(f"{half}: gifski wrote different bytes from the same frames")
    got, loops, size = decoded(b)
    if sum(cs for _, cs in got) != sum(m["delays_cs"]):
        fail(f"{half}: the GIF lasts {sum(cs for _, cs in got)} cs, not the film's {sum(m['delays_cs'])}")
    if loops or size != SIZE or len(b) >= LIMIT:
        fail(f"{half}: loops {loops}, {size[0]}x{size[1]}, {len(b)} bytes")
    s = svg(frames, m)
    if svg(frames, m) != s:
        fail(f"{half}: the SVG's encoder wrote different bytes from the same frames")
    seen = svg_frames(s, m)
    hz = haze(frames, got, PAGE[half], m["delays_cs"])
    worst, median = contrast([f for f, _ in got] + seen, frames[-1], m, half)
    if worst < CONTRAST:
        fail(f"{half}: the word's contrast falls to {worst} to 1")
    c = census(seen, m["delays_cs"], d, "svg")
    if c["outcome"] != "passes" or c["strict_outcome"] != "passes":
        fail(f"{half}: the SVG's census fails: {c}")
    motion = sum(m["delays_cs"][:-1]) * 10
    lines = [
        f"{OUT[half][0]}: {len(b)} bytes, {len(got)} frames on {m['page']}, played once, {motion} ms of motion"
        f" ({m['cycles']} cycles of {m['on_ms']} ms shown and {m['off_ms']} hidden), then the logo, the block shown",
        f"{OUT[half][1]}: {len(s)} bytes, {s.count(b'<set ') // 2} frame images over the logo, at the GIF's times",
        f"  the plasma: seed {report['seed']}, from {report['start_cs'] / 100:.2f} s of its loop,"
        f" {report['events']} events between the fades; {m['most_layers']} layers at most a frame",
        f"  haze: {hz['share_pct']}% of the page's pixels off it in the GIF, by {hz['worst_levels']} levels at worst",
        f"  the word's contrast: worst {worst} to 1, median {median}",
        f"  the SVG's census: general {c['general_flashes']}, red {c['red_flashes']} flashes at a pixel in a second;"
        f" area {c['area']}, strict {c['strict']} of {c['threshold']}: {c['outcome']}, strict {c['strict_outcome']}",
    ]
    return b, s, lines


def tests():
    """The tests that hold the files in the tree, their output shown."""
    for t in TESTS:
        cmd = ["cargo", "test", "--quiet", "--locked", *FAST, *t]
        if "--" not in cmd:
            cmd.append("--")
        cmd += ["--nocapture", "--test-threads", "1"]
        r = subprocess.run(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        for line in r.stdout.splitlines():
            if line.startswith("../assets/readme/") or line.startswith("test "):
                print(line)
        if r.returncode:
            print(r.stdout)
            fail(f"the test {' '.join(t)} fails")


def main(argv):
    if argv[:1] != ["--pinned"]:
        outer(argv)
    gifski, args = argv[1], argv[2:]
    if args not in ([], ["--check"]):
        fail("usage: make-readme-logo.py [--check]")
    check = args == ["--check"]
    tool_versions(gifski)
    bad = []
    with tempfile.TemporaryDirectory() as tmp:
        today, blink = {}, {}
        for half in ("dark", "light"):
            os.makedirs(os.path.join(tmp, half))
            today[half], blink[half] = bare(half, os.path.join(tmp, half))
        geom = walls_hold(today["dark"], tmp)
        for half in ("dark", "light"):
            gif, svg_bytes, lines = make(gifski, half, tmp, geom, blink[half])
            for line in lines:
                print(line)
            for path, b in zip(OUT[half], (gif, svg_bytes)):
                at = os.path.join(ROOT, path)
                if check:
                    same = os.path.exists(at) and open(at, "rb").read() == b
                    print(f"{'same   ' if same else 'DIFFERS'} {path}")
                    if not same:
                        bad.append(f"{path} is not what the film and encoders make")
                else:
                    open(at, "wb").write(b)
                    print(f"wrote {path}")
    tests()
    for line in bad:
        print(f"FAIL {line}")
    if bad:
        sys.exit(1)
    print("ok: the logos are the film's, byte for byte" if check else "ok")


if __name__ == "__main__":
    main(sys.argv[1:])
