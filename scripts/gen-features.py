#!/usr/bin/env python3
"""Write docs/guide/features.md and docs/roadmap.md from docs/data/features.toml
and docs/data/roadmap.toml.

    python3 scripts/gen-features.py           write both pages
    python3 scripts/gen-features.py --check   fail if anything below is wrong,
                                              both pages included

Of the twelve terminals surveyed for this, five keep a second feature list by
hand -- iTerm2, contour, rio, Windows Terminal and Wave -- and all five have
drifted from their programs. These two pages are generated instead, and the
data under them is checked against the tree. The checks, each of which has
to be able to fail:

  - every feature names its proof, and a feature marked `built` names at least
    one test, a coverage row that is done, or a measurement in MEASUREMENTS.md;
  - every proof path exists, and every measurement a feature cites is a
    heading of MEASUREMENTS.md;
  - every proof test is a `fn` in the file it names, so a renamed or deleted
    test is caught where the claim points;
  - every coverage row a feature cites is a row of that table, and a `built`
    feature's rows are all done;
  - every protocol chip is a protocol the data defines;
  - every roadmap id a feature or gap names exists and is not done yet;
  - every done roadmap item is claimed by a feature, or listed as not a
    feature with the reason, so a built feature cannot be missing from the
    list;
  - the count README.md links the list with ("See all N features") is the
    number of features the data holds;
  - both pages are byte for byte what this script writes.

The data files' headers say what each field means.
"""

import os
import re
import sys
import tomllib

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
FEATURES = "docs/data/features.toml"
ROADMAP = "docs/data/roadmap.toml"
FEATURES_MD = "docs/guide/features.md"
ROADMAP_MD = "docs/roadmap.md"
README = "README.md"
TABLES = {"vt": "docs/vt-coverage.md", "input": "docs/input-coverage.md"}
MEASUREMENTS = "MEASUREMENTS.md"

STATUS = ("built", "partial", "interim")
PLATFORM = ("both", "macos", "linux")
ROAD_STATUS = ("in_progress", "planned", "done")
ROW_WORDS = ("done", "partial", "out", "todo", "parsed", "counted")


def load(path):
    with open(os.path.join(ROOT, path), "rb") as f:
        return tomllib.load(f)


def read(path):
    with open(os.path.join(ROOT, path), encoding="utf-8") as f:
        return f.read()


def slug(heading):
    """GitHub's anchor for a heading: lower case, punctuation dropped, each
    space a hyphen."""
    h = heading.strip().lower().replace("`", "")
    h = re.sub(r"[^\w\- ]", "", h)
    return h.replace(" ", "-")


def coverage_rows():
    """Each coverage table's rows: {table: {first cell: (status word, anchor)}},
    and the first cells two rows of one table share, which a citation could not
    tell apart."""
    out, dupes = {}, set()
    for key, path in TABLES.items():
        rows, anchor, seen = {}, "", {}
        for line in read(path).split("\n"):
            m = re.match(r"#{2,4} (.*)", line)
            if m:
                a = slug(m.group(1))
                n = seen.get(a, 0)
                seen[a] = n + 1
                anchor = a if n == 0 else f"{a}-{n}"
                continue
            if not line.startswith("| ") or line.startswith("|---"):
                continue
            cells = [c.strip() for c in line.strip().strip("|").split(" | ")]
            if len(cells) < 2:
                continue
            word = ""
            for c in cells[1:]:
                w = re.match(r"\**([a-z]+)", c)
                if w and w.group(1) in ROW_WORDS:
                    word = w.group(1)
                    break
            if cells[0] in rows:
                dupes.add(f"{key}:{cells[0]}")
            rows[cells[0]] = (word, anchor)
        out[key] = rows
    return out, dupes


def fns_in(path, cache={}):
    if path not in cache:
        cache[path] = set(re.findall(r"\bfn ([A-Za-z0-9_]+)", read(path)))
    return cache[path]


