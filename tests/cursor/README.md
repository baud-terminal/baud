# vim and helix, from start to quit

Two captures of a program starting on the alternate screen and quitting, for
what they do to the cursor's blink. Both were taken on macOS 27 at 200x50,
opened on a file of two hundred numbered lines, and sent `:`, `q` and Return:

```sh
seq -f 'line %03g' 1 200 > lines.txt
: > empty.toml
TERM=xterm-256color capture-tui --seconds 8 --rows 50 \
  --cols 200 --key-delay 1.5 --keys ':q\r' --out vim-exit.raw \
  -- vim -u NONE -N lines.txt
TERM=xterm-256color capture-tui --seconds 10 --rows 50 \
  --cols 200 --key-delay 2.0 --keys ':q\r' --out helix-exit.raw \
  -- hx --config empty.toml lines.txt
```

`capture-tui` is the `baud-bench` example:
`cargo build --release -p baud-bench --example capture-tui` puts it in
`target/release/examples/`.

Run from the directory holding `lines.txt`, so the status lines name
`lines.txt` and not a path on the machine that captured them.

`vim-exit.raw` is vim 9.1 (`/usr/bin/vim`). It sends `?1049h`, then
`?12h ?12l` at bytes 45 and 51 -- vim splits `cnorm` into `t_vs` and `t_VS`
and nudges the blink at start -- and at exit `?1049l ?25h`, with no `?12`.

`helix-exit.raw` is helix 25.07.1. Its exit writes `CSI 0 SP q`, then the
terminfo's `Se`, then `cnorm`, all before it leaves the alternate screen
(`reset_cursor_approach` in helix's `helix-tui/src/backend/crossterm.rs`,
[[source: helix-tui-crossterm]]):
`0 q 2 q ?12l ?25h`, after the last frame's own `?25h 2 q`, then `?1049l`.
**Those bytes depend on the system's `xterm-256color`**: this machine's
`infocmp -x xterm-256color` gives
`Se=\E[2 q` and `cnorm=\E[?12l\E[?25h`, and an entry with another `Se` would
send another reset.

Nothing answers the programs' queries: `capture-tui` answers none, so
helix waits out its keyboard-enhancement query before it draws, and the
captures carry the queries unanswered.

`baud-core/tests/cursor_blink_rule.rs` replays both at read sizes 1, 3, 7 and
whole, and requires the configured cursor back after each exit, and vim's
session blinking.
