#!/usr/bin/env python3
"""What crates.io would receive, checked before anything is sent.

    python3 scripts/check-publish.py            the manifests and the file lists
    python3 scripts/check-publish.py --size     and each .crate's size, from
                                                `cargo package --no-verify`
    python3 scripts/check-publish.py --verify   and each crate built from its
                                                own .crate, against the others'

`cargo install baud` builds the `baud` package and every crate it depends on,
so each of those is published and nothing else is. The checks, each of which
has to be able to fail:

  - the published set is `baud` and the workspace crates it reaches through
    its dependencies, target-specific and optional ones included; every other
    member is in UNPUBLISHED and carries `publish = false`;
  - each published manifest has a description, a readme that exists, the
    repository, the workspace's licence, a rust-version, and keywords and
    categories crates.io accepts;
  - each dependency of a published crate on a workspace crate names the
    version being released as well as its path: cargo refuses to package one
    that does not, and drops the path from what it uploads;
  - each published crate's file list carries LICENSE, byte for byte the
    root's, and the notice every third-party file in it needs: for each
    `docs/research/third-party.toml` entry with a file in the list, a notice
    in LICENSES/ must be in the crate's own LICENSES/, byte for byte the
    root's, and a notice that is another file must be in the list;
  - with --size or --verify, every .crate is under crates.io's 10 MiB
    upload limit;
  - with --verify, every crate builds from its .crate alone, as `cargo
    publish` builds it before uploading: a file read through a path that
    leaves the crate's directory is not in the .crate, and fails here rather
    than partway through a release, after the crates before it are up.

A packaged symlink is a copy of the file it points at (cargo archives the
target's bytes), so it counts as that file for the notices, and it must point
at a file in the repository.
"""

import filecmp
import os
import re
import subprocess
import sys
import tomllib

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
SIDECAR = "docs/research/third-party.toml"
LICENCE = "CC-BY-NC-SA-4.0"
UNPUBLISHED = {"baud-bench", "baud-difftest", "baud-sfte-difftest"}
# crates.io's limit on an upload, in bytes.
LIMIT = 10 * 1024 * 1024
# The crates.io category slugs these manifests use, as
# https://crates.io/api/v1/category_slugs listed them on 2026-10-28. A slug
# outside the list is refused at upload.
CATEGORIES = {
    "command-line-utilities",
    "encoding",
    "graphics",
    "gui",
    "hardware-support",
    "no-std",
    "no-std::no-alloc",
    "os::linux-apis",
    "os::macos-apis",
    "parser-implementations",
    "rendering",
    "text-processing",
}
KEYWORD = re.compile(r"^[A-Za-z][A-Za-z0-9_+-]{0,19}$")
DEP_TABLES = ("dependencies", "build-dependencies")


def load(path):
    with open(os.path.join(ROOT, path), "rb") as f:
        return tomllib.load(f)


def members(ws):
    """Package name -> (directory, manifest) for every workspace member."""
    out = {}
    for d in ws["workspace"]["members"]:
        m = load(os.path.join(d, "Cargo.toml"))
        out[m["package"]["name"]] = (d, m)
    return out


def field(pkg, ws, key):
    v = pkg.get(key)
    if isinstance(v, dict) and v.get("workspace"):
        return ws["workspace"]["package"].get(key)
    return v


def deps(m):
    """(table, name, spec) for every dependency a published manifest keeps."""
    for t in DEP_TABLES:
        for n, s in m.get(t, {}).items():
            yield t, n, s
    for target, tt in m.get("target", {}).items():
        for t in DEP_TABLES:
            for n, s in tt.get(t, {}).items():
                yield f"target.{target}.{t}", n, s


def closure(root, mem):
    seen, todo = set(), [root]
    while todo:
        n = todo.pop()
        if n in seen:
            continue
        seen.add(n)
        for _, d, s in deps(mem[n][1]):
            name = s.get("package", d) if isinstance(s, dict) else d
            if name in mem:
                todo.append(name)
    return seen


def listed(name):
    out = subprocess.run(
        ["cargo", "package", "--list", "--allow-dirty", "-q", "-p", name],
        cwd=ROOT, capture_output=True, text=True,
    )
    if out.returncode != 0:
        return None, out.stderr.strip()
    return [l for l in out.stdout.splitlines() if l], None


def resolved(crate_dir, rel):
    """The repository path a packaged file's bytes come from."""
    return os.path.relpath(os.path.realpath(os.path.join(ROOT, crate_dir, rel)), ROOT)


def is_path(notice):
    return "/" in notice or notice == "LICENSE"


