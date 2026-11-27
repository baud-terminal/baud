# Palette captures

`ls-lsd-bat.raw` (254 bytes) is GNU `ls` 9.11, lsd 1.2.0 and bat 0.26.1 run
through a real pty on 2026-09-29, over a small directory: a directory `dir`,
a symlink `link` to `main.rs`, `main.rs`, `notes.txt`, and an executable
`run.sh`. It is what they sent; nothing was edited. `ls` colours from its
built-in defaults (`LS_COLORS` unset): `01;34` directory, `01;36` link,
`01;32` executable. lsd sends `38;5;4`/`6`/`2` with bold, and
`bat --theme=ansi --style=plain` the sixteen for Rust syntax. So every colour
in it comes from the terminal's sixteen, which is what
`crates/baud-render/tests/light_palette.rs` holds to 6:1 under the light theme.

Captured with (the fixture directory built as above, `F` its path):

    P=$(nix shell nixpkgs#coreutils nixpkgs#lsd nixpkgs#bat -c sh -c 'echo $PATH')
    TERM=xterm-256color PATH=$P target/release/examples/capture-tui --seconds 2 \
        --rows 50 --cols 200 --out tools/tests/palette/ls-lsd-bat.raw -- \
        sh -c "cd $F && unset LS_COLORS; ls --color=always; \
               lsd --color=always --icon=never; \
               bat --color=always --theme=ansi --style=plain --paging=never main.rs"

`cargo build --release -p baud-bench --example capture-tui` builds it.
