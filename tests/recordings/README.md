# recordings

Real escape streams, captured once from real programs through a real PTY, and
committed as fixed input.

Every payload baud had been tested and measured against before these was
written by the same people who wrote baud. `baud-bench/benches/ingest2.rs` says so
in its own header: the synthetic payloads want validating against real capture
"before it is designed around". These are that capture.

## The split

`ascii/` and `utf8/` divide by measurement, not intent: a file is in `utf8/`
when **more than 1%** of its bytes are non-ASCII. The three ASCII scrollers
carry 400-odd stray high bytes each, 0.1%, which is a stray glyph in a log
rather than a decoder workout.

| file | program | bytes | non-ASCII | wide cps | scrolls | what it exercises that the others do not |
|---|---|---|---|---|---|---|
| `ascii/asciiquarium.raw` | asciiquarium, 45x160 | 335479 | 0.0% | 0 | 20 | curses animation: heavy cursor addressing with 16-colour SGR |
| `ascii/cat-scroll.raw` | `cat` of 437 KB of Rust source, 50x200 | 448460 | 0.1% | 0 | many | **scrolling**, and text with no escapes at all |
| `ascii/git-log.raw` | `git --no-pager log --stat --color`, 50x200 | 501185 | 0.1% | 0 | many | colour over scrolling text, an SGR every 141 bytes |
| `ascii/less-pager.raw` | `less -R` paged with space/`b`/`G`/`g` | 30146 | 0.1% | 0 | 5104 | full-screen repaint in the **alternate screen** |
| `utf8/btop.raw` | btop, 45x160 | 524288 | 14.5% | **0** | 0 | truecolour `38;2`, box drawing, dense full-screen repaint, SGR mouse modes |
| `utf8/vim-unicode.raw` | vim on a generated UTF-8 file, paged with `^F`/`^B` | 138571 | 28.2% | 3740 | 0 | scroll regions (DECSTBM), CJK, emoji, ZWJ, RTL, combining marks |
| `utf8/cat-cjk.raw` | `cat` of `cjk-corpus.txt` x3, 50x200 | 327351 | 81.2% | 69741 | 125951 | **wide characters while scrolling**, with no escapes at all |
| `utf8/less-cjk.raw` | `less -R` over the same, paged both ways | 84473 | 76.3% | 17002 | 30930 | wide characters scrolling in the **alternate screen** |

## Why the CJK two exist

**Counting non-ASCII bytes against wide codepoints split what looked like one
axis into two.** btop is 14.5% multi-byte and has **zero wide codepoints** --
box drawing and braille are one column each -- so it exercises the decoder and
never the two-cell pair. `vim-unicode` was the only file in the corpus with
wide characters at all, 3,740 of them, and it scrolls **zero** times.

So every scroll, erase and line-move this project has ever measured or tested
ran on one-column text. Two things were hiding in that gap:

* **Correctness.** Nothing maintained the wide-pair invariant. Erasing,
  overwriting or shifting across half a double-width character left an orphan
  -- a lead cell with no tail, or a tail with no lead. `[[source:
  ghostty-screen]]`'s `eraseChars` states the rule baud was missing: "if our
  last cell is a wide char then we need to also clear the cell beyond it since
  we can't just split a wide char".
* **Performance.** baud is about **six times slower** on CJK than on the same
  shape of ASCII workload -- `cat-cjk` 122 MB/s against `cat-scroll`'s 743,
  `less-cjk` 130 against `less-pager`'s 705. The attribution says where it
  goes: CJK is 5% bulk and 13% single-cell where ASCII is 15% bulk and 0%
  single. A wide character cannot use `put_ascii_run`, so it is written one
  codepoint at a time.

`cjk-corpus.txt` is the generated input, committed so the *input* is
reproducible even though the capture is not. It is seeded Han, kana, Hangul,
emoji, ZWJ sequences, combining marks and mixed ASCII/CJK lines, including the
case where a wide character lands against the last column.

## `sixel/yazi-walk.raw`

yazi 26.5.6's output, 2,805,276 bytes, as a capture harness's pty received it
on 2026-10-09 (sha256
`d16d2a1f18f0d5f3236f1dd166a6014b8d47c48ae8497746a65b5c1d1b18f8e6`, the
`walk-big` run of round 2026-10-13c's `runs.sha256`): the cursor stepped down
six files with `j` and back six with `k` over seven synthetic test files
(round 2026-10-13c, Limits) -- `photo.jpg`, `flat.png`, `alpha.png`,
`screenshot.png`, `small.png`, `sparse.pdf` and `text.pdf`. Thirteen previews,
each a Sixel `ESC P 9;1 q "1;1;W;H` -- `P2 = 1`, so an unset bit keeps what is
under the image -- cleared with spaces and the next drawn at the same cell.
The replies it was sent are not in the file; one gave the cell as 17x34
(`CSI 6 ; 34 ; 17 t`); the file's first screen shows the capture's scratch
directory,
`$TMPDIR/scratch`.
It is committed whole because a recording is not edited before it is measured,
and because `every_preview_in_a_yazi_walk_is_its_own_body_alone`
(`baud-core/tests/sixel_reuse.rs`) needs both directions of the walk: it
requires preview k to equal preview 14 - k as well as each preview to equal
its body decoded alone.

## kitty graphics: regenerated, not committed

`scripts/record-kitten-icat.py` records what `kitten icat` 0.49.2 writes,
`--transfer-mode=stream`, to a pty carrying the app's pixels (17x34 a
cell), for note V's V19 step 0 (`docs/record.md`, "A `kitten icat`
session, recorded, and the three numbers V19 owed"). Unlike the captures
above, these regenerate: kitty and the two nixos-artwork pictures are
pinned by nixpkgs revision, and two runs gave the same bytes. None is
committed. `mosaic-blue` and `waterfall` are CC-BY-SA-4.0 and `skyglow` is
Apple's, and whether a recording that carries one belongs in this
repository is the owner's call. kitten fits a picture to the window's
width alone, so 113x24, 816 pixels high, sends the same bytes as 113x32.
The sha256 of each:

