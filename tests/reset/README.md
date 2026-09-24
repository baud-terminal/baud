# A real `reset(1)`

`seq-reset.raw` is what `reset(1)` sends a terminal, captured through a real
pty rather than written by hand: the byte shape a full reset arrives in, with
the output that came before and after it.

```sh
TERM=xterm-256color python3 scripts/capture-tui.py --seconds 3 --rows 20 \
  --cols 60 --out tests/reset/seq-reset.raw -- sh -c 'seq 1 45; reset; seq 101 105'
```

`reset` here is macOS's `/usr/bin/reset` (ncurses `tset`). What it sends after
the forty-five lines is the tab-stop reset (`CSI 3 g`, then `HTS` every eight
columns), `ESC c`, `DECSTR` (`CSI ! p`), `CSI ? 3 ; 4 l`, `CSI 4 l` and
`ESC >`: xterm-256color's `rs1` and `rs2`. Forty-five lines on twenty rows put
twenty-six in the history before the reset, and five after it put none.

`baud-core/tests/scrollback.rs` replays it at read sizes 1, 2, 3, 4, 5, 8, 17
and 4096 and requires the history empty and only the five later lines on the
screen. It first feeds everything before the `ESC c` and requires the
twenty-six lines of history the reset has to clear, so the capture cannot pass
by never having scrolled.
