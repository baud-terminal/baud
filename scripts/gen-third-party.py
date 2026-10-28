#!/usr/bin/env python3
"""Write THIRD_PARTY.md, the index of every third-party file in the tree, from
docs/research/third-party.toml and docs/research/sources/MANIFEST.toml.

    python3 scripts/gen-third-party.py           write THIRD_PARTY.md
    python3 scripts/gen-third-party.py --check   fail if anything below is wrong,
                                                 THIRD_PARTY.md included

The checks, each of which has to be able to fail:

  - every `retrieved` file in the manifest is in exactly one group, and no
    group names a file the manifest does not hold as `retrieved`;
  - every file in LICENSES/ has a [text] entry and hashes to it, every entry's
    file exists, and every text is cited by some group or [[tree]] entry;
  - every `notice` that is a path, and every [[tree]] path, exists;
  - every status is one the sidecar defines;
  - every repository path LICENSE names exists (see licence_paths);
  - a [[tree]] entry whose notice is "in file", under a licence whose URI
    this script knows, names that URI in each of its files: the URI is
    what a Creative Commons licence asks a copy to carry, and a header the
    generator stopped writing would otherwise pass;
  - THIRD_PARTY.md is byte for byte what this script writes.

The sidecar's header says what each field means. Nothing here decides a
licence: it renders what the sidecar records and refuses to let the sidecar,
the manifest and the tree drift apart.
"""

import hashlib
import os
import sys
import tomllib

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
SIDECAR = "docs/research/third-party.toml"
MANIFEST = "docs/research/sources/MANIFEST.toml"
OUT = "THIRD_PARTY.md"
SRC = "docs/research/sources"

STATUS = {
    "kept": "redistributable, and its notice is satisfied",
    "own": "the work of baud's author",
    "nc": "kept verbatim under CC BY-NC-SA 3.0, which allows a verbatim copy that carries its attribution and the licence's URI; its NonCommercial term binds every copy of this file, whatever the repository's licence",
    "copyleft": "redistributable with its licence text, which is in LICENSES/; keeping it or making it `external` is a choice still open",
    "no-grant": "no grant to redistribute was found; moving it out of the tree (`external`) is a choice still open",
    "unverified": "licence not established; a choice still open",
    "open": "an open question, recorded where the file is",
}
ORDER = ["no-grant", "unverified", "open", "copyleft", "nc", "kept", "own"]


def load(path):
    with open(os.path.join(ROOT, path), "rb") as f:
        return tomllib.load(f)


def is_path(notice):
    return "/" in notice or notice in ("LICENSE",)


def notice_md(notice):
    """A notice as THIRD_PARTY.md shows it: a link when it is a file."""
    if is_path(notice):
        return f"[{notice}]({notice})"
    return {
        "uri": "the licence's URI, below",
        "none needed": "none needed",
        "none": "none",
        "in file": "in the file",
    }.get(notice, notice)


def licence_uri(spdx):
    uris = {
        "CC-BY-4.0": "https://creativecommons.org/licenses/by/4.0/",
        "CC-BY-SA-4.0": "https://creativecommons.org/licenses/by-sa/4.0/",
        "CC-BY-ND-4.0": "https://creativecommons.org/licenses/by-nd/4.0/",
        "CC-BY-NC-SA-3.0": "https://creativecommons.org/licenses/by-nc-sa/3.0/",
        "CC0-1.0": "https://creativecommons.org/publicdomain/zero/1.0/",
        "LicenseRef-W3C-Document": "https://www.w3.org/copyright/document-license/",
        "LicenseRef-IETF-RFC": "https://trustee.ietf.org/documents/trust-legal-provisions/",
        "LicenseRef-Ecma-Copyright": "https://www.ecma-international.org/policies/by-ipr/ecma-text-copyright-policy/",
    }
    return uris.get(spdx)


def gather():
    side = load(SIDECAR)
    man = load(MANIFEST)["sources"]
    # file -> the url of its copy, from the first key (in key order) naming it.
    url = {}
    for key in sorted(man):
        e = man[key]
        if e.get("status") == "retrieved" and e.get("file"):
            url.setdefault(e["file"], e["url"])
    return side, url


# LICENSE names at least this many paths from the repository root. The floor
# is the control: a matcher that stopped matching would find no missing path
# and pass, which is how a renamed file in LICENSE went unnoticed.
LICENCE_PATHS_FLOOR = 30