def check_data(feat, road, rows, dupes):
    errs = []
    areas = [a["id"] for a in feat["area"]]
    protocols = {p["name"] for p in feat.get("protocol", [])}
    road_items = {i["id"]: i for i in road.get("item", [])}
    ids = set()
    for p in feat.get("protocol", []):
        t, _, cell = p["row"].partition(":")
        if cell not in rows.get(t, {}):
            errs.append(f"protocol {p['name']}: no {t} row {cell}")

    def roadmap_refs(where, refs):
        for r in refs:
            if r not in road_items:
                errs.append(f"{where}: roadmap id {r} is not in {ROADMAP}")
            elif road_items[r]["status"] == "done":
                errs.append(f"{where}: roadmap id {r} is done, so it closes no gap")

    headings = {m.group(1).strip() for m in re.finditer(r"^#+ (.*)$", read(MEASUREMENTS), re.M)}
    claimed = {}
    for f in feat["feature"]:
        where = f"feature {f.get('id', '?')}"
        for k in ("id", "area", "name", "does", "status", "platform"):
            if not f.get(k):
                errs.append(f"{where}: no {k}")
        if f["id"] in ids:
            errs.append(f"{where}: id used twice")
        ids.add(f["id"])
        if f["area"] not in areas:
            errs.append(f"{where}: area {f['area']} is not defined")
        if f["status"] not in STATUS:
            errs.append(f"{where}: status {f['status']} is not one of {STATUS}")
        if f["platform"] not in PLATFORM:
            errs.append(f"{where}: platform {f['platform']} is not one of {PLATFORM}")
        if f["status"] != "built" and not f.get("gap"):
            errs.append(f"{where}: a {f['status']} feature says what is missing in `gap`")
        if f["status"] == "built" and f.get("gap"):
            errs.append(f"{where}: a built feature has no gap")
        for p in f.get("paths", []):
            if not os.path.isfile(os.path.join(ROOT, p)):
                errs.append(f"{where}: proof path {p} does not exist")
        tests = f.get("tests", [])
        for t in tests:
            path, sep, name = t.partition("::")
            if not sep:
                errs.append(f"{where}: test {t} is not written <path>::<fn>")
            elif not os.path.isfile(os.path.join(ROOT, path)):
                errs.append(f"{where}: test file {path} does not exist")
            elif name not in fns_in(path):
                errs.append(f"{where}: {path} has no fn {name}")
        done_rows = 0
        for r in f.get("rows", []):
            t, _, cell = r.partition(":")
            if r in dupes:
                errs.append(f"{where}: two {t} rows begin {cell}, so the citation is ambiguous")
            if cell not in rows.get(t, {}):
                errs.append(f"{where}: no {t} coverage row {cell}")
                continue
            word = rows[t][cell][0]
            if word == "done":
                done_rows += 1
            elif f["status"] == "built":
                errs.append(f"{where}: built, but its row {cell} is {word or 'unmarked'}")
        measured = f.get("measured", [])
        for m in measured:
            if m not in headings:
                errs.append(f"{where}: {MEASUREMENTS} has no heading {m!r}")
        if not tests and not f.get("rows") and not f.get("paths") and not measured:
            errs.append(f"{where}: names no proof")
        if f["status"] == "built" and not tests and not done_rows and not measured:
            errs.append(f"{where}: built, and names no test, no done row and no measurement")
        for c in f.get("chips", []):
            if c not in protocols:
                errs.append(f"{where}: chip {c} is not a defined protocol")
        roadmap_refs(where, f.get("roadmap", []))
        for s in f.get("shipped", []):
            claimed.setdefault(s, []).append(f["id"])
    for g in feat.get("missing", []):
        where = f"missing {g.get('name', '?')}"
        if g.get("area") not in areas:
            errs.append(f"{where}: area {g.get('area')} is not defined")
        if not g.get("roadmap"):
            errs.append(f"{where}: names no roadmap item")
        roadmap_refs(where, g.get("roadmap", []))
    unlisted = {u["id"]: u for u in feat.get("unlisted", [])}
    for u in unlisted.values():
        if not u.get("why"):
            errs.append(f"unlisted {u['id']}: no reason")
    for i in road.get("item", []):
        where = f"roadmap {i.get('id', '?')}"
        if i.get("status") not in ROAD_STATUS:
            errs.append(f"{where}: status {i.get('status')} is not one of {ROAD_STATUS}")
        if i.get("area") not in areas:
            errs.append(f"{where}: area {i.get('area')} is not defined")
        if i.get("status") == "done":
            if i["id"] in claimed and i["id"] in unlisted:
                errs.append(f"{where}: both claimed by a feature and listed as not a feature")
            elif i["id"] not in claimed and i["id"] not in unlisted:
                errs.append(
                    f"{where}: done, and no feature claims it in `shipped` -- a built "
                    f"feature missing from the list, or an `unlisted` entry with its reason"
                )
    for s in list(claimed) + list(unlisted):
        if s not in road_items:
            errs.append(f"shipped or unlisted id {s} is not in {ROADMAP}")
        elif road_items[s]["status"] != "done":
            errs.append(f"shipped or unlisted id {s} is not done")
    return errs


def platform_mark(f):
    return {"both": "", "macos": " *macOS.*", "linux": " *Linux, headless so far.*"}[f["platform"]]


