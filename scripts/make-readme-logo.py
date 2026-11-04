#!/usr/bin/env python3
"""Film the README's header logo with baud's own renderer and encode it.

    python3 scripts/make-readme-logo.py          write assets/readme/logo-dark.gif
                                                 and logo-light.gif
    python3 scripts/make-readme-logo.py --check  film and encode both again in
                                                 memory and fail unless each
                                                 equals the committed file byte
                                                 for byte; writes no tracked file

**What it shows**: the lockup, `baud ❯ █`, as the launch splash holds it
and the logo files draw it, its block blinking as baud's cursor blinks by
default -- the app's own caret, asked for each phase -- for the most whole
cycles that end before five seconds, then resting on the block shown. At
the default of 250 ms shown and 250 hidden that is nine cycles, 4.5 s, in
nineteen frames. baud-app/src/logo_film.rs says how each frame is drawn.

**Why it plays once and stops**: WCAG 2.2's 2.2.2 Pause, Stop, Hide
(Level A) asks that blinking which starts by itself, lasts more than five
seconds and is shown beside other content can be paused, stopped or
hidden ([[source: wcag22]]). A README carries no control of its own, so
the logo blinks for under five seconds and rests.

**How it is made**: each half is filmed by baud-app's logo film, a test
run from here in release's optimisation without LTO, on GitHub's page
colour: #0d1117 for the dark theme, #ffffff for the light. It is a test
and not an example because the default blink is the app's, and the app is
a binary whose modules no example reaches. Each frame becomes a PNG
(Pillow), the PNGs become a GIF with gifski at readme-film.sh's quality,
80, and its pin, played once, and each GIF frame's delay is then set to
its phase's length in hundredths, which gifski's frame rate cannot say
for phases of two lengths.

**What it checks before it writes**, each from the files it made:
  - gifski, run twice on the same frames, writes the same bytes;
  - the GIF decodes to the film's frames, each frame's delay its phase's
    length, 840 by 580, no loop, under 1 MB.
Then, on the files in the tree -- written, or with --check the committed
ones -- it runs the two tests that hold them, and prints what they read:
  - baud-app's logo_film::tests::the_readme_logos_blink_the_default_and_rest_shown:
    shown and hidden read off the block's pixels, each run the default
    phase's length, under five seconds, the last frame shown;
  - baud-bench's readme_logo_flashes: WCAG 2.3.1's census on the decoded
    frames at 100 a second and 2 pixels a point, the last held 3 s.

**The environment is pinned whole.** Python with Pillow and NumPy comes
from make-brand-pictures.py's nixpkgs revision, fetched by hash, and
gifski from readme-film.sh's; the script runs itself again under that
Python with -I and refuses other versions. The frames come from the
repository's renderer, built by the cargo on PATH. Measured on
aarch64-darwin only, which is why --check runs by hand and not in CI.
"""

import io
import json
import os
import struct
import subprocess
import sys
import tempfile

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

NIXPKGS_REV = "7dd199b0e2993e37b4775ed66b1c291699608c9f"
NIXPKGS_SHA256 = "08kxnni9l87i089hy7p9a7yr64w6h9x5m2cc7k2hn455hidm9mm3"
GIFSKI_NIXPKGS = "github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a"
TOOLS = {"python": "3.14.7", "Pillow": "12.3.0", "numpy": "2.5.2", "gifski": "gifski 1.34.0"}
QUALITY = 80

OUT = {"dark": "assets/readme/logo-dark.gif", "light": "assets/readme/logo-light.gif"}
SIZE = (840, 580)
LIMIT = 1_000_000
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
    ["-p", "baud", "--bin", "baud", "--", "logo_film::tests::the_readme_logos_blink_the_default_and_rest_shown"],
    ["-p", "baud-bench", "--test", "readme_logo_flashes"],
]


def fail(msg):
    raise SystemExit(f"make-readme-logo: {msg}")


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

    have = {
        "python": "%d.%d.%d" % sys.version_info[:3],
        "Pillow": PIL.__version__,
        "numpy": numpy.__version__,
        "gifski": subprocess.run([gifski, "--version"], check=True, stdout=subprocess.PIPE, text=True).stdout.strip(),
    }
    if have != TOOLS:
        fail(f"the tools are not the pinned ones: {have}, not {TOOLS}")


# ---------------------------------------------------------------- the film


def film(half, d):
    """Film one half into `d`; its report."""
    cmd = ["cargo", "test", "--quiet", "--locked", "-p", "baud", "--bin", "baud", *FAST]
    cmd += ["logo_film::film_the_readme_logo", "--", "--ignored", "--exact"]
    env = dict(os.environ, BAUD_LOGO_OUT=d, BAUD_LOGO_HALF=half)
    subprocess.run(cmd, check=True, cwd=ROOT, env=env, stdout=subprocess.DEVNULL)
    m = json.load(open(os.path.join(d, "film.json")))
    if (m["width"], m["height"]) != SIZE or m["half"] != half:
        fail(f"{half}: the film is {m['width']}x{m['height']} of {m['half']}")
    if m["frames"] != len(m["delays_cs"]):
        fail(f"{half}: {m['frames']} frames and {len(m['delays_cs'])} delays")
    return m


