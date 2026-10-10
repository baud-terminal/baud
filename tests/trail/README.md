# Helix places its cursor and hides it

`helix-ge-gg.raw` is helix 25.07.1 at 200x50, opened on a file of two hundred
numbered lines with an empty config, and sent `g`, `e`, `g`, `g` and `:`, each
key a second after the one before:

```sh
python3 -c "open('lines.txt','w').write(''.join(f'line {i:03}\n' for i in range(1,201)))"
: > empty.toml
TERM=xterm-256color python3 scripts/capture-tui.py --seconds 6.5 --rows 50 \
  --cols 200 --key-delay 1.0 --keys 'gegg:' --out helix-ge-gg.raw \
  -- hx --config empty.toml lines.txt
```

Run from the directory holding `lines.txt`, so the status line names
`lines.txt` and not a path on the machine that captured it.

Helix draws every block cursor itself and hides the terminal's, but it still
moves the terminal cursor to the primary selection each frame and then hides
it (`draw` in `docs/research/sources/helix-tui-terminal.rs`, pinned from
helix's own tree at 25.07.1), so its frames end `CUP` then `?25l`:
seven hides here, every one straight after a `CUP` -- to 1;8 at the first line,
to 43;8 after `ge` and back to 1;8 after `gg`. The `:` opens the command line
and shows the cursor at 50;2, which is the capture's one `?25h`. The capture
stops there with helix still running, as the recordings in
`tests/recordings/` stop mid-session.

`baud-core/tests/placed_cursor.rs` replays it at read sizes 1, 3 and 7 and
requires exactly that sequence of placements -- not at 4096, where the `gg`
frame and the `:` fall in one read and the placement is made and forgotten
inside it. Its corpus gate replays it at read sizes 1 and 4096 and pins the
trail it gains at each. The app's trail follows the placements: see "A
hidden cursor a program placed is followed" in `CLAUDE.md`.

# Helix scrolled back by the wheel

`helix-wheel.raw` is helix 25.07.1 at 80x24 on the same two hundred lines and
the same empty config, sent `g`, `e`, `g`, `t` and then five wheel-up reports
at column 41, row 11 -- each written whole, 300 ms apart, the first key 2.5 s
in, once helix has waited out its start-up queries:

```sh
python3 -c "open('lines.txt','w').write(''.join(f'line {i:03}\n' for i in range(1,201)))"
: > empty.toml
W='\x1b[<64;41;11M'
TERM=xterm-256color python3 scripts/capture-tui.py --seconds 5.5 --rows 24 \
  --cols 80 --first-key 2.5 --key-delay 0.3 --key-sep '|' \
  --keys "g|e|g|t|$W|$W|$W|$W|$W" --out helix-wheel.raw \
  -- hx --config empty.toml lines.txt
```

`--key-sep` writes each report whole: a byte a delay, helix reads the
report's `ESC` as the Escape key. It was run with `HOME` and the XDG
directories pointed at a scratch directory, so helix's log went there.

`ge` puts the cursor on the last line and `gt` on the window's top line but
for helix's margin of five; each wheel line then scrolls the view three
lines up the file and carries the cursor, which keeps its line, three rows
down the screen, until the bottom margin holds it -- two rows on the last
step. Thirteen hides, each
straight after a `CUP`: 1;8 twice, 17;8 after `ge` and for the next two
frames, 6;8 after `gt`, then 9;8 twice, 12;8, 15;8 and 17;8 three times. No
`?25h`, and no synchronised update.

`a_wheel_scroll_back_in_helix_moves_every_trail_only_down`
(`baud-app/src/app.rs`) feeds it a helix frame -- the bytes up to each hide --
every 300 ms and draws every trail on every path through the app's chase on a
120 Hz link, and requires that nothing drawn moves up once the wheel has
begun: see "A landed flame waits where the chase rests, and helix's wheel no
longer jumps it" in `docs/record.md`.