def licence_paths():
    """Every path LICENSE writes from the repository root, with its line: a
    token with a `/` whose first segment is an entry at the root. A path
    relative to its stanza's directory (`icon-licences/`, `var/bake/`) and an
    upstream project's path (one under its own `src/`) start with no root
    entry and are not repository paths; URLs and patterns (`*-observed/`) are
    skipped."""
    roots = set(os.listdir(ROOT))
    out = []
    with open(os.path.join(ROOT, "LICENSE"), encoding="utf-8") as f:
        for n, line in enumerate(f, 1):
            for tok in line.split():
                tok = tok.lstrip("(").rstrip(",.;:)")
                if "/" not in tok or "://" in tok or any(c in tok for c in "*<>{}"):
                    continue
                if tok.split("/")[0] in roots:
                    out.append((n, tok))
    return out


def check(side, url):
    errs = []
    paths = licence_paths()
    if len(paths) < LICENCE_PATHS_FLOOR:
        errs.append(
            f"LICENSE names {len(paths)} repository paths, fewer than {LICENCE_PATHS_FLOOR}: "
            "the match in licence_paths has stopped working, or the floor is stale"
        )
    for n, p in paths:
        if not os.path.exists(os.path.join(ROOT, p)):
            errs.append(f"LICENSE:{n} names {p}, which does not exist")
    seen = {}
    for gid, g in side["upstream"].items():
        if g["status"] not in STATUS:
            errs.append(f"group {gid}: status {g['status']!r} is not one of {sorted(STATUS)}")
        for f in g["files"]:
            if f in seen:
                errs.append(f"{f} is in two groups, {seen[f]} and {gid}")
            seen[f] = gid
            if f not in url:
                errs.append(f"group {gid} names {f}, which the manifest does not hold as retrieved")
        if is_path(g["notice"]) and not os.path.exists(os.path.join(ROOT, g["notice"])):
            errs.append(f"group {gid}: notice {g['notice']} does not exist")
    for f in sorted(url):
        if f not in seen:
            errs.append(f"{SRC}/{f} is retrieved in the manifest and in no group of {SIDECAR}")

    texts = side["text"]
    have = sorted(os.listdir(os.path.join(ROOT, "LICENSES")))
    for name in have:
        if name not in texts:
            errs.append(f"LICENSES/{name} has no [text] entry in {SIDECAR}")
    cited = {g["notice"] for g in side["upstream"].values()} | {t["notice"] for t in side["tree"]}
    for name, t in sorted(texts.items()):
        p = os.path.join(ROOT, "LICENSES", name)
        if not os.path.exists(p):
            errs.append(f"[text] {name} names LICENSES/{name}, which does not exist")
            continue
        got = hashlib.sha256(open(p, "rb").read()).hexdigest()
        if got != t["sha256"]:
            errs.append(f"LICENSES/{name} hashes {got}, {SIDECAR} says {t['sha256']}")
        if f"LICENSES/{name}" not in cited:
            errs.append(f"LICENSES/{name} is cited by no group and no [[tree]] entry")

    for i, t in enumerate(side["tree"]):
        if t["status"] not in STATUS:
            errs.append(f"[[tree]] {i}: status {t['status']!r} is not one of {sorted(STATUS)}")
        for p in t["paths"]:
            if not os.path.exists(os.path.join(ROOT, p)):
                errs.append(f"[[tree]] {i} ({t['what'][:40]}...): {p} does not exist")
        if is_path(t["notice"]) and not os.path.exists(os.path.join(ROOT, t["notice"])):
            errs.append(f"[[tree]] {i}: notice {t['notice']} does not exist")
        uri = licence_uri(t["licence"])
        if t["notice"] == "in file" and uri:
            for p in t["paths"]:
                full = os.path.join(ROOT, p)
                if os.path.isfile(full) and uri.encode() not in open(full, "rb").read():
                    errs.append(f"[[tree]] {i}: {p} is {t['licence']} with its notice in the file, and does not name {uri}")
    return errs


