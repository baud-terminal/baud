#!/usr/bin/env python3
"""check-publish.py's package() verifies each crate as it is now.

    python3 scripts/test-publish-fresh.py [path/to/check-publish.py]

Cargo takes a crate of one name and version to be immutable, so a verify
that reuses an earlier run's unpack or compile of a crate builds the
crates after it against that crate's old code (package() says where
cargo keeps each). This packages a workspace of two crates, `probe-b`
depending on `probe-a` by path and version, three times, the version
never bumped:

  1. a has one(), and b calls a::one(): it builds;
  2. a gains two(), and b calls a::two(): it builds, where an a from
     step 1 has no two() and fails it;
  3. a loses one(), and b calls a::one(): it fails, on one(), where an a
     from step 1 has one() and builds.

Each step must also have read a from an unpack folder of its own, and
left neither that unpack nor the folder behind. Neither crate depends on
anything outside the workspace, so it runs with CARGO_NET_OFFLINE and
asks crates.io nothing. Exit 0 when every check holds, 1 otherwise.
"""

import importlib.util
import os
import sys
import tempfile

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
VERSION = "0.1.0"
A, B = "check-publish-fresh-probe-a", "check-publish-fresh-probe-b"

STEPS = [
    # (a's lib.rs, b's lib.rs, None when it must build, else what the
    # failure must name)
    ("pub fn one() -> u8 {\n    1\n}\n", "one", None),
    ("pub fn one() -> u8 {\n    1\n}\n\npub fn two() -> u8 {\n    2\n}\n", "two", None),
    ("pub fn two() -> u8 {\n    2\n}\n", "one", "cannot find function `one`"),
]


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def manifest(name, deps=""):
    return (
        f'[package]\nname = "{name}"\nversion = "{VERSION}"\nedition = "2024"\n'
        f'license = "MIT"\ndescription = "A probe of check-publish.py."\n{deps}'
    )


def main():
    path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "scripts", "check-publish.py")
    spec = importlib.util.spec_from_file_location("check_publish", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    os.environ["CARGO_NET_OFFLINE"] = "true"
    bad = 0
    seen = set()
    with tempfile.TemporaryDirectory(prefix="publish-fresh-") as ws:
        write(os.path.join(ws, "Cargo.toml"), '[workspace]\nmembers = ["a", "b"]\nresolver = "3"\n')
        write(os.path.join(ws, "a", "Cargo.toml"), manifest(A))
        write(
            os.path.join(ws, "b", "Cargo.toml"),
            manifest(B, f'\n[dependencies]\n{A} = {{ path = "../a", version = "{VERSION}" }}\n'),
        )
        for i, (a, call, want) in enumerate(STEPS, 1):
            write(os.path.join(ws, "a", "src", "lib.rs"), a)
            crate = A.replace("-", "_")
            write(os.path.join(ws, "b", "src", "lib.rs"), f"pub fn f() -> u8 {{\n    {crate}::{call}()\n}}\n")
            errs, _, read = mod.package(ws, [A, B], VERSION, True)
            said = "\n".join(errs)
            checks = [
                (
                    "builds" if want is None else f"fails on {want}",
                    not errs if want is None else want in said,
                    said[-600:] or "it built",
                ),
                (
                    "read a from one unpack",
                    [os.path.basename(r) for r in read] == [f"{A}-{VERSION}"],
                    sorted(read),
                ),
                (
                    "an unpack folder no earlier step used",
                    bool(read) and not {os.path.dirname(r) for r in read} & seen,
                    sorted(read),
                ),
                (
                    "left no unpack and no folder behind",
                    not any(os.path.exists(r) or os.path.exists(os.path.dirname(r)) for r in read),
                    sorted(read),
                ),
            ]
            seen |= {os.path.dirname(r) for r in read}
            for what, ok, detail in checks:
                print(f"{'ok' if ok else 'FAIL'} step {i}: {what}" + ("" if ok else f": {detail}"))
                bad += not ok
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
