# Colour-scheme captures

Real programs' output, captured through a real pty, for the `?2031` and
`CSI ? 996 n` tests in `baud-core/tests/color_scheme.rs`.

`tmux-start.raw` is tmux 3.7c starting with no configuration, 4907 bytes. With
`TERM` VT100-like it subscribes to palette notifications and asks the colour
preference of the outer terminal (`[[source: tmux-tty-c]]` 386-389):
`CSI ? 2031 h` at byte 82 and `CSI ? 996 n` at byte 90, then DA1 and DA2.
Captured on 2026-09-29 with:

    T=$(nix shell nixpkgs#tmux -c sh -c 'command -v tmux')
    TERM=xterm-256color python3 scripts/capture-tui.py --seconds 4 \
        --rows 50 --cols 200 --out tests/scheme/tmux-start.raw \
        -- $T -L baudcap -f /dev/null new-session

It is a recording of what tmux sent; nothing in it was edited. The capture
answers none of tmux's queries, so what tmux draws after them is what it
draws without replies.