def render(side, url):
    groups = side["upstream"]
    tree = side["tree"]
    texts = side["text"]
    n = {s: 0 for s in STATUS}
    for g in groups.values():
        n[g["status"]] += len(g["files"])
    t_n = {s: 0 for s in STATUS}
    for t in tree:
        t_n[t["status"]] += 1

    o = []
    w = o.append
    w("# Third-party files in baud\n\n")
    w(
        "<!-- Generated by scripts/gen-third-party.py from docs/research/third-party.toml\n"
        "and docs/research/sources/MANIFEST.toml. Do not edit: change the sidecar and\n"
        "regenerate. scripts/check-sources.sh fails when this file is stale. -->\n\n"
    )
    w(
        "baud's own work is under CC BY-NC-SA 4.0 ([LICENSE](LICENSE)). This file lists the files in the\n"
        "tree that are someone else's work or carry some of it, as a licence audit of the tree (2026-10-10)\n"
        "and a sweep of what scripts/gen-* and scripts/extract-* read and write found them: each file's\n"
        "licence, where the notice that licence asks for is, and where it came from. Every file in\n"
        "docs/research/sources/ is listed, and the check fails on one that is not. The licence texts the\n"
        "files need are in\n"
        "[LICENSES/](LICENSES/). LICENSE names the fonts and code copies that its own licence does not\n"
        "cover. Each crate published to crates.io carries LICENSE, and in its own LICENSES/ the texts its\n"
        "files need, as links to these, which scripts/check-publish.py checks. Nothing here is legal\n"
        "advice; where a file's status is a choice still open, this file says so and decides nothing.\n\n"
    )
    w("## Summary\n\n")
    w("| status | research sources | other entries | meaning |\n|---|---|---|---|\n")
    for s in ORDER:
        if n[s] or t_n[s]:
            w(f"| {s} | {n[s]} | {t_n[s]} | {STATUS[s]} |\n")
    w(f"| total | {sum(n.values())} | {len(tree)} | |\n\n")

    w("## Licence texts in LICENSES/\n\n")
    w(
        "Fetched texts are byte for byte what the URL served; built texts say in their first lines what\n"
        "they were built from.\n\n"
    )
    w("| file | licence | from | sha256 |\n|---|---|---|---|\n")
    for name in sorted(texts, key=str.lower):
        t = texts[name]
        w(f"| [{name}](LICENSES/{name}) | {t['licence']} | {t['how']}: {t['from']} | {t['sha256'][:16]} |\n")
    w("\n")

    w("## In the tree outside docs/research/sources/\n\n")
    for s in ORDER:
        for t in tree:
            if t["status"] != s:
                continue
            w(f"### {t['what']}\n\n")
            w(f"- **Status:** {s} -- {STATUS[s]}\n")
            w(f"- **Licence:** {t['licence']}\n")
            w(f"- **Notice:** {notice_md(t['notice'])}\n")
            up = t["upstream"]
            w(f"- **From:** {up}\n")
            w("- **Files:** " + ", ".join(f"[{p}]({p})" for p in t["paths"]) + "\n\n")

    w("## docs/research/sources/\n\n")
    w(
        "Copies of other people's documents and code that baud's research cites, pinned by sha256 in\n"
        "[MANIFEST.toml](docs/research/sources/MANIFEST.toml). Each group gives the licence, where its\n"
        "notice is, and the URL each file was copied from.\n\n"
    )
    for s in ORDER:
        for gid in sorted(groups, key=lambda k: groups[k]["name"].lower()):
            g = groups[gid]
            if g["status"] != s:
                continue
            w(f"### {g['name']}\n\n")
            w(f"- **Status:** {s} -- {STATUS[s]}\n")
            lic = g["licence"]
            uri = licence_uri(lic)
            w(f"- **Licence:** {lic}" + (f" ({uri})" if uri else "") + "\n")
            w(f"- **Notice:** {notice_md(g['notice'])}\n")
            w(f"- **Files:** {len(g['files'])}\n\n")
            w("| file | copied from |\n|---|---|\n")
            for f in sorted(g["files"]):
                w(f"| [{f}]({SRC}/{f}) | {url[f]} |\n")
            w("\n")
    return "".join(o)


def main():
    for need in (SIDECAR, MANIFEST, "LICENSES"):
        if not os.path.exists(os.path.join(ROOT, need)):
            print(f"  FAIL {need} is missing, so no third-party file can be checked")
            return 1
    side, url = gather()
    errs = check(side, url)
    text = render(side, url)
    out = os.path.join(ROOT, OUT)
    if "--check" in sys.argv[1:]:
        try:
            cur = open(out, encoding="utf-8").read()
        except FileNotFoundError:
            cur = None
        if cur != text:
            errs.append(f"{OUT} is {'missing' if cur is None else 'stale'}: run scripts/gen-third-party.py")
        for e in errs:
            print(f"  FAIL {e}")
        if errs:
            return 1
        total = sum(len(g["files"]) for g in side["upstream"].values())
        print(
            f"  ok   {total} sources and {len(side['tree'])} other entries indexed, "
            f"{len(side['text'])} licence texts, {len(licence_paths())} LICENSE paths"
        )
        return 0
    if errs:
        for e in errs:
            print(f"  FAIL {e}")
        return 1
    with open(out, "w", encoding="utf-8") as f:
        f.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
