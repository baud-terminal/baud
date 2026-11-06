# Resize recordings

A real `zsh` resized through a real pty: the bytes it sent, and the window
size in force when each of them arrived. `baud-core/tests/resize_recorded.rs`,
`resize_measure.rs` and `prompt_at_bottom.rs` replay them, and the `winch`,
`drag` and `flash_screens` benches read them.

Each recording is two files written by the `capture-resize` example:
`<name>.raw`, the bytes exactly as the pty returned them, and
`<name>.steps`, one `COLSxROWS <bytes>` line per step. `<name>.sizes` is the
gesture the tool was given. Nothing in a `.raw` was edited.

| recording | started in | before the first prompt | gesture | steps | bytes |
|---|---|---|---|---|---|
| `drag` | `~/work/baud` | a full screen, `zdotdir/banner` | 329 sizes from 111x33, between 72 and 132 columns and 3 and 54 rows | 331 | 159824 |
| `short` | `~/work/baud` | the same | 80x20 down to 45x4 and up to 100x30, three times, the first narrowing a burst | 26 | 12730 |
| `zoom` | `~/work/baud` | the same | 90x26 down to 44x12 and up to 111x32, three times, the first narrowing a burst | 26 | 13280 |
| `fresh-drag` | `/tmp` | one line, `zdotdir/banner-fresh` | `drag`'s sizes | 331 | 156248 |
| `fresh-zoom` | `/tmp` | the same | `zoom`'s sizes, one a line | 26 | 9704 |
| `fresh-burst` | `/tmp` | the same | Enter twice, a drag in and out as two bursts, Enter, a deeper drag in and out, Enter | 43 | 15463 |
| `live-drag` | `~/work/baud` | nothing | Enter four times, then a recorded drag's 512 size changes as bursts | 519 | 231204 |

## How they were made

On 2026-10-28, in a Linux container of its own, so that the host name the
shell reports is `baud` and the user is `user`:

- colima 0.10.3 on macOS, a VM of its own (aarch64), and the
  `nixos/nix@sha256:f159a58f390677f41e8e900a74b6e2f9c7f27b364122ae6acf7a652ce6c551c0`
  image run with `--hostname baud`, with a `user` account (uid 1000, home
  `/home/user`) added to its `/etc/passwd`.
- Every program from nixpkgs `319e4cebef10cbb636c6ea392137b52ad105336a`,
  aarch64-linux: zsh 5.9.2, Python 3.14.7, git 2.55.0, coreutils 9.11 and
  ncurses 6.6's terminfo. Linux 6.8.0.
- The environment emptied (`env -i`) and then only `PATH` (those programs),
  `HOME=/home/user`, `USER=user`, `LOGNAME=user`, `HOSTNAME=baud`,
  `SHELL` (that zsh), `LANG=C.UTF-8`, `TERM=xterm-256color`,
  `TERMINFO_DIRS`, `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`,
  `ZDOTDIR` naming this directory's `zdotdir/`, and `WEZTERM_SH`.
- `~/work/baud` is a git repository with one commit, made with a fixed
  author and date, so the prompt's branch and hash are `master` and
  `b4dd72fc`.
- `WEZTERM_SH` is WezTerm's shell integration, `wezterm.sh`, at commit
  `76b606ec597a3c0263fa60321548637451c0a547`, fetched from
  `https://raw.githubusercontent.com/wezterm/wezterm/76b606ec597a3c0263fa60321548637451c0a547/assets/shell-integration/wezterm.sh`
  (sha256 `8635895db5fd8828e6f7654e015ddb8564ccec21179d2aca627e0329edf9b408`)
  and sourced unmodified. It is not committed. It sends the `OSC 7` and
  `OSC 133` marks the tests read; `zdotdir/.zshrc` turns its user variables
  off.

Each recording, from the directory in the table, with `BAUD_BANNER` naming
the banner file in the table (unset for `live-drag`):

    capture-resize --sizes tests/resize/<name>.sizes \
        --out <name> -- zsh -i

`capture-resize` is the `baud-bench` example:
`cargo build --release -p baud-bench --example capture-resize` puts it in
`target/release/examples/`.

`capture-resize`'s defaults were used: 2.5 seconds before the first
size, 0.05 a step, 0.02 between the sizes of a burst, 2 after the last.

## The shell

`zdotdir/.zshrc` draws a five-row prompt: an empty row, three rows padded
to the window's width, so each wraps when the window narrows, and the input
line. The padding is expanded from `$COLUMNS` each time zsh draws the
prompt, so a resize redraws it at the new width: zsh moves up to the
prompt's first row (`CUU 4`), erases to the end of the screen and redraws.
The glyphs are Nerd Font private-use code points. At start-up the shell
prints the file `BAUD_BANNER` names: a screen and more of text for the
three recordings that begin with output, one line for the fresh ones.

## The gestures

The size sequences are those of the recordings these replace, which the
`.sizes` files reproduce; each new `.steps` has the same size column as
the one it replaces, and `live-drag`'s the same size changes. The byte
column differs: how a shell's output splits between steps depends on how
fast it redraws.

- `drag`, `fresh-drag` and `fresh-zoom`: one size a line. `drag`'s 329
  sizes are what `BAUD_DEBUG_RESIZE=1` printed while a window was dragged.
- `short` and `zoom`: one size a line but the first narrowing, `60x10 50x6`
  and `70x20 55x16`, which is a burst. They are kept for two of the
  shell's redraws arriving in one step, the first computed for a width the
  screen has left. The recordings they replace reached that at step 3
  because the shell had not answered the first size before the second
  came; a burst reaches it whatever the shell's speed, and the size column
  is the same. `zoom_and_short_each_carry_two_redraws_in_one_step` checks
  it.
- `fresh-burst`: several sizes on a line are a burst, set a tick apart with
  nothing read between them, as the app delivers a fast drag; `enter`
  presses Enter. A line repeating the size in force changes nothing, since
  the kernel signals only a change, and is a pause of one step.
- `live-drag`: the app's `BAUD_TRACE` of a session recorded a resize as a
  step with no bytes. Each run of those is one burst here, with sizes
  repeated in a run dropped, after four Enters at the first size.
