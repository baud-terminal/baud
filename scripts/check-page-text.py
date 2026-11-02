#!/usr/bin/env python3
"""Check the pinned learn.microsoft.com pages by the article text they render.

A learn.microsoft.com page is rebuilt with the site around it. Its bytes carry
the site's asset version string, and the markup around the article moves, so
the page's sha256 stops matching within days while the article does not
change. Fetched on 2026-11-01, none of the 57 HTML pages pinned in
docs/research/sources/MANIFEST.toml hashed as pinned, one pinned the day
before among them, though two fetches that day were byte-identical. What does
reproduce is the article's text. Wayback copies of eight of the pages, served
in older templates, hash as that day's fetch wherever the article's words had
not changed (fvar, gvar, head, avar, and recom, whose revision had moved) and
differ where they had (os2's "type face", chapter2's "glyphs IDS" and seven
script tags added to scripttags).

So each such entry records three more facts about a fetch:

    revision     the page's git_commit_id meta, the commit of the documentation
                 repository it renders, where the page has one. A label: the
                 repositories most pages' gitcommit meta names answer 404 (GitHub) or ask for a
                 sign-in (Azure DevOps). The three win32-pr pages
                 (ms-dxgi-flip-model, ms-high-dpi, ms-dxgi-vrr) are the
                 exception: their revisions are also commits of the public
                 MicrosoftDocs/win32, the repository each page names for
                 feedback. On 2026-11-01 the GitHub commits API answered 200
                 for 3fcc17ad, 55e845e9 and 129846d6 in MicrosoftDocs/win32,
                 and raw.githubusercontent.com served each page's .md at its
                 revision. The others resolve in no public repository the
                 pages name: the sdk-api-build revisions answer 422 in
                 MicrosoftDocs/sdk-api, the typography ones 422 in
                 MicrosoftDocs/typography-issues, and the tn-archive page
                 names only its sign-in repository.
    text_sha256  the sha256 of the article text, below. This is the check.
    text_bytes   that text's length in UTF-8 bytes.

The entry's sha256 and bytes stay the record of the page as fetched the day it
was read.

**The article text** is the character data inside every <div class="content">
-- the site's template puts the heading in one and the article in another --
in document order, with two kinds of subtree left out:

  * the page-metadata container (the date, contributor count and feedback
    button) and nav elements: an older template put both inside the article's
    div, and its article text then carried "Article05/29/2024 3 contributors
    Feedback In this article", which is the site's and not the article's;
  * button, script, style and template elements, which render no article text.

Character references are decoded, every run of HTML's ASCII whitespace (tab,
line feed, form feed, carriage return, space) becomes one space, so markup
reindented around the same words reads the same, and the ends are trimmed. A
no-break space is kept, as rendering keeps it: the script tags page writes
Yi's tag with two of them.

**Refused, never compared:** a page with no such div, or whose text is under
FLOOR bytes. An extractor that matches nothing hashes the empty string, and a
pin recorded from it would pass on any page at all; the first extractor tried
on these pages returned empty text on the older template. Each key in CONTROL
names a sentence the manifest quotes from that page, and a text without it is
refused too: a positive control, so a broken extraction is never read as a
changed page.

Usage:
    python3 scripts/check-page-text.py              # fetch every entry, compare
    python3 scripts/check-page-text.py --key KEY    # only these entries
    python3 scripts/check-page-text.py --from DIR   # read DIR/KEY.html, no fetch
    python3 scripts/check-page-text.py --save DIR   # keep each fetch as DIR/KEY.html
    python3 scripts/check-page-text.py --print      # the new fields, as TOML
    python3 scripts/check-page-text.py --self-test  # synthetic pages, no network

It needs the network and is run on demand. scripts/check-sources.sh, which
must stay offline and deterministic, calls offline_problems() instead: every
page entry carries a well-formed text_sha256 and text_bytes, a revision is
well formed where one is recorded, and the self-test passes.

Exit: 0 every text matches its pin; 1 a text differs, or an entry lacks its
fields; 2 a page was refused; 3 a fetch failed.
"""

from __future__ import annotations

import argparse
import hashlib
import html.parser
import re
import sys
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "docs/research/sources/MANIFEST.toml"

