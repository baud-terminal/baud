# Colour-scheme captures

Real programs' output, captured through a real pty, for the `?2031` and
`CSI ? 996 n` tests in `baud-core/tests/color_scheme.rs`.

`tmux-start.raw` is tmux 3.7c starting with no configuration, 1940 bytes. With
`TERM` VT100-like it subscribes to palette notifications and asks the colour
preference of the outer terminal (`[[source: tmux-tty-c]]` 386-389):
`CSI ? 2031 h` at byte 91 and `CSI ? 996 n` at byte 99, then DA1 and DA2.

Captured on 2026-10-28 in the Linux container `tests/resize/README.md`
describes (host name `baud`, user `user`, programs from nixpkgs
`319e4cebef10cbb636c6ea392137b52ad105336a`), in `/home/user`, with
`SHELL` the zsh there and `ZDOTDIR` naming `tests/resize/zdotdir/`:

    TERM=xterm-256color capture-tui --seconds 4 \
        --rows 50 --cols 200 --out tmux-start.raw \
        -- tmux -L baudcap -f /dev/null new-session

`capture-tui` is the `baud-bench` example:
`cargo build --release -p baud-bench --example capture-tui` puts it in
`target/release/examples/`.

It is a recording of what tmux sent; nothing in it was edited. The capture
answers none of tmux's queries, so what tmux draws after them is what it
draws without replies: its status line and the shell's prompt. Up to DA2
its bytes are those of the macOS capture it replaces but for one
`CSI 22 ; 0 ; 0 t`, which the terminfo entry this tmux read sends on entering
the alternate screen.
