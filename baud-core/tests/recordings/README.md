# recordings

Real escape streams, captured once from real programs through a real PTY, and
committed as fixed input.

Every payload baud had been tested and measured against before these was
written by the same people who wrote baud. `baud-bench/src/payload.rs` says so
in its own header: the synthetic payloads want validating against real capture
"before it is designed around". These are that capture.

| file | program | bytes | what it exercises that the others do not |
|---|---|---|---|
| `btop.raw` | btop, 45x160 | 524288 | truecolour `38;2`, box drawing, dense full-screen repaint, SGR mouse modes |
| `asciiquarium.raw` | asciiquarium, 45x160 | 335479 | curses animation: heavy cursor addressing with 16-colour SGR |
| `vim-unicode.raw` | vim on a generated UTF-8 file, paged with `^F`/`^B` | 138571 | scroll regions (DECSTBM), and CJK, emoji, ZWJ, RTL and combining marks |

`scripts/capture-tui.py` is how they were made and is kept for provenance, not
because CI runs it. **A capture is not reproducible**: btop draws live CPU
figures, so a second run of the same command produces different bytes and a
different hash. That is why these are committed rather than regenerated, and
why `replay.rs` pins each file's length and checksum -- a recording that
changes is a recording that is no longer the thing the assertions were written
against.

`btop.raw` is the first 512 KiB of a 2.48 MB capture. The whole thing was 10
seconds and 147,492 escape sequences; the tail is more of the same, and a
megabyte of it in git buys nothing a test can use.

They are fixtures, deliberately not entries in
`docs/research/sources/MANIFEST.toml`. That manifest is for *specifications*
that a claim can cite, with a `url` and a retrieval status. A recording is
neither retrieved nor something to cite -- it is input.
