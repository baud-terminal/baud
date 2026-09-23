# Themed editors, as they draw their panels

Real captures of three full-screen editors under colour schemes that set a
background on every cell. They are the false-positive half of the gate for
`[glass] pictures`: `baud-render/tests/pictures.rs` feeds each one a read at
a time and requires that no read leaves a picture on the screen. A picture
there is a connected block of set backgrounds holding sixteen colours or
more (`baud_render::PICTURE_COLOURS`).

They are not in `tests/recordings/`. That corpus is the throughput corpus,
its count is gated, and these are here only for what their panels look like.

| file | what it draws | most distinct backgrounds in one block, at any read |
|---|---|---|
| `vimdiff.raw` | vim's diff mode over two of baud's own sources, `habamax`, cursorline and numbers | **8**, the busiest block measured |
| `helix.raw` | helix editing `baud-core/src/prompt.rs` under its default theme, with a few edits | 5 |
| `vimhl.raw` | vim under `desert` with a search highlighted and a visual selection | 3 |

Every screen shows this repository's own source and nothing else. Two other
captures taken the same afternoon, of `mc` and of `delta`, were measured (3
and 2) and not committed, because they show a home directory's paths.

## How they were made

With `scripts/capture-tui.py` at the corpus geometry, 200x50, six seconds
each, the listed keys typed in after the program starts:

```sh
cap() { n=$1; shift; secs=$1; shift; keys=$1; shift
  python3 scripts/capture-tui.py --seconds $secs --rows 50 --cols 200 \
    --keys "$keys" --out tests/panels/$n.raw -- "$@"; }
cap helix 6 'jjjjjxxxjjj' env TERM=xterm-256color COLORTERM=truecolor \
  hx baud-core/src/prompt.rs
cap vimdiff 6 'jjjjj' env TERM=xterm-256color vim -u NONE -c 'syntax on' \
  -c 'colorscheme habamax' -c 'set cursorline number' \
  -d baud-core/src/prompt.rs baud-core/src/sixel.rs
cap vimhl 6 'jjjjjVjjj' env TERM=xterm-256color vim -u NONE -c 'syntax on' \
  -c 'colorscheme desert' -c 'set cursorline number hlsearch' -c '/self' \
  baud-core/src/prompt.rs
```

Taken on 2026-09-22 with helix 25.07.1 and vim 9.1. The md5 digests are
`e2e62b524db392f729d07732f0fcdc1a` for `helix.raw`,
`fa4f5b8786224de372875557198a4ada` for `vimdiff.raw` and
`0614ba9af7e032fe5bf23e66c6ec4ce0` for `vimhl.raw`.

Like the recordings, they stop mid-session: the capture ends while the
editor is still drawing, so the alternate screen is never left.