def check(size, verify):
    errs = []
    ws = load("Cargo.toml")
    version = ws["workspace"]["package"]["version"]
    wdeps = ws["workspace"].get("dependencies", {})
    mem = members(ws)
    if "baud" not in mem:
        return ["no workspace member is named `baud`: `cargo install baud` would find nothing"]
    want = closure("baud", mem)
    published = {n for n, (_, m) in mem.items() if m["package"].get("publish", True) is not False}
    if published != want:
        for n in sorted(want - published):
            errs.append(f"{n}: `baud` depends on it, so it must be published, and it says publish = false")
        for n in sorted(published - want):
            errs.append(f"{n}: published, and `baud` does not depend on it")
    if set(mem) - want != UNPUBLISHED:
        errs.append(f"unpublished members are {sorted(set(mem) - want)}, UNPUBLISHED says {sorted(UNPUBLISHED)}")
    if len(want) < 10:
        errs.append(f"only {len(want)} crates reached from `baud`: the dependency walk has stopped working")

    for n in sorted(want):
        d, m = mem[n]
        p = m["package"]
        for key in ("description", "repository", "rust-version"):
            if not field(p, ws, key):
                errs.append(f"{n}: no {key}")
        if field(p, ws, "license") != LICENCE:
            errs.append(f"{n}: license is {field(p, ws, 'license')!r}, not {LICENCE}")
        readme = field(p, ws, "readme")
        if not readme or not os.path.isfile(os.path.join(ROOT, d, readme)):
            errs.append(f"{n}: readme {readme!r} is not a file")
        kw = field(p, ws, "keywords") or []
        if not 1 <= len(kw) <= 5 or not all(KEYWORD.match(k) for k in kw):
            errs.append(f"{n}: keywords {kw} are not one to five crates.io keywords")
        cats = field(p, ws, "categories") or []
        if not 1 <= len(cats) <= 5 or not set(cats) <= CATEGORIES:
            errs.append(f"{n}: categories {cats} are not one to five of CATEGORIES")
        for t, dn, s in deps(m):
            name = s.get("package", dn) if isinstance(s, dict) else dn
            if name not in mem:
                continue
            spec = wdeps.get(dn, {}) if isinstance(s, dict) and s.get("workspace") else s
            if not isinstance(spec, dict) or "path" not in spec:
                errs.append(f"{n}: [{t}] {dn} is a workspace crate taken without a path")
                continue
            if spec.get("version") != version:
                errs.append(
                    f"{n}: [{t}] {dn} has version {spec.get('version')!r}, not {version!r}: "
                    "cargo refuses to package a path dependency with none"
                )

    side = load(SIDECAR)
    entries = [(f"[[tree]] {t['what'][:50]}", t["paths"], t["notice"]) for t in side["tree"]]
    entries += [
        (f"upstream {g}", [f"docs/research/sources/{f}" for f in e["files"]], e["notice"])
        for g, e in side["upstream"].items()
    ]
    for n in sorted(want):
        d, _ = mem[n]
        files, err = listed(n)
        if files is None:
            errs.append(f"{n}: cargo package --list failed: {err}")
            continue
        if "LICENSE" not in files:
            errs.append(f"{n}: LICENSE is not in the package")
        elif not filecmp.cmp(os.path.join(ROOT, d, "LICENSE"), os.path.join(ROOT, "LICENSE"), shallow=False):
            errs.append(f"{n}: its LICENSE is not the root's")
        src = {}
        for f in files:
            r = resolved(d, f)
            if os.path.islink(os.path.join(ROOT, d, f)) and (r.startswith("..") or not os.path.isfile(os.path.join(ROOT, r))):
                errs.append(f"{n}: {f} links to {r}, which is not a file in the repository")
            src.setdefault(r, f)
        for what, paths, notice in entries:
            hit = [p for p in paths if p in src]
            if not hit or not is_path(notice):
                continue
            if notice.startswith("LICENSES/"):
                local = os.path.join(d, notice)
                if notice not in files:
                    errs.append(f"{n}: ships {src[hit[0]]} ({what}) without {notice}")
                elif not filecmp.cmp(os.path.join(ROOT, local), os.path.join(ROOT, notice), shallow=False):
                    errs.append(f"{n}: {notice} is not the root's")
            elif notice != "LICENSE" and notice not in src:
                errs.append(f"{n}: ships {src[hit[0]]} ({what}) without its notice {notice}")

    if (size or verify) and not errs:
        target = os.environ.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target"))
        args = ["cargo", "package", "--allow-dirty", "-q"] + ([] if verify else ["--no-verify"])
        for n in sorted(want):
            args += ["-p", n]
        out = subprocess.run(args, cwd=ROOT, capture_output=True, text=True)
        if out.returncode != 0:
            errs.append(f"{' '.join(args[:2])} failed: {out.stderr.strip()[-2000:]}")
        else:
            for n in sorted(want):
                crate = os.path.join(target, "package", f"{n}-{version}.crate")
                b = os.path.getsize(crate)
                print(f"  {n}: {b / 1048576:.2f} MiB compressed")
                if b > LIMIT:
                    errs.append(f"{n}: {b} bytes, over crates.io's {LIMIT}")
    if not errs:
        print(f"ok: {len(want)} crates to publish, {len(set(mem) - want)} not")
    return errs


def main():
    errs = check("--size" in sys.argv[1:], "--verify" in sys.argv[1:])
    for e in errs:
        print(f"FAIL {e}")
    sys.exit(1 if errs else 0)


if __name__ == "__main__":
    main()
