#!/usr/bin/env python3
"""What crates.io would receive, checked before anything is sent.

    python3 scripts/check-publish.py            the manifests and the file lists
    python3 scripts/check-publish.py --size     and each .crate's size, from
                                                `cargo package --no-verify`
    python3 scripts/check-publish.py --verify   and each crate built from its
                                                own .crate, against the others'
    python3 scripts/check-publish.py --release  and no published crate ships a
                                                file whose licence is not settled
    python3 scripts/check-publish.py --write-notices
                                                write each crate's NOTICE, and
                                                check the rest

Exit 0 when everything passes, 1 on any FAIL, and 2 under --release when the
only thing left is a row awaiting the owner.

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
  - no published crate ships a file of a `docs/research/third-party.toml`
    entry whose status is one of UNSETTLED: without --release each is
    printed and the check still passes, since what is missing is a
    decision, not a change to the tree; with --release (the Release job,
    before anything is uploaded) each one refuses the release. A file with
    no entry at all is not seen here;
  - each published crate carries NOTICE, and it is what notice() writes
    from the crate's own file list: every entry of the sidecar with a file
    in the crate, the files by their paths in the crate, each one's
    licence, and where its notice is in the crate (--write-notices writes
    them);
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
import textwrap
import tomllib

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
SIDECAR = "docs/research/third-party.toml"
LICENCE = "CC-BY-NC-SA-4.0"
UNPUBLISHED = {"baud-bench", "baud-difftest", "baud-sfte-difftest"}
# Statuses that say the licence of a file is not settled. A file under one
# of these may sit in the repository, and must not go out in a published
# crate: a crates.io version cannot be taken back. `copyleft` is one,
# since keeping such a file or making it external is a choice still open;
# `nc` is not, since CC BY-NC-SA 3.0 allows the verbatim copy it is kept
# as.
UNSETTLED = {"open", "unverified", "no-grant", "copyleft"}
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


def gen_third_party():
    """scripts/gen-third-party.py as a module, for its status meanings and
    licence URIs, so NOTICE and THIRD_PARTY.md say the same thing."""
    import importlib.util

    spec = importlib.util.spec_from_file_location("gen_third_party", os.path.join(ROOT, "scripts", "gen-third-party.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def notice(name, entries, src, g):
    """The NOTICE of crate `name`: each sidecar entry with a file in the
    crate, by the paths the crate gives its files. `src` maps a repository
    path to the crate's path for it, both a file's own path and the one a
    link resolves to. Every [[tree]] entry with a file here is listed; a
    source group is left out when each of its files here is already listed
    under a [[tree]] entry, as the target of a link is."""
    wrap = lambda label, text: textwrap.fill(
        text, width=76, initial_indent=f"  {label:<9}", subsequent_indent=" " * 11, break_on_hyphens=False,
        break_long_words=False,
    )
    o = [
        f"Third-party material in the {name} crate",
        "",
        textwrap.fill(
            f"The licence of {name} is LICENSE. This file lists the material from other "
            "projects that files in this crate carry: for each, the files by their paths "
            "in this crate, the licence it keeps, and where the notice that licence asks "
            "for is. It is generated from docs/research/third-party.toml in baud's "
            "repository, whose THIRD_PARTY.md lists every such file in the repository.",
            width=76,
        ),
    ]
    shown = set()
    count = 0
    for what, paths, note, status, lic, origin, kind in entries:
        files = sorted({src[p] for p in paths if p in src})
        if not files or (kind == "source" and set(files) <= shown):
            continue
        shown |= set(files)
        count += 1
        if note.startswith("LICENSES/") or note == "LICENSE":
            where = note
        elif is_path(note):
            where = src.get(note, note)
        else:
            uri = g.licence_uri(lic)
            where = {
                "in file": "in each file",
                "uri": f"the licence's URI, {uri}" if uri else "the licence's URI",
                "none needed": "none needed",
                "none": "none",
            }.get(note, note)
        o += [
            "",
            textwrap.fill(what, width=76, break_on_hyphens=False, break_long_words=False),
            wrap("Files:", ", ".join(files)),
            wrap("Licence:", lic),
            wrap("Notice:", where),
            wrap("Status:", f"{status}: {g.STATUS[status]}"),
        ]
        if origin:
            # A path in baud's repository, which is not in the crate.
            if os.path.exists(os.path.join(ROOT, origin.split()[0].rstrip(","))):
                origin = f"baud's repository, {origin}"
            o.append(wrap("From:", origin))
    if not count:
        o += ["", "No file in this crate carries third-party material that", "docs/research/third-party.toml records."]
    return "\n".join(o) + "\n"


def check(size, verify, write):
    """(errors, awaiting): what fails the check, and the shipped files whose
    rows await the owner. With `write`, each crate's NOTICE is written
    first."""
    errs, awaiting = [], []
    ws = load("Cargo.toml")
    version = ws["workspace"]["package"]["version"]
    wdeps = ws["workspace"].get("dependencies", {})
    mem = members(ws)
    if "baud" not in mem:
        return ["no workspace member is named `baud`: `cargo install baud` would find nothing"], []
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
    entries = [
        (t["what"], t["paths"], t["notice"], t["status"], t["licence"], t["upstream"], "tree") for t in side["tree"]
    ]
    entries += [
        (e["name"], [f"docs/research/sources/{f}" for f in e["files"]], e["notice"], e["status"], e["licence"], "", "source")
        for g, e in sorted(side["upstream"].items())
    ]
    g = gen_third_party()
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
            src.setdefault(os.path.normpath(os.path.join(d, f)), f)
        want_notice = notice(n, entries, src, g)
        path = os.path.join(ROOT, d, "NOTICE")
        if write:
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(want_notice)
        if not os.path.isfile(path):
            errs.append(f"{n}: has no NOTICE: run scripts/check-publish.py --write-notices")
        elif "NOTICE" not in files and (not write or "NOTICE" not in (listed(n)[0] or [])):
            # A NOTICE written just now is listed only from here on.
            done = "NOTICE written, but " if write else ""
            errs.append(f"{n}: {done}NOTICE is not in the package: add /NOTICE to its include")
        elif open(path, encoding="utf-8").read() != want_notice:
            errs.append(f"{n}: NOTICE is not what its files' rows give: run scripts/check-publish.py --write-notices")
        for what, paths, notice_at, status, _, _, _ in entries:
            what = what[:50]
            hit = [p for p in paths if p in src]
            if hit and status in UNSETTLED:
                awaiting.append(f"{n}: ships {', '.join(src[p] for p in hit)} ({what}...), status {status}")
            if not hit or not is_path(notice_at):
                continue
            notice_ = notice_at
            if notice_.startswith("LICENSES/"):
                local = os.path.join(d, notice_)
                if notice_ not in files:
                    errs.append(f"{n}: ships {src[hit[0]]} ({what}) without {notice_}")
                elif not filecmp.cmp(os.path.join(ROOT, local), os.path.join(ROOT, notice_), shallow=False):
                    errs.append(f"{n}: {notice_} is not the root's")
            elif notice_ != "LICENSE" and notice_ not in src:
                errs.append(f"{n}: ships {src[hit[0]]} ({what}) without its notice {notice_}")

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
        print(f"checked: {len(want)} crates to publish, {len(set(mem) - want)} not")
    return errs, awaiting


def main():
    args = sys.argv[1:]
    release = "--release" in args
    errs, awaiting = check("--size" in args, "--verify" in args, "--write-notices" in args)
    for e in errs:
        print(f"FAIL {e}")
    for a in awaiting:
        print(f"{'REFUSED' if release else 'awaiting the owner'}: {a}")
    if errs:
        sys.exit(1)
    if awaiting and release:
        print(f"not releasable: {len(awaiting)} shipped files' licences await the owner")
        sys.exit(2)
    print("ok")
    sys.exit(0)


if __name__ == "__main__":
    main()