def pngs(d, m):
    """The film's frames, each written as a PNG; their paths."""
    from PIL import Image

    n = m["width"] * m["height"] * 3
    out = os.path.join(d, "png")
    os.makedirs(out)
    names = []
    with open(os.path.join(d, "frames.rgb"), "rb") as f:
        for i in range(m["frames"]):
            p = os.path.join(out, f"{i:05d}.png")
            Image.frombytes("RGB", SIZE, f.read(n)).save(p, compress_level=1)
            names.append(p)
        if f.read(1):
            fail("frames.rgb holds more than the film's frames")
    return names


# ---------------------------------------------------------------- the GIF


def gif_delays(b):
    """Offsets of each Graphic Control Extension's delay field, in order."""
    i, flags, out = 13, b[10], []
    if flags & 0x80:
        i += 3 * (1 << ((flags & 7) + 1))
    while i < len(b):
        c = b[i]
        if c == 0x3B:
            return out
        if c == 0x21:
            j = i + 2
            if b[i + 1] == 0xF9:
                out.append(j + 2)
            while b[j]:
                j += b[j] + 1
            i = j + 1
        elif c == 0x2C:
            j = i + 10
            if b[i + 9] & 0x80:
                j += 3 * (1 << ((b[i + 9] & 7) + 1))
            j += 1
            while b[j]:
                j += b[j] + 1
            i = j + 1
        else:
            fail(f"a GIF block {c:#x} at {i}")
    fail("a GIF with no trailer")


def encode(gifski, d, m, names):
    """gifski's GIF of the frames, played once, each frame's delay its
    phase's length in hundredths."""
    out = os.path.join(d, "logo.gif")
    cmd = [gifski, "--quiet", "--fps", "4", "--quality", str(QUALITY), "--width", str(SIZE[0])]
    subprocess.run(cmd + ["--repeat", "-1", "-o", out, *names], check=True)
    b = bytearray(open(out, "rb").read())
    at = gif_delays(b)
    if len(at) != len(m["delays_cs"]):
        fail(f"gifski wrote {len(at)} frames of the film's {len(m['delays_cs'])}")
    for o, cs in zip(at, m["delays_cs"]):
        b[o : o + 2] = struct.pack("<H", cs)
    return bytes(b)


def decoded(b):
    """The GIF's delays in hundredths, whether it loops, and its size."""
    from PIL import Image, ImageSequence

    im = Image.open(io.BytesIO(b))
    loops = "loop" in im.info
    delays = [round(f.info["duration"] / 10) for f in ImageSequence.Iterator(im)]
    return delays, loops, im.size


# ---------------------------------------------------------------- the run


def make(gifski, half, tmp):
    """One half's GIF bytes, checked, and the line that reports it."""
    d = os.path.join(tmp, half)
    os.makedirs(d)
    m = film(half, d)
    names = pngs(d, m)
    b = encode(gifski, d, m, names)
    if encode(gifski, d, m, names) != b:
        fail(f"{half}: gifski wrote different bytes from the same frames")
    delays, loops, size = decoded(b)
    if delays != m["delays_cs"]:
        fail(f"{half}: the GIF's delays {delays} are not the film's {m['delays_cs']}")
    if loops or size != SIZE or len(b) >= LIMIT:
        fail(f"{half}: loops {loops}, {size[0]}x{size[1]}, {len(b)} bytes")
    blink = sum(delays[:-1]) * 10
    line = (
        f"{OUT[half]}: {len(b)} bytes, {len(delays)} frames on {m['page']}, played once;"
        f" {m['cycles']} cycles of {m['on_ms']} ms shown and {m['off_ms']} hidden, {blink} ms,"
        f" then shown and still"
    )
    return b, line


def tests():
    """The two tests that hold the files in the tree, their output shown."""
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
        for half in ("dark", "light"):
            b, line = make(gifski, half, tmp)
            print(line)
            at = os.path.join(ROOT, OUT[half])
            if check:
                same = os.path.exists(at) and open(at, "rb").read() == b
                print(f"{'same   ' if same else 'DIFFERS'} {OUT[half]}")
                if not same:
                    bad.append(f"{OUT[half]} is not what the film and encoder make")
            else:
                open(at, "wb").write(b)
                print(f"wrote {OUT[half]}")
    tests()
    for line in bad:
        print(f"FAIL {line}")
    if bad:
        sys.exit(1)
    print("ok: both logos are the film's, byte for byte" if check else "ok")


if __name__ == "__main__":
    main(sys.argv[1:])
