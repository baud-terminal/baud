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
    python3 scripts/check-publish.py --write-licences
                                                write each crate's `license`
                                                field, and check the rest

Exit 0 when everything passes, 1 on any FAIL, and 2 under --release when the
only thing left is a row awaiting the owner.

`cargo install baud` builds the `baud` package and every crate it depends on,
so each of those is published and nothing else is. The checks, each of which
has to be able to fail:

  - the published set is `baud` and the workspace crates it reaches through
    its dependencies, target-specific and optional ones included; every other
    member is in UNPUBLISHED and carries `publish = false`;
  - each published manifest has a description, a readme that exists, the
    repository, a rust-version, and keywords and categories crates.io
    accepts;
  - each published crate's `license` is what licence_expr() gives from its
    own file list: CC-BY-NC-SA-4.0, baud's, AND the licence of every
    `docs/research/third-party.toml` entry with a file in the crate, an
    SPDX expression whose every term is an identifier of SPDX_IDS or a
    LicenseRef- (--write-licences writes it); an entry whose `licence` is
    not such an expression says what it adds in `spdx`, "" when it adds
    nothing, or the check fails;
  - each dependency of a published crate on a workspace crate names the
    version being released as well as its path: cargo refuses to package one
    that does not, and drops the path from what it uploads;
  - each published crate's file list carries LICENSE, byte for byte the
    root's, and the notice every third-party file in it needs: for each
    `docs/research/third-party.toml` entry with a file in the list, a notice
    in LICENSES/ must be in the crate's own LICENSES/, byte for byte the
    root's, with each text its [text] entry `needs`, and a notice that is
    another file must be in the list;
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
  - what a NOTICE prints of a settled entry is its `public` and `holder`
    and the URLs it came from, and none of it names a research note, an
    ISO date, who decided, a hyphenated source key of MANIFEST.toml or
    any key written as [[source: key]], or a repository path the crate
    does not carry (wording()); an entry whose status is one of UNSETTLED is
    reported on the awaiting path instead, since --release refuses it and
    it is reworded when it is settled;
  - `baud`, the package `cargo install` builds as its own root, carries
    the workspace's [profile.release] and cargo-features: cargo reads a
    profile only from the root manifest, so without the copy an install
    builds without LTO, in 16 codegen units and without the function
    alignment; with --size or --verify, the manifest inside baud's .crate
    is held to the same;
  - with --size or --verify, every .crate is under crates.io's 10 MiB
    upload limit;
  - with --verify, every crate builds from its .crate alone, as `cargo
    publish` builds it before uploading: a file read through a path that
    leaves the crate's directory is not in the .crate, and fails here rather
    than partway through a release, after the crates before it are up;
  - with --size or --verify, each run packages and builds in a folder of
    its own, so no crate is built from an unpack or a compile of an
    earlier run (package()), and a crate the build took from cargo's
    unpack of the other .crates must have been unpacked by this run.