HOST = "https://learn.microsoft.com/"
# A figure on the site is served as itself and its sha256 reproduces.
NOT_PAGES = (".png", ".jpg", ".jpeg", ".gif", ".svg", ".pdf")
# The shortest article pinned is loca's, 1,539 bytes on 2026-11-01; a heading
# alone is under a hundred.
FLOOR = 1024
# The page entries on 2026-11-01. A scope that matches fewer has stopped
# matching; lower it only when an entry is removed.
AT_LEAST = 59
EMPTY = hashlib.sha256(b"").hexdigest()

CONTROL = {
    "ms-opentype-head": "Left sidebearing point at x=0 (relevant only for TrueType rasterizers)",
    "ms-opentype-vmtx": "The x coordinate of a glyph\u2019s vertical origin is not specified in the 'vmtx' table",
    "ms-opentype-sbix": "then the glyph must be drawn using a bitmap from some strike",
    "ms-opentype-chapter2": "Applications may process script-neutral characters together with",
    "ms-opentype-gpos-2026-10-23": "If valueFormat2 is set to 0, then the second glyph of the pair",
    "ms-itaskbarlist3-setprogressstate": "the window with the least progress is used",
    "ms-devanagari-script-development": "Shaping engine implementations may vary somewhat with regard to what sequences are or are not considered valid",
    "ms-opentype-features-ae": "This feature needs to be implemented prior to any other feature",
}

SKIP = {"nav", "button", "script", "style", "template"}
WHITESPACE = re.compile(r"[\t\n\f\r ]+")
HEX = re.compile(r"[0-9a-f]+")


def classes(attrs) -> list[str]:
    return (dict(attrs).get("class") or "").split()