def render_features(feat, road, rows):
    protocols = {p["name"]: p for p in feat.get("protocol", [])}
    road_anchor = {}
    for a in feat["area"]:
        road_anchor[a["id"]] = slug(a["title"])

    def row_link(key):
        t, _, cell = key.partition(":")
        return f"../{os.path.basename(TABLES[t])}#{rows[t][cell][1]}"

    def first_test_file(f):
        for t in f.get("tests", []):
            return t.partition("::")[0]
        for p in f.get("paths", []):
            return p
        return None

    out = []
    w = out.append
    w("# Features")
    w("")
    w(feat["intro"].strip())
    w("")
    w(feat["legend"].strip())
    w("")
    w(" · ".join(f"[{a['title']}](#{slug(a['title'])})" for a in feat["area"]) + " · [Where baud is behind](#where-baud-is-behind)")
    w("")
    for a in feat["area"]:
        fs = [f for f in feat["feature"] if f["area"] == a["id"]]
        w(f"## {a['title']}")
        w("")
        if a.get("intro"):
            w(a["intro"].strip())
            w("")
        for f in fs:
            line = f"- **{f['name']}** — {f['does'].strip()}{platform_mark(f)}"
            if f["status"] == "partial":
                line += f" **Partly:** {f['gap'].strip()}"
            elif f["status"] == "interim":
                line += f" **For now:** {f['gap'].strip()}"
            small = []
            for c in f.get("chips", []):
                small.append(f"[{c}]({row_link(protocols[c]['row'])})")
            p = first_test_file(f)
            if p:
                small.append(f"[proof](../../{p})")
            for m in f.get("measured", [])[:1]:
                small.append(f"[measured](../../{MEASUREMENTS}#{slug(m)})")
            if f.get("rows"):
                small.append(f"[reference]({row_link(f['rows'][0])})")
            line += " <sub>" + " · ".join(small) + "</sub>"
            w(line)
        w("")
    w("## Protocols")
    w("")
    w(feat["protocols_intro"].strip())
    w("")
    w("| protocol | from | reference |")
    w("|---|---|---|")
    for p in feat.get("protocol", []):
        w(f"| {p['name']} | {p['origin']} | [in the coverage table]({row_link(p['row'])}) |")
    w("")
    w("## Where baud is behind")
    w("")
    w(feat["behind_intro"].strip())
    w("")
    for f in feat["feature"]:
        if f.get("behind"):
            refs = f.get("roadmap", [])
            tail = f" [Roadmap](../roadmap.md#{road_anchor[f['area']]})" if refs else ""
            w(f"- **{f['name']}** — {f['behind'].strip()}{tail}")
    for g in feat.get("missing", []):
        w(f"- **{g['name']}** — {g['does'].strip()} [Roadmap](../roadmap.md#{road_anchor[g['area']]})")
    w("")
    w("## How this page stays true")
    w("")
    w(feat["footer"].strip())
    return "\n".join(out) + "\n"


def render_roadmap(feat, road):
    out = []
    w = out.append
    w("# Roadmap")
    w("")
    w(road["intro"].strip())
    w("")
    w(" · ".join(f"[{a['title']}](#{slug(a['title'])})" for a in feat["area"]))
    w("")
    order = {"in_progress": 0, "planned": 1, "done": 2}
    for a in feat["area"]:
        items = [i for i in road.get("item", []) if i["area"] == a["id"]]
        if not items:
            continue
        open_items = sorted((i for i in items if i["status"] != "done"), key=lambda i: (order[i["status"]], i["title"]))
        done = sorted((i for i in items if i["status"] == "done"), key=lambda i: i["title"])
        done.sort(key=lambda i: i["date"], reverse=True)
        items = open_items + done
        w(f"## {a['title']}")
        w("")
        for i in items:
            text = i["summary"].strip()
            if i["status"] == "done":
                w(f"- [x] **{i['title']}** — {text} ({i['date']})")
            elif i["status"] == "in_progress":
                w(f"- [ ] **{i['title']}** — {text} *In progress.*")
            else:
                w(f"- [ ] **{i['title']}** — {text}")
        w("")
    w("## How this page stays true")
    w("")
    w(road["footer"].strip())
    return "\n".join(out) + "\n"


def check_readme(feat):
    """The front page's count of the list, which no generator writes."""
    n = len(feat.get("feature", []))
    said = re.findall(r"See all (\S+) features", read(README))
    if not said:
        return [f"{README}: no 'See all N features' link, so its count is unchecked"]
    return [f"{README}: says 'See all {s} features', the list has {n}" for s in said if s != str(n)]


def main():
    check = sys.argv[1:] == ["--check"]
    if sys.argv[1:] not in ([], ["--check"]):
        sys.exit(__doc__)
    feat, road = load(FEATURES), load(ROADMAP)
    rows, dupes = coverage_rows()
    errs = check_data(feat, road, rows, dupes) + check_readme(feat)
    if errs:
        print(f"{len(errs)} problem(s) in {FEATURES} / {ROADMAP}:", file=sys.stderr)
        for e in errs:
            print("  " + e, file=sys.stderr)
        sys.exit(1)
    pages = {FEATURES_MD: render_features(feat, road, rows), ROADMAP_MD: render_roadmap(feat, road)}
    stale = []
    for path, text in pages.items():
        full = os.path.join(ROOT, path)
        if check:
            have = open(full, encoding="utf-8").read() if os.path.exists(full) else ""
            if have != text:
                stale.append(path)
        else:
            with open(full, "w", encoding="utf-8") as f:
                f.write(text)
    if stale:
        print(
            "stale: " + ", ".join(stale) + " -- edit the data under docs/data/ and rerun "
            "scripts/gen-features.py; a generated page is never edited by hand",
            file=sys.stderr,
        )
        sys.exit(1)


if __name__ == "__main__":
    main()
