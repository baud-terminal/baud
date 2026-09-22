# Text-drawn image previews, as a file manager sends them

What yazi sends a terminal that it gives no image protocol: chafa's
`symbols` output. These are **real captures of chafa's standard output**,
not a shape written to look like one. The earlier fixture here claimed
yazi drew upper half blocks (`U+2580`). That shape was assumed rather
than captured, and it is wrong.

| file | chafa's environment | cells | what they are |
|---|---|---|---|
| `chafa-truecolour.ans` | `TERM=xterm-256color COLORTERM=truecolor TERM_PROGRAM=baud`, which is baud's child environment | 41x28 | 803 block elements, 314 spaces, 31 box-drawing strokes; `38;2`/`48;2` truecolour |
| `chafa-240.ans` | the same with `TERM_PROGRAM=Apple_Terminal` | 41x28 | 661 spaces, 485 block elements, 2 box-drawing; `38;5`/`48;5` |

The second file is what baud received on 2026-09-22, while it passed its
parent's `TERM_PROGRAM` through to the shell. chafa picks 240 colours
when it believes it is in Apple's Terminal. Every cell in both files
carries an explicit foreground *and* background. A flat area is a space
on a set background, which is exactly what a TUI panel is too.

## How they were made

The input is synthetic: nobody's photo is committed. It is a seeded
plasma over a flat band with a disc, so the output carries all three cell
classes chafa draws.

```sh
magick -seed 22 -size 600x520 plasma:fractal -blur 0x2 \
  \( -size 600x280 xc:'#2f4f6f' -fill '#d0b070' -draw 'circle 420,140 420,60' \) \
  -append src.png
# yazi's own thumbnail step, argv from its magick previewer
magick -limit thread 1 -limit memory 536870912 -limit disk 1MiB \
  -limit width 10000 -limit height 10000 src.png -auto-orient -strip \
  -filter triangle -thumbnail '600x900>' -quality 75 JPG:thumb.jpg
# yazi 26.5.6's chafa argv, yazi-adapter/src/drivers/chafa.rs
env -i HOME=$HOME PATH=/usr/bin:/bin TERM=xterm-256color COLORTERM=truecolor \
  TERM_PROGRAM=baud chafa -f symbols --relative off --probe off --polite on \
  --passthrough none --animate off --view-size 41x29 thumb.jpg
```

Captured with ImageMagick 7.1.2-27 and chafa 1.18.2. The md5 digests are
`6570c8b772291a27e928ed6ab8161cb0` for the truecolour file and
`d5752fd4058eb812bd282e0cdfd90f39` for the 240-colour one.

## How yazi puts them on the screen

yazi does not pass chafa's output through as one write. It splits it on
`\n` and writes each line at the pane's origin, one row down each time,
with a cursor move between lines (crossterm `MoveTo`, which is `CUP`).
A test that feeds these files feeds them that way:
`baud-core/tests/image_preview.rs` and `baud-render/tests/translucent.rs`.
