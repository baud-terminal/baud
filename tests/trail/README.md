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
