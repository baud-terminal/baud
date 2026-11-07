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
| `ascii/cat-scroll.raw` | `cat` of 437 KB of Rust source, 50x200 | 448460 | 0.1% | 0 | many | **scrolling**, and text with no escapes at all |
| `ascii/git-log.raw` | `git --no-pager log --stat --color`, 50x200 | 501185 | 0.1% | 0 | many | colour over scrolling text, an SGR every 141 bytes |
| `ascii/less-pager.raw` | `less -R` paged with space/`b`/`G`/`g` | 30146 | 0.1% | 0 | 5104 | full-screen repaint in the **alternate screen** |
| `ascii/pipes.raw` | pipes.sh 1.3.0, 45x160 | 536908 | 0.0% | 0 | 0 | an animation: a cursor move, a bold and a colour `SGR` before every character, an `SGR` every 9.1 bytes |
| `utf8/btop.raw` | btop 1.4.7, cpu and mem boxes, 45x160 | 459870 | 21.5% | **0** | 0 | truecolour `38;2`, box drawing, braille graphs, dense full-screen repaint, SGR mouse modes |
| `utf8/vim-unicode.raw` | vim on a generated UTF-8 file, paged with `^F`/`^B`, 45x160 | 138583 | 28.2% | 3740 | 0 | scroll regions (DECSTBM), CJK, emoji, ZWJ, RTL, combining marks |
| `utf8/cat-cjk.raw` | `cat` of `cjk-corpus.txt` x3, 50x200 | 327351 | 81.2% | 69741 | 125951 | **wide characters while scrolling**, with no escapes at all |
| `utf8/less-cjk.raw` | `less -R` over the same, paged both ways | 84473 | 76.3% | 17002 | 30930 | wide characters scrolling in the **alternate screen** |

## Why the CJK two exist

**Counting non-ASCII bytes against wide codepoints split what looked like one
axis into two.** btop is 21.5% multi-byte and has **zero wide codepoints** --
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

yazi 26.5.6's output, 2,804,600 bytes (sha256
`a6ef7cade8cb52f13aa776343fc4e3a588ac631f80435630862ca1dc4801f599`), as
the `capture-yazi-walk` example's pty received it at 120x40: the cursor
stepped down six files with `j` and back six with `k` over the seven files
the `gen-yazi-files` example writes -- `photo.jpg`, `flat.png`, `alpha.png`,
`screenshot.png`, `small.png`, `sparse.pdf` and `text.pdf`. Thirteen
previews, each a Sixel `ESC P 9;1 q "1;1;W;H` -- `P2 = 1`, so an unset bit
keeps what is under the image -- cleared with spaces and the next drawn at
the same cell. The replies it was sent are not in the file; one gave the
cell as 17x34 (`CSI 6 ; 34 ; 17 t`). The first screen shows `~/files`.
It is committed whole because a recording is not edited before it is
measured, and because `every_preview_in_a_yazi_walk_is_its_own_body_alone`
(`baud-core/tests/sixel_reuse.rs`) needs both directions of the walk: it
requires preview k to equal preview 14 - k as well as each preview to equal
its body decoded alone. Its thirteen Sixel bodies are byte for byte those of
the capture of 2026-10-09 it replaces; only the text around them moved.

## kitty graphics: regenerated, not committed

The `record-kitten-icat` example records what `kitten icat` 0.49.2 writes,
`--transfer-mode=stream`, to a pty carrying the app's pixels (17x34 a
cell), for note V's V19 step 0. Unlike the captures
above, these regenerate: kitty and the two nixos-artwork pictures are
pinned by nixpkgs revision, and two runs gave the same bytes. None is
committed. `mosaic-blue` and `waterfall` are CC-BY-SA-4.0 and `skyglow` is
Apple's, and whether a recording that carries one belongs in this repository is open. kitten fits a picture to the window's
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
tool's `placeholder` mode, `--unicode-placeholder`, at 80x24, for a 60x40
RGB gradient made for it -- pixel `(x, y)` is `(4x, 6y, 3(x + y))` mod 256,
written as a PNG with zlib level 9 (sha256
`198fb2c1572a4da53a7140ba8dfcd0ac10954c199d5a1064707c2871a19389af`), no
one's picture. kitten sends `a=T,U=1,c=4,r=2,X=4` with the PNG and two rows
of four U+10EEEE cells, each with its row, column and id-byte diacritics.
`kittens_placeholders_place_the_picture` (`baud-render/tests/kitty_placeholder.rs`)
replays it. The image id is random, so a new run has other bytes.

The tool checks each input against its sha256 first, and skips the
macOS file where it differs. Its `detect-*` sessions answer
kitten's queries as baud would and name a random temp file in a query, so
they have no fixed sha256.

## `kitty/icat-logo-dark-140.raw`