A packaged symlink is a copy of the file it points at (cargo archives the
target's bytes), so it counts as that file for the notices, and it must point
at a file in the repository.
"""

import filecmp
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import textwrap
import tomllib

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
SIDECAR = "docs/research/third-party.toml"
LICENCE = "CC-BY-NC-SA-4.0"
# What crates.io takes in `license`, and so what licence_expr() may write.
# Cargo's reference ("The license and license-file fields",
# [[source: cargo-1ef25ea-manifest-license]]) says crates.io reads the field
# as an SPDX 2.3 licence expression, AND and OR joining licences, that the
# name must be on the SPDX License List 3.20 (a version the parser has
# since passed: Unicode-3.0 postdates it, so the parser's 3.29.0 governs),
# and that a nonstandard licence may name a file in `license-file` instead.
# crates.io's parser ([[source: crates-io-b676909-licenses-rs]]) parses it
# with spdx =0.13.6 ([[source: crates-io-61edf7b-cargo-toml]]), which
# carries SPDX License List 3.29.0, takes an identifier on that list or a
# LicenseRef- term, and refuses any other term (`allow_unknown: false`;
# [[source: spdx-0-13-6-crate]] lexer.rs); a `license-file` with no
# `license` is recorded as "non-standard"
# ([[source: crates-io-c7d5e23-publish-rs]]), which would hide baud's own
# licence. So each crate sets `license`, and every term in it is one of
# these identifiers, each on the SPDX License List 3.29.0
# ([[source: spdx-license-list-3-29-0-json]]), or a LicenseRef-. An
# identifier a row comes to use is added here once it is checked against
# that list.
SPDX_IDS = frozenset({
    "Apache-2.0",
    "CC-BY-4.0",
    "CC-BY-NC-SA-4.0",
    "CC-BY-ND-4.0",
    "HPND",
    "HPND-sell-variant",
    "IJG",
    "MIT",
    "MIT-Modern-Variant",
    "MIT-open-group",
    "OFL-1.1",
    "Unicode-3.0",
    "Unlicense",
    "X11",
    "Zlib",
})
# A LicenseRef- term as the SPDX 2.3 grammar spells its idstring.
LICENCE_REF = re.compile(r"^LicenseRef-[A-Za-z0-9.-]+$")
UNPUBLISHED = {"baud-bench", "baud-difftest", "baud-sfte-difftest", "xtask"}
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
SOURCES = "docs/research/sources/"
# What a NOTICE must not say. It goes to crates.io, where a version stays
# as it was uploaded, to a reader who has only the crate: no research note,
# no ISO date a decision was taken, no word about who decided what, no
# hyphenated source key (g_keys) and no key written as [[source: key]].
# The rows' `what`, which THIRD_PARTY.md shows in the repository, may
# say all of these; a NOTICE prints a row's `public` instead.
PROCESS = (
    re.compile(r"\b[Nn]ote [A-Z]{1,2}\b"),
    re.compile(r"\b\d{4}-\d{2}-\d{2}\b"),
    # "lane" in lower case only: Thomas G. Lane is the IJG's holder.
    re.compile(r"\b(?:[Dd]ecided|[Dd]ecision|[Rr]ecommendation|[Oo]wner|lane|[Rr]efuter|[Mm]anifest|MANIFEST|Jev|TODO)\b"),
    # The repository's citation form, whatever key it names.
    re.compile(r"\[\[source:"),
)
URL = re.compile(r"^https?://\S+$")
DEP_TABLES = ("dependencies", "build-dependencies")
# Each source file's URL, by its name under SOURCES; set by check().
g_url = {}
# The source keys of MANIFEST.toml a NOTICE must not name bare; set by
# check(). Only the hyphenated ones: a one-word key such as xterm or
# wayland is also the word, and a NOTICE may say it.
g_keys = set()


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


def unshipped(text, src, tops):
    """The repository paths `text` names that the crate does not carry: a
    token with a slash, URLs aside, under a directory that never ships
    (`tops`), or naming a file or directory of the repository that is not
    in the crate under that name. An upstream project's own path, such as
    OT/glyf/Glyph.hh, names nothing here and passes."""
    shipped = set(src.values())
    bad = []
    for tok in re.sub(r"https?://\S+", " ", text).split():
        tok = tok.strip(".,;:()'\"")
        if "/" not in tok:
            continue
        here = tok.rstrip("/")
        if not re.search(r"[A-Za-z0-9]", here):
            continue
        carried = here in shipped or any(f.startswith(here + "/") for f in shipped)
        if tok.startswith(tops) or (not carried and os.path.exists(os.path.join(ROOT, here))):
            bad.append(tok)
    return bad


def keys(text):
    """The hyphenated source keys `text` names as a word of its own, URLs
    aside."""
    toks = (t.strip(".,;:()[]'\"`") for t in re.sub(r"https?://\S+", " ", text).split())
    return [t for t in toks if t in g_keys]


def wording(text, src, tops):
    """What `text` says that a NOTICE must not: process wording, source
    keys and repository-only paths."""
    found = [m.group(0) for rx in PROCESS for m in rx.finditer(text)]
    return found + keys(text) + unshipped(text, src, tops)


def notice(name, entries, src, g, repo, tops):
    """The NOTICE of crate `name`, and what is wrong with it: each sidecar
    entry with a file in the crate, by the paths the crate gives its files.
    `src` maps a repository path to the crate's path for it, both a file's
    own path and the one a link resolves to. Every [[tree]] entry with a
    file here is listed; a source group is left out when each of its files
    here is already listed under a [[tree]] entry, as the target of a link
    is. The second value is (status, what, finding) for each thing
    wording() finds in what the NOTICE prints of an entry, and for the
    header ("own")."""
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
            f"for is. It is generated from baud's record of third-party files, "
            f"in its repository at {repo}, whose THIRD_PARTY.md lists every such file "
            "there.",
            width=76,
            break_long_words=False,
            break_on_hyphens=False,
        ),
    ]
    found = [("own", "the header", f) for f in wording("\n".join(o), src, tops)]
    shown = set()
    count = 0
    for e in entries:
        what, paths, note, status, lic, kind = (
            e["what"], e["paths"], e["notice"], e["status"], e["licence"], e["kind"],
        )
        files = sorted({src[p] for p in paths if p in src})
        if not files or (kind == "source" and set(files) <= shown):
            continue
        shown |= set(files)
        count += 1
        text = e.get("public") or what
        origin = e.get("from") or ([e["upstream"]] if e["upstream"] else [])
        # A file of baud's sources is named by the URL it was copied from.
        origin = [g_url.get(x[len(SOURCES):], x) if x.startswith(SOURCES) else x for x in origin]
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
        block = [
            "",
            textwrap.fill(text, width=76, break_on_hyphens=False, break_long_words=False),
            wrap("Files:", ", ".join(files)),
        ]
        if e.get("holder"):
            block.append(wrap("Holder:", e["holder"]))
        block += [
            wrap("Licence:", lic),
            wrap("Notice:", where),
            wrap("Status:", f"{status}: {g.STATUS[status]}"),
        ]
        if origin:
            block.append(wrap("From:", " and ".join(origin)))
        o += block
        said = "\n".join([text, e.get("holder", ""), lic, where])
        found += [(status, what, f) for f in wording(said, src, tops)]
        found += [(status, what, f"From: {x}") for x in origin if not URL.match(x)]
        # A settled row says in its own words what it is and whose.
        found += [(status, what, f"no {k} field") for k in ("public", "holder") if not e.get(k)]
    if not count:
        o += [
            "",
            "No file in this crate carries material from another project that baud's",
            "record of third-party files lists.",
        ]
    return "\n".join(o) + "\n", found


def spdx_ok(expr):
    """Whether `expr` is an SPDX licence expression crates.io takes:
    operands of SPDX_IDS or LicenseRef- terms joined by AND and OR, in
    balanced parentheses. WITH is refused, since no exception is listed."""
    toks = re.findall(r"\(|\)|[^\s()]+", expr)
    depth, want_operand = 0, True
    for tok in toks:
        if tok == "(":
            if not want_operand:
                return False
            depth += 1
        elif tok == ")":
            if want_operand or depth == 0:
                return False
            depth -= 1
        elif tok in ("AND", "OR"):
            if want_operand:
                return False
            want_operand = True
        else:
            if not want_operand or not (tok in SPDX_IDS or LICENCE_REF.match(tok)):
                return False
            want_operand = False
    return bool(toks) and depth == 0 and not want_operand


def row_spdx(e):
    """What sidecar entry `e` adds to a crate's licence field: its `spdx`
    when it has one ("" for facts and protocol data, which carry no
    licence), else its `licence` when that is an expression crates.io
    takes, else None."""
    if "spdx" in e:
        return e["spdx"]
    return e["licence"] if spdx_ok(e["licence"]) else None


def licence_expr(terms):
    """The licence field of a crate whose third-party files add `terms`:
    baud's own licence AND each of them. A term of ANDs alone is split into
    its licences, so each is named once; a term with an OR is kept whole in
    parentheses, since AND binds tighter than OR."""
    out = set()
    for s in terms:
        if not s:
            continue
        toks = re.findall(r"\(|\)|[^\s()]+", s)
        if "OR" in toks or "(" in toks:
            out.add(s if s.startswith("(") and s.endswith(")") else f"({s})")
        else:
            out.update(x for x in toks if x != "AND")
    out.discard(LICENCE)
    return " AND ".join([LICENCE] + sorted(out))


def write_licence(path, expr):
    """Set the `license` of the manifest at `path` to `expr`, in place of
    whatever `license` line its [package] has."""
    with open(path, encoding="utf-8") as f:
        lines = f.read().split("\n")
    start = lines.index("[package]")
    end = next((i for i in range(start + 1, len(lines)) if lines[i].startswith("[")), len(lines))
    hits = [i for i in range(start, end) if re.match(r"^license(\.workspace)?\s*=", lines[i])]
    if len(hits) != 1:
        raise SystemExit(f"{path}: {len(hits)} license lines in [package], not one")
    lines[hits[0]] = f'license = "{expr}"'
    with open(path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))


def root_settings(name, m, ws):
    """What is wrong with `m`, the manifest of the package `cargo install`
    builds as its own root, against the workspace's release settings."""
    errs = []
    want, got = ws.get("profile", {}).get("release"), m.get("profile", {}).get("release")
    if not want:
        errs.append("the workspace has no [profile.release]: the profile check has stopped working")
    elif got != want:
        errs.append(f"{name}: [profile.release] is {got!r}, not the workspace's {want!r}")
    for f in ws.get("cargo-features", []):
        if f not in m.get("cargo-features", []):
            errs.append(f"{name}: cargo-features lacks {f!r}, which its [profile.release] needs")
    return errs


def package(root, names, version, verify):
    """(errs, packed, read): `cargo package` of the crates `names` of the
    workspace at `root`, each built from its own .crate when `verify`, as
    `cargo publish` builds it. `packed` maps each name to its .crate's size
    in bytes and the bytes of the Cargo.toml inside it; `read` is the set of
    unpacks, `<name>-<version>` directories under cargo's registry/src,
    that the build read.

    Cargo puts the .crate of each crate it packages in a local registry,
    `package/tmp-registry` under the build folder, and lays it over
    crates.io for the crates after it ([[source: cargo-eb94155-package-mod-rs]]
    lines 283-284 and 1100-1102). A crate of one name and version is
    immutable to cargo in two places, and each kept a crate's code from
    before a change that left the version alone, so that the crates built
    on it failed, or passed, on that old code:

      - its unpack: the local registry's crates unpack into
        $CARGO_HOME/registry/src/-<hash>, the hash of the registry's path
        as a file URL, and an unpack whose .cargo-ok is written is used as
        it is ([[source: cargo-eb94155-workspace-rs]] lines 2031-2035;
        [[source: cargo-eb94155-source-id-rs]] lines 248-251 and 686-690;
        [[source: cargo-eb94155-registry-mod-rs]] lines 459-469, 553 and
        618-636);
      - its compile: the verify builds in the build folder
        ([[source: cargo-eb94155-package-verify-rs]] lines 72-77), where a
        crate from the registry is fresh for as long as its version is
        ([[source: cargo-eb94155-overlay-rs]] lines 117-119;
        [[source: cargo-eb94155-fingerprint-mod-rs]] lines 336-338).

    So each call builds in a folder made for it, set as the target folder
    and as the build folder, which defaults to the target folder and may be
    configured apart ([[source: cargo-eb94155-config-md]] lines 540-547): a
    path no build used, so a registry URL and an unpack folder no build
    used, and no compiled crate. A crate read from an unpack folder that
    was there before the call is a FAIL. The folder is removed after, and
    so are the unpacks of `names` in each unpack folder the call made,
    found from the build's dep-info and errors, and that folder once it is
    empty; nothing else under CARGO_HOME is touched."""
    errs, packed, read = [], {}, set()
    home = os.environ.get("CARGO_HOME") or os.path.join(os.path.expanduser("~"), ".cargo")
    src = os.path.join(home, "registry", "src")
    parent = os.path.join(root, os.environ.get("CARGO_TARGET_DIR", "target"))
    os.makedirs(parent, exist_ok=True)
    fresh = tempfile.mkdtemp(prefix="check-publish-", dir=parent)
    unpack = re.compile(
        re.escape(src + os.sep) + r"([^/\s]+)/(" + "|".join(re.escape(f"{n}-{version}") for n in names) + r")/"
    )
    args = ["cargo", "package", "--allow-dirty", "-q"] + ([] if verify else ["--no-verify"])
    for n in names:
        args += ["-p", n]
    made = set()
    try:
        before = set(os.listdir(src)) if os.path.isdir(src) else set()
        env = {**os.environ, "CARGO_TARGET_DIR": fresh, "CARGO_BUILD_BUILD_DIR": fresh}
        out = subprocess.run(args, cwd=root, env=env, capture_output=True, text=True)
        said = [out.stderr]
        for d, _, fs in os.walk(fresh):
            for f in fs:
                if f.endswith(".d"):
                    with open(os.path.join(d, f), encoding="utf-8", errors="replace") as fh:
                        said.append(fh.read())
        stale = set()
        for m in unpack.finditer("\n".join(said)):
            read.add(os.path.join(src, m.group(1), m.group(2)))
            (stale if m.group(1) in before else made).add(m.group(1))
        errs += [
            f"cargo built a crate from {os.path.join(src, d)}, which was there before this run: "
            "what is unpacked there may be an older .crate's"
            for d in sorted(stale)
        ]
        if out.returncode != 0:
            errs.append(f"{' '.join(args[:2])} failed: {out.stderr.strip()[-2000:]}")
        else:
            for n in names:
                crate = os.path.join(fresh, "package", f"{n}-{version}.crate")
                with tarfile.open(crate) as t:
                    manifest = t.extractfile(f"{n}-{version}/Cargo.toml").read()
                packed[n] = (os.path.getsize(crate), manifest)
    finally:
        for d in made:
            for n in names:
                shutil.rmtree(os.path.join(src, d, f"{n}-{version}"), ignore_errors=True)
            try:
                os.rmdir(os.path.join(src, d))
            except OSError:
                pass
        shutil.rmtree(fresh, ignore_errors=True)
    return errs, packed, read


def check(size, verify, write, write_lic=False):
    """(errors, awaiting): what fails the check, and the shipped files whose
    rows await the owner. With `write`, each crate's NOTICE is written
    first, and with `write_lic` its `license` field."""
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

    errs += root_settings("baud", mem["baud"][1], ws)
    for n in sorted(want):
        d, m = mem[n]
        p = m["package"]
        for key in ("description", "repository", "rust-version"):
            if not field(p, ws, key):
                errs.append(f"{n}: no {key}")
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
    for e in [*side["tree"], *side["upstream"].values()]:
        if "spdx" in e and e["spdx"] and not spdx_ok(e["spdx"]):
            errs.append(f"{SIDECAR}: spdx {e['spdx']!r} is no SPDX expression crates.io takes")
    entries = [{**t, "kind": "tree"} for t in side["tree"]]
    entries += [
        {**e, "what": e["name"], "paths": [f"{SOURCES}{f}" for f in e["files"]], "upstream": "", "kind": "source"}
        for _, e in sorted(side["upstream"].items())
    ]
    g = gen_third_party()
    global g_url, g_keys
    manifest = load(SOURCES + "MANIFEST.toml")["sources"]
    g_url = {s["file"]: s["url"] for s in manifest.values() if s.get("file")}
    g_keys = {k for k in manifest if "-" in k}
    if len(g_keys) < 100:
        errs.append(f"only {len(g_keys)} hyphenated source keys in MANIFEST.toml: the key check has stopped working")
    repo = ws["workspace"]["package"]["repository"]
    # Directories of the repository no crate is packaged from.
    tops = tuple(f"{d}/" for d in ("docs", "scripts", ".github", "nix", "packaging", *ws["workspace"]["members"]))
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
        want_notice, found = notice(n, entries, src, g, repo, tops)
        terms = []
        for e in entries:
            hit = [src[x] for x in e["paths"] if x in src]
            if not hit:
                continue
            s = row_spdx(e)
            if s is None:
                line = (
                    f"{n}: ships {', '.join(hit)} ({e['what'][:50]}...), whose licence {e['licence']!r} "
                    "is no SPDX expression crates.io takes, and the entry has no spdx field"
                )
                (awaiting if e["status"] in UNSETTLED else errs).append(line)
                continue
            terms.append(s)
        want_lic = licence_expr(terms)
        if write_lic:
            write_licence(os.path.join(ROOT, d, "Cargo.toml"), want_lic)
        got_lic = load(os.path.join(d, "Cargo.toml"))["package"].get("license")
        if isinstance(got_lic, dict):
            got_lic = field({"license": got_lic}, ws, "license")
        if got_lic != want_lic:
            errs.append(
                f"{n}: license is {got_lic!r}, and its files give {want_lic!r}: "
                "run scripts/check-publish.py --write-licences"
            )
        for status, what, f in found:
            line = f"{n}: NOTICE says {f!r} ({what[:50]}...)"
            # A row awaiting the owner is refused at release anyway, and is
            # reworded when it is settled.
            (awaiting if status in UNSETTLED else errs).append(line)
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
        for e in entries:
            what, paths, notice_at, status = e["what"][:50], e["paths"], e["notice"], e["status"]
            hit = [p for p in paths if p in src]
            if hit and status in UNSETTLED:
                awaiting.append(f"{n}: ships {', '.join(src[p] for p in hit)} ({what}...), status {status}")
            if not hit or not is_path(notice_at):
                continue
            notice_ = notice_at
            if notice_.startswith("LICENSES/"):
                # A text another names as part of its notice goes with it.
                needs = side["text"].get(notice_.removeprefix("LICENSES/"), {}).get("needs", [])
                for text in [notice_, *needs]:
                    local = os.path.join(d, text)
                    if text not in files:
                        errs.append(f"{n}: ships {src[hit[0]]} ({what}) without {text}")
                    elif not filecmp.cmp(os.path.join(ROOT, local), os.path.join(ROOT, text), shallow=False):
                        errs.append(f"{n}: {text} is not the root's")
            elif notice_ != "LICENSE" and notice_ not in src:
                errs.append(f"{n}: ships {src[hit[0]]} ({what}) without its notice {notice_}")

    if (size or verify) and not errs:
        perrs, packed, read = package(ROOT, sorted(want), version, verify)
        errs += perrs
        if verify and not perrs and not read:
            # Every crate but the leaves builds against the others' .crates.
            errs.append("the verify build read no crate from cargo's unpack of the others: the check that it is this run's has stopped working")
        for n, (b, manifest) in sorted(packed.items()):
            print(f"  {n}: {b / 1048576:.2f} MiB compressed")
            if b > LIMIT:
                errs.append(f"{n}: {b} bytes, over crates.io's {LIMIT}")
            if n == "baud":
                errs += [f"{e} in its .crate" for e in root_settings(n, tomllib.loads(manifest.decode()), ws)]
    if not errs:
        print(f"checked: {len(want)} crates to publish, {len(set(mem) - want)} not")
    return errs, awaiting


def main():
    args = sys.argv[1:]
    release = "--release" in args
    errs, awaiting = check("--size" in args, "--verify" in args, "--write-notices" in args, "--write-licences" in args)
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