| input | window | sent | bytes | sha256 |
|---|---|---|---|---|
| mosaic-blue, 1920x1080 RGB | 113x32 | `f=100`, the file | 1281344 | `e2951c9e4603718c255ebcd47c4b2e41f02aa1e0a0f101497bbc5c656f378520` |
| | 80x24 | `f=24,o=z`, 1360x765 | 1273450 | `f0bb2f55e9df86cbbdf70fed6c8de75d8a8b464d0587dffa62471957d94bc90d` |
| | 113x24 | `f=100`, the file | 1281344 | `e2951c9e4603718c255ebcd47c4b2e41f02aa1e0a0f101497bbc5c656f378520` |
| | 226x64 | `f=100`, the file | 1281349 | `7d96457f2c7d1c1358795d0534167ccfdc45148bdb2af81f36367720e67eeee6` |
| waterfall, 3840x2160 RGBA | 113x32 | `f=24,o=z`, 1921x1080 | 5622860 | `fcd84ebb50c320c4dbdaff032a3d524082c7945e9aced72404090895d9788d8b` |
| | 80x24 | `f=24,o=z`, 1360x765 | 2933892 | `a213899c577ecb1a865cd6d80e2d53d3545949117ef8ace66a211fd8c09e2602` |
| | 113x24 | `f=24,o=z`, 1921x1080 | 5622860 | `fcd84ebb50c320c4dbdaff032a3d524082c7945e9aced72404090895d9788d8b` |
| | 226x64 | `f=100`, the file | 18288394 | `0e573d73813b4405e83b89f541c0fc68afe7a138c5681e205c99e4ffc5decee2` |
| skyglow, 1160x580, Adam7 (macOS 27.0 only) | 113x32 | `f=100`, the file | 24022 | `a8a1e2d64c3f00e01879b02166dd9bf4075afd449dec2f2bb537e8ea902d6735` |
| | 80x24 | `f=100`, the file | 24021 | `19c7f4931b2f6c728b087feaee576ef3e066c4cb4c308ac8b8f42407338e5016` |
| | 113x24 | `f=100`, the file | 24022 | `a8a1e2d64c3f00e01879b02166dd9bf4075afd449dec2f2bb537e8ea902d6735` |
| | 226x64 | `f=100`, the file | 24022 | `4338404469e1ba447d80c3e4323ec6ae7de3daad5e2cc135725c540c8097b0ed` |

**One is committed**, `kitty/grad-80x24-placeholder.raw` (8604 bytes, sha256
`5860df5a73c9e2ba9e63303ad4ff85afd895d96438cf60c1910c75a7559bcc5a`): the
script's `placeholder` mode, `--unicode-placeholder`, at 80x24, for a 60x40
RGB gradient made for it -- pixel `(x, y)` is `(4x, 6y, 3(x + y))` mod 256,
written as a PNG with zlib level 9 (sha256
`198fb2c1572a4da53a7140ba8dfcd0ac10954c199d5a1064707c2871a19389af`), no
one's picture. kitten sends `a=T,U=1,c=4,r=2,X=4` with the PNG and two rows
of four U+10EEEE cells, each with its row, column and id-byte diacritics.
`kittens_placeholders_place_the_picture` (`baud-render/tests/kitty_placeholder.rs`)
replays it. The image id is random, so a new run has other bytes.

The script checks each input against its sha256 first, and skips the
macOS file where it differs. Its `detect-*` sessions answer
kitten's queries as baud would and name a random temp file in a query, so
they have no fixed sha256.

## Provenance

`scripts/capture-tui.py` is how these were made and is kept for provenance, not
because CI runs it. **A capture is not reproducible**: btop draws live CPU
figures, so a second run of the same command produces different bytes and a
different hash. That is why these are committed rather than regenerated: a
recording that changes is a recording that is no longer the thing the
assertions were written against. `every_recording_renders_to_its_pinned_digest`
(`baud-core/tests/golden.rs`) pins what each `ascii/` and `utf8/` recording
renders to -- a digest of its final screen, the cells' content and resolved
rendition folded a word at a time with FNV-1a's constants, and its count of
non-blank cells, both in `baud-core/tests/golden/recordings.txt` -- so it sees
a change only where the change reaches the final screen. Each file's own length
and FNV-1a were pinned by `replay.rs`, which went with `baud-core2` in
`bcefaae`; nothing pins the bytes themselves now.

`btop.raw` is the first 512 KiB of a 2.48 MB capture. The whole thing was 10
seconds and 147,492 escape sequences; the tail is more of the same, and a
megabyte of it in git buys nothing a test can use.

They are fixtures, deliberately not entries in
`docs/research/sources/MANIFEST.toml`. That manifest is for *specifications*: a
claim about what a terminal must do traces to a primary source, and a capture
is evidence about what one program did once.
