# Helix places its cursor and hides it

`helix-ge-gg.raw` is helix 25.07.1 at 200x50, opened on a file of two hundred
numbered lines with an empty config, and sent `g`, `e`, `g`, `g` and `:`, each
key a second after the one before:

```sh
seq -f 'line %03g' 1 200 > lines.txt
: > empty.toml
TERM=xterm-256color capture-tui --seconds 6.5 --rows 50 \
  --cols 200 --key-delay 1.0 --keys 'gegg:' --out helix-ge-gg.raw \
  -- hx --config empty.toml lines.txt
```

`capture-tui` is the `baud-bench` example:
`cargo build --release -p baud-bench --example capture-tui` puts it in
`target/release/examples/`.

Run from the directory holding `lines.txt`, so the status line names
`lines.txt` and not a path on the machine that captured it.

Helix draws every block cursor itself and hides the terminal's, but it still
moves the terminal cursor to the primary selection each frame and then hides
it (`draw` in helix's `helix-tui/src/terminal.rs`,
[[source: helix-tui-terminal]], pinned from helix's own tree at 25.07.1),
so its frames end `CUP` then `?25l`:
seven hides here, every one straight after a `CUP` -- to 1;8 at the first line,
to 43;8 after `ge` and back to 1;8 after `gg`. The `:` opens the command line
and shows the cursor at 50;2, which is the capture's one `?25h`. The capture
stops there with helix still running, as the recordings in
`tests/recordings/` stop mid-session.

`baud-core/tests/placed_cursor.rs` replays it at read sizes 1, 3 and 7 and
requires exactly that sequence of placements -- not at 4096, where the `gg`
frame and the `:` fall in one read and the placement is made and forgotten
inside it. Its corpus gate replays it at read sizes 1 and 4096 and pins the
trail it gains at each. The app's trail follows the placements: a hidden cursor a program placed is followed.

# Helix scrolled back by the wheel

`helix-wheel.raw` is helix 25.07.1 at 80x24 on the same two hundred lines and
the same empty config, sent `g`, `e`, `g`, `t` and then five wheel-up reports
at column 41, row 11 -- each written whole, 300 ms apart, the first key 2.5 s
in, once helix has waited out its start-up queries:

```sh
seq -f 'line %03g' 1 200 > lines.txt
: > empty.toml
W='\x1b[<64;41;11M'
TERM=xterm-256color capture-tui --seconds 5.5 --rows 24 \
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
120 Hz link, and requires that nothing drawn moves up once the wheel has begun: a landed flame waits where the chase rests, and helix's wheel no longer jumps it.

# Programs that scroll and carry the cursor

`scroll/` is thirteen sessions of real programs at 80x24, each a `.raw` of
everything the program wrote and a `.steps` log of when: `R <seconds> <bytes>`
for each read off the terminal device, `K <seconds> <label> <name> <hex>` for
each key written whole, and `A` for an answer sent back. The first line names
the program and its arguments. They were taken with a copy of
the `capture-tui` tool, then a script, that stamps each read and key with the time since the
fork, every program with `HOME` and the XDG directories in a scratch
directory, `TERM=xterm-256color`, from a directory holding `lines.txt` (two
hundred numbered lines, as above), `trail.rs` (`baud-app/src/trail.rs` as of
these captures, sha256
`5a33db16ecb06d69f31c37cff69e2d65fa0dc824974c7ce3cd83062d0044c714`), an empty `empty.toml`, `rel.toml`
(helix's relative numbers and cursor line) and a `tmux.conf` that turns the
mouse on, sets `screen-256color` and a 10 ms escape time and empties the
status line's right side.

The labels say what each key does to the cursor: `J` moves it to another
line (`ge`, `gg`, `G`, `10j`, a search, a page, a click), `S` scrolls the
view and keeps the cursor on its line (a wheel step, `zz`, `zt`, `zb`, `5
Ctrl-E`, `5 Ctrl-Y`), `P` is a wheel that vim was sent as arrow keys, and `N`
anything else. Keys are 450 ms apart and wheel steps 300 ms.

| file | program | keys |
|---|---|---|
| `helix-code`, `helix-rel` | helix 25.07.1 on `trail.rs`, `rel.toml` for the second | `ge`, `gt`, four wheel steps up and four down, `zz`, `zt`, `zb`, jumps, a click, two wheel steps, a click |
| `helix-num` | helix on `lines.txt` | the same, a search for its jumps |
| `vim-code`, `vim-num`, `vim-rnu` | vim 9.1, `-u NONE`, the mouse on; `rnu` with relative numbers and the cursor line | `G`, `H`, the wheel as helix's, `zz`, `zt`, `zb`, `5 Ctrl-E`, `5 Ctrl-Y`, jumps, pages, clicks |
| `nvim-code` | neovim 0.12.6, `--clean` | vim's keys |
| `vim-arrows` | vim with no mouse | `50G`, then the wheel as three arrow keys a step |
| `tmux-vim`, `tmux-split` | tmux 3.6a around vim; `split` with a second pane beside it | vim's keys, the wheel over vim's pane |
| `tmux-copy` | tmux's copy mode over `seq 1 400` | wheel steps, `g`, `G`, `k`, `H`, `L` |
| `less` | less 668, `--mouse`, on `trail.rs` | wheel steps, space, `b`, `G`, `g`, a search |
| `fzf` | fzf 0.74.1 over `seq 1 1000` | arrows, wheel steps, pages, typing |

`carried::captures` (`baud-app/src/carried.rs`) replays each at its read
times on a 120 Hz clock and pins what every move of the followed cursor does,
through the Linux driver's `plan` (`timed.rs`) and the macOS app's
`frame_trail`, for every trail: every jump trails, and every scroll that
carried the cursor with its line draws nothing, but for `zb` with the cursor
on the top line, which leaves too little of the old screen to read the
scroll from.

The `.raw` files are the bytes as captured, sha256:

| file | sha256 |
|---|---|
| `fzf.raw` | `80ccc66941de5dd335da1e06ab6b9aabaf5e1f90b784dc24d7ae2c55fe75c478` |
| `helix-code.raw` | `66ab4f5ee9aac05f131194f7b545c74199223affe9a2a75b5db66ffdc272d950` |
| `helix-num.raw` | `12670284a149f7c13f200f3f83bd355ca4661bdf58e35f22da422bb0cbf85683` |
| `helix-rel.raw` | `c0360b19c9c405f879910832208edb3a02ccf22ad20555001a65e94c4205455b` |
| `less.raw` | `c10bc92341ac6d82841bcceb4075166f906729f39b8aace7a6b37271e69f6549` |
| `nvim-code.raw` | `41d62dfcdc3a753dd3a5869a0167e4bf573f55815d1337339eed773d6a21c1d3` |
| `tmux-copy.raw` | `c95b91513c6480e9eaa3c10a7c7404e8c9d134ef27a11bbabb52bc9f82447847` |
| `tmux-split.raw` | `70d15c292978909652e1fd3d3ae9d86794756af1790fcfb1d932592e6ca8813f` |
| `tmux-vim.raw` | `8a255fb34814a504a75ac0a61c7941d98442b008184273c5630df33ee76f7167` |
| `vim-arrows.raw` | `89d6122452153d9477c1829dd139e9d93abde2dfa6aba2f19620216ffe6a1f8b` |
| `vim-code.raw` | `1c9da5decef53d6d68c78b726385b48fdb224f2a4cc2ebbcfbc8fa54c99b0127` |
| `vim-num.raw` | `788a330eae7c4acacb987985a64a8eb1b7fedb3be9ad966915e9c844c3786a48` |
| `vim-rnu.raw` | `d4e6375bdacaf8bc3840aed11f4599adc7e757b9bce90c4dfaa4b9eb29ddc30e` |