class Article(html.parser.HTMLParser):
    """The article text of one page, and the meta tags that label it."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.divs = 0  # open divs inside the current content div, itself included
        self.skip = None  # the tag of the subtree being left out
        self.nest = 0  # open elements of that tag inside it
        self.found = 0
        self.parts: list[str] = []
        self.meta: dict[str, str] = {}

    def handle_starttag(self, tag, attrs):
        if tag == "meta":
            a = dict(attrs)
            if a.get("name") in ("git_commit_id", "gitcommit", "updated_at"):
                self.meta[a["name"]] = a.get("content") or ""
        if self.skip:
            self.nest += tag == self.skip
            return
        if not self.divs:
            if tag == "div" and classes(attrs) == ["content"]:
                self.divs, self.found = 1, self.found + 1
            return
        if tag in SKIP or (tag == "div" and "page-metadata-container" in classes(attrs)):
            # What it held stood between the words on either side of it.
            self.skip, self.nest = tag, 1
            self.parts.append(" ")
            return
        self.divs += tag == "div"

    def handle_endtag(self, tag):
        if self.skip:
            if tag == self.skip:
                self.nest -= 1
                if not self.nest:
                    self.skip = None
            return
        if self.divs and tag == "div":
            self.divs -= 1
            if not self.divs:
                self.parts.append("\n")

    def handle_data(self, data):
        if self.divs and not self.skip:
            self.parts.append(data)


class Refused(Exception):
    pass


def read(page: bytes) -> dict:
    """The three facts the manifest records, and the labels beside them."""
    p = Article()
    p.feed(page.decode("utf-8"))
    p.close()
    if not p.found:
        raise Refused('no <div class="content"> on the page')
    text = WHITESPACE.sub(" ", "".join(p.parts)).strip(" ").encode("utf-8")
    if len(text) < FLOOR:
        raise Refused(f"its article text is {len(text)} bytes, under the floor of {FLOOR}")
    rev = p.meta.get("git_commit_id", "")
    return {
        "revision": rev if len(rev) == 40 and HEX.fullmatch(rev) else None,
        "repository": re.sub(r"/(blob|commit)/[0-9a-f]{40}.*", "", p.meta.get("gitcommit", "")),
        "updated": p.meta.get("updated_at"),
        "text": text,
        "text_sha256": hashlib.sha256(text).hexdigest(),
        "text_bytes": len(text),
        "sha256": hashlib.sha256(page).hexdigest(),
        "bytes": len(page),
    }


def compare(key: str, entry: dict, page: bytes, control: str | None = None) -> tuple[int, str]:
    """0 and a note when the text matches the pin, else 1 or 2 and why."""
    try:
        got = read(page)
    except (Refused, UnicodeDecodeError) as e:
        return 2, f"refused: {e}"
    control = control or CONTROL.get(key)
    if control and control.encode("utf-8") not in got["text"]:
        return 2, f"refused: the text does not hold the sentence its control quotes, {control!r}"
    if "text_sha256" not in entry:
        return 1, "the entry records no text_sha256"
    notes = []
    if got["sha256"] != entry.get("sha256"):
        notes.append(f"page {got['bytes']} bytes, pinned {entry.get('bytes')}, another sha256")
    if got["revision"] != entry.get("revision"):
        notes.append(f"revision {str(entry.get('revision'))[:8]} -> {str(got['revision'])[:8]}")
    if got["text_sha256"] == entry["text_sha256"] and got["text_bytes"] == entry.get("text_bytes"):
        return 0, "; ".join(["text as pinned", *notes])
    return 1, "; ".join(
        [f"TEXT CHANGED: {got['text_bytes']} bytes {got['text_sha256'][:8]}, "
         f"pinned {entry.get('text_bytes')} {entry['text_sha256'][:8]}", *notes]
    )


def in_scope(entry: dict) -> bool:
    url = entry.get("url", "")
    return (
        url.startswith(HOST)
        and entry.get("status") != "gated"
        and not url.lower().endswith(NOT_PAGES)
    )


def field_problems(key: str, entry: dict) -> list[str]:
    out = []
    t = entry.get("text_sha256")
    n = entry.get("text_bytes")
    r = entry.get("revision")
    if not isinstance(t, str) or len(t) != 64 or not HEX.fullmatch(t):
        out.append(f"{key}: text_sha256 is not 64 lowercase hex: {t!r}")
    elif t == EMPTY:
        out.append(f"{key}: text_sha256 is the empty string's")
    if type(n) is not int or n < FLOOR:
        out.append(f"{key}: text_bytes is {n!r}, want an integer of at least {FLOOR}")
    if r is not None and (not isinstance(r, str) or len(r) != 40 or not HEX.fullmatch(r)):
        out.append(f"{key}: revision is not 40 lowercase hex: {r!r}")
    return out


def offline_problems(sources: dict) -> list[str]:
    """What check-sources.sh fails on: every page entry carries a well-formed
    text_sha256 and text_bytes, a revision is well formed wherever one is
    recorded, an entry outside the scope that carries any of the three is held
    to the same, the scope still matches, and the self-test passes."""
    out = []
    scope = {k for k, e in sources.items() if in_scope(e)}
    if len(scope) < AT_LEAST:
        out.append(f"only {len(scope)} learn.microsoft.com page entries, want at least {AT_LEAST}: the scope stopped matching")
    for k, e in sources.items():
        if k in scope or any(f in e for f in ("revision", "text_sha256", "text_bytes")):
            out += field_problems(k, e)
    out += [f"self-test: {m}" for m in self_test()]
    return out


# -- the self-test: synthetic pages, so no vendor page has to be committed ----

SENTENCE = "Each glyph has a top side bearing (the 'yi  ' tag) &amp; an advance. "
BODY = "<p>" + SENTENCE * 20 + "</p>\n<pre><code>uint16 numOfLongVerMetrics;</code></pre>"


def page(body: str, asset: str = "0.4.03571.8296-bb082b73", old: bool = False, indent: str = "\n\t\t") -> bytes:
    head = (
        f'<html><head><meta name="git_commit_id" content="{"a" * 40}" />'
        f'<link rel="stylesheet" href="/static/assets/{asset}/styles/site.css">'
        '<meta name="updated_at" content="2024-05-31T17:41:00Z" /></head><body>'
    )
    meta = (
        f'<div class="page-metadata-container">{indent}<ul class="metadata page-metadata"><li>Article</li>'
        '<li><time datetime="2024-05-29T08:00:00Z">05/29/2024</time></li>'
        '<li><button>3 contributors</button></li></ul>'
        '<div id="user-feedback"><button><span>Feedback</span></button></div></div>'
        '<nav class="doc-outline"><h2>In this article</h2></nav>'
    )
    if old:
        art = f'<div class="content "><h1 id="t">vmtx table</h1>{meta}<!-- <content> -->{body}</div>'
    else:
        art = f'<div class="content"><h1 id="t">vmtx table</h1></div>{indent}{meta}{indent}<div class="content">{body}</div>'
    return (head + f"<main>{indent}{art}{indent}</main><footer>{asset}</footer></body></html>").encode("utf-8")


def self_test() -> list[str]:
    bad = []
    base = page(BODY)
    pin = read(base)
    entry = {k: pin[k] for k in ("revision", "text_sha256", "text_bytes", "sha256", "bytes")}

    def expect(name, want, got):
        code, why = got
        if code != want:
            bad.append(f"{name}: exit {code}, want {want} ({why})")

    expect("the pinned page", 0, compare("k", entry, base))
    expect("new chrome around the same text", 0, compare("k", entry, page(BODY, asset="0.4.03568.8292-0a6c2196", indent="\n    ")))
    expect("the older template, metadata inside the article", 0, compare("k", entry, page(BODY, old=True)))
    expect("a script inside the article", 0, compare("k", entry, page(BODY + "<script>var v = 1;</script>")))
    expect("one word of the article changed", 1, compare("k", entry, page(BODY.replace("advance.", "advances.", 1))))
    expect("a no-break space turned into a space", 1, compare("k", entry, page(BODY.replace(" ", " ", 1))))
    expect("an article cut to its heading", 2, compare("k", entry, page("")))
    expect("no content div at all", 2, compare("k", entry, base.replace(b'class="content"', b'class="article"')))
    if read(page(BODY, asset="0.4.03568.8292-0a6c2196"))["sha256"] == pin["sha256"]:
        bad.append("the chrome change did not change the page bytes, so it tests nothing")
    if b"&amp;" in pin["text"] or " ".encode() not in pin["text"]:
        bad.append("character references are not decoded as the page renders them")
    expect("a control sentence the text holds", 0, compare("k", entry, base, control="the 'yi\u00a0\u00a0' tag) & an"))
    expect("a control sentence the text does not hold", 2, compare("k", entry, base, control="a sentence never written"))
    return bad


# -- on demand: the network ---------------------------------------------------


def fetch(url: str) -> bytes:
    # urllib's own User-Agent: on 2026-11-01 sbix served it curl's bytes, and a
    # browser's got an injected script around the same article text.
    with urllib.request.urlopen(url, timeout=60) as r:
        return r.read()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--key", action="append", default=[])
    ap.add_argument("--from", dest="src", type=Path)
    ap.add_argument("--save", type=Path)
    ap.add_argument("--print", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()

    if args.self_test:
        bad = self_test()
        for b in bad:
            print(f"  FAIL {b}")
        print("self-test: " + ("ok" if not bad else f"{len(bad)} failed"))
        return 1 if bad else 0

    with open(MANIFEST, "rb") as f:
        sources = tomllib.load(f)["sources"]
    keys = args.key or [k for k, e in sources.items() if in_scope(e)]
    worst = 0
    hits = 0
    for key in keys:
        entry = sources.get(key)
        if entry is None:
            print(f"  FAIL {key}: not in the manifest")
            worst = max(worst, 1)
            continue
        try:
            raw = (args.src / f"{key}.html").read_bytes() if args.src else fetch(entry["url"])
        except OSError as e:
            print(f"  FAIL {key}: {e}")
            worst = max(worst, 3)
            continue
        if args.save:
            args.save.mkdir(parents=True, exist_ok=True)
            (args.save / f"{key}.html").write_bytes(raw)
        if args.print:
            try:
                got = read(raw)
            except (Refused, UnicodeDecodeError) as e:
                print(f"# {key}: refused: {e}")
                worst = max(worst, 2)
                continue
            control = CONTROL.get(key)
            if control and control.encode("utf-8") not in got["text"]:
                print(f"# {key}: refused: the text does not hold {control!r}")
                worst = max(worst, 2)
                continue
            print(f"[sources.{key}]")
            if got["revision"]:
                print(f'revision    = "{got["revision"]}"')
            print(f'text_sha256 = "{got["text_sha256"]}"')
            print(f"text_bytes  = {got['text_bytes']}")
            print(f"# {got['repository'] or 'no repository named'}; updated_at {got['updated']}")
            continue
        code, why = compare(key, entry, raw)
        hits += code == 0 and key in CONTROL
        print(f"  {'ok  ' if code == 0 else 'FAIL'} {key}: {why}")
        worst = max(worst, code)
    if not args.print:
        print(f"{len(keys)} checked, worst exit {worst}; {hits} control sentence(s) found")
    return worst


if __name__ == "__main__":
    raise SystemExit(main())
