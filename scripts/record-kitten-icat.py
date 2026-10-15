#!/usr/bin/env python3
"""Record what `kitten icat` writes to a terminal, through a real PTY.

The kitty graphics stream is `APC G <keys> ; <base64> ST`. `kitten icat`
chooses what to send from the window's size in pixels, which it reads from
the PTY's `winsize` (`ws_xpixel`, `ws_ypixel`), so the PTY here carries the
pixels baud's app reports: `cols * 17` by `rows * 34`, the default 13 pt cell
at 2x. `--transfer-mode=stream` is the medium that needs no shared file
(`t=d`, direct), and with it and no `--detect-support` kitten sends no query,
so nothing has to answer it.

kitten's default, `--transfer-mode=detect`, queries first: three `a=q`, one
per medium, then the Primary DA, and it waits up to ten seconds for the DA.
The `detect-*` cases answer as baud would: baud's DA is `CSI ? 6 c`, or
`CSI ? 6 ; 4 c` with `[images] advertise_sixel`, and baud answers no `a=q`
today. `detect-direct` also answers `OK` to the direct query alone, which is
what a baud implementing `t=d` and no other medium would send.

kitty and the input pictures are pinned by nixpkgs revision, and every input
is checked against its sha256 before it is used; a case whose input is a macOS
system file is skipped where that file differs or is missing.

`--scan ROOT...` records nothing and counts instead: every PNG under the
roots, by its `IHDR`, that kitten would send as-is at each window, and how
many of those are interlaced (Adam7), by count and by bytes. kitten's
`--fit` defaults to `width`, so that is every PNG no wider than the window,
whatever its height; it scales a wider one and sends it raw, which cannot
carry Adam7. A multi-frame PNG is counted as sent as-is, which it is not.

Usage:
    python3 scripts/record-kitten-icat.py --out DIR
    python3 scripts/record-kitten-icat.py --scan /System/Library /Library /Applications
"""

from __future__ import annotations

import argparse
import fcntl
import hashlib
import os
import pty
import re
import selectors
import struct
import subprocess
import sys
import termios
import time

NIXPKGS = "github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a"
CELL = (17, 34)  # baud-app's default cell, device pixels: 13 pt at 2x
WINDOWS = {
    # The smallest grid at that cell whose text area holds 1920x1080.
    "113x32": (113, 32),
    "80x24": (80, 24),
    # 1920 wide and 816 high: kitten fits to width alone, so a 1920x1080
    # picture still goes whole.
    "113x24": (113, 24),
    # The smallest that holds 3840x2160: a full-screen window on a 4K panel.
    "226x64": (226, 64),
}
INPUTS = {
    # nixos-artwork, CC-BY-SA-4.0: 1920x1080 RGB, full-window at 113x32.
    "mosaic-blue": (
        "nix",
        "nixos-artwork.wallpapers.mosaic-blue",
        "share/wallpapers/mosaic-blue/contents/images/1920x1080.png",
        "c596cd2bcb37fe8a11bf27871b185c6279e27de1808809d48f0f448c94d66cb1",
    ),
    # nixos-artwork, CC-BY-SA-4.0: 3840x2160 RGBA, wider than all but 226x64.
    "waterfall": (
        "nix",
        "nixos-artwork.wallpapers.waterfall",
        "share/wallpapers/waterfall/contents/images/3840x2160.png",
        "50b14d5193d4f648431baaed90cb242dee6c62951cac956f705bc4929bd78cc3",
    ),
    # macOS 27.0: 1160x580 RGBA, interlace method 1 (Adam7); narrower than
    # every window.
    "skyglow-adam7": (
        "file",
        None,
        "/System/Library/PrivateFrameworks/GeoKit.framework/Versions/A/"
        "Resources/UTC-08.00-Skyglow@2x.png",
        "e035d914fa659efae67108035fa97d31b756fc71cce807e7844d475702e74d7f",
    ),
}


def sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def nix_out(attr: str) -> str:
    return subprocess.run(
        ["nix", "build", "--no-link", "--print-out-paths", f"{NIXPKGS}#{attr}"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


QUERY = re.compile(rb"\x1b_G([^;\x1b]*a=q[^;\x1b]*);")
MODES = {
    # mode: (argv, Primary DA reply or None, answer the direct query)
    "stream": (["--transfer-mode=stream"], None, False),
    "detect-da6": ([], b"\x1b[?6c", False),
    "detect-da64": ([], b"\x1b[?6;4c", False),
    "detect-direct": ([], b"\x1b[?6;4c", True),
}


def reply(out: bytes, da: bytes, direct: bool) -> bytes:
    """The answers to kitten's detection burst, once its DA has been sent."""
    ans = b""
    for keys in QUERY.findall(out):
        kv = dict(k.split(b"=", 1) for k in keys.split(b","))
        if direct and kv.get(b"t", b"d") == b"d":
            ans += b"\x1b_Gi=" + kv[b"i"] + b";OK\x1b\\"
    return ans + da


def record(kitten: str, mode: str, image: str, cols: int, rows: int, timeout: float) -> tuple[bytes, int, float]:
    argv, da, direct = MODES[mode]
    pid, master = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm-256color"
        os.execv(kitten, [kitten, "icat", *argv, image])
    xpixel, ypixel = cols * CELL[0], rows * CELL[1]
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, xpixel, ypixel))
    sel = selectors.DefaultSelector()
    sel.register(master, selectors.EVENT_READ)
    out = bytearray()
    answered = da is None
    start = time.monotonic()
    end = start + timeout
    while time.monotonic() < end:
        if not sel.select(timeout=0.2):
            continue
        try:
            chunk = os.read(master, 65536)
        except OSError:
            chunk = b""
        if not chunk:
            break
        out += chunk
        if not answered and b"\x1b[c" in out:
            os.write(master, reply(bytes(out), da, direct))
            answered = True
    else:
        os.kill(pid, 9)
        raise SystemExit(f"kitten icat {image} did not finish in {timeout}s")
    _, status = os.waitpid(pid, 0)
    os.close(master)
    return bytes(out), os.waitstatus_to_exitcode(status), time.monotonic() - start


APC = re.compile(rb"\x1b_G([^;\x1b]*)(?:;([^\x1b]*))?\x1b\\")


def summary(raw: bytes) -> str:
    cmds = APC.findall(raw)
    if not cmds:
        return "no APC G command"
    first = cmds[0][0].decode()
    payload = sum(len(p) for _, p in cmds)
    chunks = [len(p) for _, p in cmds]
    return f"{len(cmds)} commands, first keys {first}, base64 {payload} bytes, largest chunk {max(chunks)}"


def scan(roots: list[str]) -> int:
    seen, pngs = set(), []
    for root in roots:
        for dp, _, fn in os.walk(root):
            for f in fn:
                if not f.lower().endswith(".png"):
                    continue
                p = os.path.join(dp, f)
                try:
                    st = os.stat(p)
                    if (st.st_dev, st.st_ino) in seen:
                        continue
                    seen.add((st.st_dev, st.st_ino))
                    with open(p, "rb") as h:
                        head = h.read(29)
                except OSError:
                    continue
                if head[:8] != b"\x89PNG\r\n\x1a\n" or head[12:16] != b"IHDR":
                    continue
                w, h_, _, _, _, _, interlace = struct.unpack(">IIBBBBB", head[16:29])
                pngs.append((w, h_, interlace, st.st_size))
    print(f"{len(pngs)} PNGs, {sum(p[2] == 1 for p in pngs)} interlaced")
    for window, (cols, rows) in WINDOWS.items():
        xp = cols * CELL[0]
        fit = [p for p in pngs if p[0] <= xp]
        adam7 = [p for p in fit if p[2] == 1]
        n, b = len(fit), sum(p[3] for p in fit)
        n7, b7 = len(adam7), sum(p[3] for p in adam7)
        print(
            f"{window} ({xp} wide): {n} sent as-is, {b} bytes; Adam7 {n7} ({100 * n7 / n:.2f}%),"
            f" {b7} bytes ({100 * b7 / b:.2f}%); {len(pngs) - n} sent raw"
        )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out")
    ap.add_argument("--scan", nargs="+")
    ap.add_argument("--timeout", type=float, default=60.0)
    args = ap.parse_args()
    if args.scan:
        return scan(args.scan)
    if not args.out:
        ap.error("--out or --scan")
    os.makedirs(args.out, exist_ok=True)

    kitty = nix_out("kitty")
    kitten = os.path.join(kitty, "bin", "kitten")
    version = subprocess.run([kitten, "--version"], check=True, capture_output=True, text=True).stdout
    print(version.strip())

    paths = {}
    for name, (kind, attr, rel, want) in INPUTS.items():
        path = os.path.join(nix_out(attr), rel) if kind == "nix" else rel
        if not os.path.exists(path):
            print(f"{name}: skipped, {path} is missing")
            continue
        got = sha256(path)
        print(f"{name}: {path}\n  input sha256 {got}")
        if got != want:
            if kind == "nix":
                print(f"  expected {want}")
                return 1
            print("  skipped: not the file the recordings were made from")
            continue
        paths[name] = path
    cases = [(n, w, "stream") for n in paths for w in WINDOWS]
    if "mosaic-blue" in paths:
        cases += [("mosaic-blue", "113x32", m) for m in MODES if m != "stream"]
    for name, window, mode in cases:
        cols, rows = WINDOWS[window]
        raw, code, secs = record(kitten, mode, paths[name], cols, rows, args.timeout)
        dest = os.path.join(args.out, f"{name}-{window}-{mode}.raw")
        with open(dest, "wb") as f:
            f.write(raw)
        print(f"{name} {window} {mode}: exit {code} after {secs:.1f} s, {len(raw)} bytes")
        print(f"  sha256 {hashlib.sha256(raw).hexdigest()}")
        print(f"  {summary(raw)}")
        if code != 0:
            print(f"  ends {raw[-160:]!r}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