kitty 0.50.0's `kitten icat` (nixpkgs
`4fe25e12eb9f4c744966dd3de5a05246cf098601`) showing a copy of README's logo
film made 140 pixels wide, 612,266 bytes (sha256
`55c6aa70a8e112ef6b0796b81e255736e22c6257a0cc0c1086e04408946ce551`), as a pty
of 120x40 cells and 1680x1280 pixels received it, a 14x32 cell, the film
harness's terminal at `font_size = 12` (`baud-app/src/film.rs`), recorded
by a harness written against that terminal for this capture and not in the
tree. kitten ran from a directory holding only the copy:

    kitten icat --transfer-mode=stream logo-dark-140.gif

Nothing was written back: the `a=T` and every `a=f` carry `q=2`, and `a=a`
answers only an error. kitten sends the first frame as `a=T,f=24,o=z`,
140x97 RGB, then each of the other 319 as an `a=f,o=z` over the frame
before (`c=` the previous number), RGBA, of the rectangle the GIF's graphic
covers at `x`, `y`, its alpha 0 or 255, gaps of 30 and 40 ms, and `a=a` with
`v=1`, then `s=2`, then after the last frame `s=3`. The image number `I` is
random: run again, the stream is the same but for it.

The copy, `logo-dark-140.gif` (244,014 bytes, sha256
`4cd8e634f526f901ad2b55671b8c2ced7a59d2e841182f3a71103ccc0290cd9c`), is
`assets/readme/logo-dark.gif` (sha256
`9fbfc879b07ddaa9ccd0d22016556e147e97f30539c14901aef21324604ec326`) as the
same nixpkgs' ffmpeg 9.0.2 made it, its 320 frames:

    ffmpeg -i assets/readme/logo-dark.gif \
      -vf "scale=140:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=diff_mode=rectangle" \
      -bitexact -map_metadata -1 logo-dark-140.gif

Run again, the command gives these bytes, so the copy is not committed.
`baud-core/tests/kitty_records.rs` replays the capture in 1 KiB reads.
`kitten icat --transfer-mode=stream logo-dark.gif`, the film itself at
840x580, sent 7,873,589 bytes the same way (sha256
`4451c2eef00c68e4dafbb501418f5d6768343dd6dac33eaf64b7a1c66125117e`), which
are not committed; `MEASUREMENTS.md` reads its size and its advance.

## Provenance

The `capture-tui` example (`baud-bench/src/capture/tui.rs`) is how these were
made, a Python script then, and is kept for provenance, not because a gate
runs it. **A capture is not reproducible**: btop draws live CPU
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
`2538310`; nothing pins the bytes themselves now.

## How the 2026-10-28 captures were made

`btop.raw`, `vim-unicode.raw`, `pipes.raw` and `yazi-walk.raw` were captured
again on 2026-10-28 on macOS 27.0 (Apple silicon), each with its whole output
committed, `HOME` and every XDG directory in a scratch folder and the
environment emptied but for `PATH=/usr/bin:/bin`, `TERM=xterm-256color` and
`LANG=en_US.UTF-8`:

- **btop** 1.4.7, nixpkgs `319e4cebef10cbb636c6ea392137b52ad105336a`, 45
  seconds at 45x160:
  `capture-tui --seconds 45 --rows 45 --cols 160 -- btop -c btop.conf`,
  where `btop.conf` is `btop --default-config` with three keys changed:
  `shown_boxes = "cpu mem"`, `disks_filter = "/"` and
  `show_battery = false`. btop's README gives the first: "Manually set which
  boxes to show. Available values are "cpu mem net proc" and "gpu0" through
  "gpu5"" (`[[source: btop-readme-147]]`). Without the process box, which
  lists every user's commands, and the net box, which shows the address.
- **vim** 9.1 (`/usr/bin/vim`, patches 1-1752, no vimrc in `HOME`) on
  `unicode.txt`, written by the `gen-vim-unicode` example, opened by that
  relative name from its own directory, at 45x160, sent `^F` twelve times,
  `^B` twice and `^F` twice, 0.35 s apart:
  `capture-tui --seconds 9 --rows 45 --cols 160 --keys <those> -- vim unicode.txt`.
  Run again on this machine with the example, this gives the file back byte
  for byte (`MEASUREMENTS.md`, "The capture tools are examples").
- **pipes.sh** 1.3.0 (MIT), the same nixpkgs, 200 seconds at 45x160:
  `capture-tui --seconds 200 --rows 45 --cols 160 -- pipes.sh -t 4 -t 5 -t 8 -p 4 -f 100 -r 0`:
  its three ASCII pipe sets, four pipes, a hundred frames a second, never
  reset. What it draws is its own pipe characters, so the capture carries
  no one's art.
- **yazi** 26.5.6 (nixpkgs' build of 2026-05-05, with its own `file`,
  poppler and ImageMagick on its `PATH`), started in `~/files`:
  `capture-yazi-walk --cols 120 --out yazi-walk.raw -- yazi`.

They are fixtures, deliberately not entries in
`docs/research/sources/MANIFEST.toml`. That manifest is for *specifications*: a
claim about what a terminal must do traces to a primary source, and a capture
is evidence about what one program did once.
