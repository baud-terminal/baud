# Completion recordings

**Press Tab at a shell prompt and the list of completions stays on the
screen.** In bash, and in zsh when the list does not fit below the
prompt, the list was drawn and erased almost at once, and the prompt
jumped back to where it was before the list. These are the sessions
that showed it, captured from real shells through a real pty, and the
ones that never did, kept as controls. `baud-core/tests/completion_lists.rs`
replays each read by read, and `baud-render/tests/completion_lists.rs`
paints them.

## What happened

bash's readline prints the list under the line and then the prompt
again under the list. zsh does the same when the list is taller than the
room below its prompt, or after it asks "do you wish to see all"; when
the list fits it moves the cursor back up to its prompt instead. A shell
that marks its prompt with `OSC 133` sends the prompt's mark again with
it: the `A` of `bashrc`, or the `P;k=i` WezTerm's integration puts in
zsh's `PS1`. No `C` or `D` came between, so baud took that mark for the
shell redrawing its prompt after a resize -- which is what zsh does, by
moving the cursor up -- put the cursor back on the prompt's first row and
erased everything from there down: the list.

The semantic prompts proposal asks of `A` only "First do a fresh-line.
Then start a new command, and enter prompt mode" (`[[source:
semantic-prompts]]`). A shell that redraws moves the cursor back to its
prompt, or onto its input's line as readline does, before it marks; one
that prints a list has written below its input, and a mark there is the
prompt drawn again. baud now tells the two apart by where the cursor is:
past the end of the input's line, with no resize since the last mark,
nothing is erased. After a resize the next mark is the shell's redraw
wherever the cursor is, since readline redraws from the input's last
line, below any line break inside it. That needs the shell to mark where its input begins, `B` or
`I`, as every shell here does; one that sends only `A` keeps the old
reading.

| shell | gesture | before | now |
|---|---|---|---|
| bash 5.3, git's completion | `git log --s`, Tab Tab | list erased | kept |
| bash, `show-all-if-ambiguous` | `git log --s`, Tab | list erased | kept |
| bash, a full screen | `seq 30`, then the same | list erased | kept |
| zsh 5.9, compinit | `git log --s`, Tab, in 12 rows | list erased | kept |
| zsh | `git log --`, Tab, `y` to "see all 177" | list erased | kept |
| zsh | the list fits below the prompt | kept | kept |
| zsh, menu selection | `git log --s`, Tab Tab | kept | kept |
| zsh, the five-line prompt of `tests/resize/` | `git log --s`, Tab | kept | kept |
| fish 4.9 | `git log --s`, Tab: its pager | kept | kept |
| nushell 0.116 | `str `, Tab: its menu | kept | kept |

The gesture first reported, `git log-` and Tab, lists nothing in zsh with
either zsh's own `_git` or git's `git-completion.zsh` and no other
configuration: zsh rings the bell. `git log --s` is the nearest gesture
that lists, and the one every shell here was given.

## The files

Each recording is `<name>.raw`, the bytes exactly as the pty returned
them, and `<name>.steps`, written by `scripts/capture-tui.py --steps`:
one `COLSxROWS <bytes>` line per `read(2)`, and a `# sent` line for each
key before the first read after it. Nothing in a `.raw` was edited.

| recording | shell | window | marks | keys | bytes | reads |
|---|---|---|---|---|---|---|
| `bash-double` | bash | 80x24 | `bashrc`'s | `git log --s`, Tab, Tab | 408 | 16 |
| `bash-double-plain` | bash | 80x24 | none | the same | 366 | 14 |
| `bash-show-all` | bash, `BAUD_SHOWALL=1` | 80x24 | `bashrc`'s | `git log --s`, Tab | 407 | 14 |
| `bash-full` | bash | 80x24 | `bashrc`'s | `seq 30`, Enter, `git log --s`, Tab, Tab | 584 | 23 |
| `zsh-tall` | zsh | 80x12 | WezTerm's | `git log --s`, Tab | 1645 | 17 |
| `zsh-tall-plain` | zsh | 80x12 | none | the same | 1582 | 18 |
| `zsh-ask` | zsh | 80x24 | WezTerm's | `git log --`, Tab, `y` | 13067 | 38 |
| `zsh-fits` | zsh | 80x24 | WezTerm's | `git log --s`, Tab | 1624 | 17 |
| `zsh-menu` | zsh, `BAUD_MENU=1` | 80x24 | WezTerm's | `git log --s`, Tab, Tab | 3270 | 22 |
| `zsh-five` | zsh, `BAUD_FIVE` | 80x24 | WezTerm's | `git log --s`, Tab | 2010 | 18 |
| `fish` | fish | 80x24 | its own | `git log --s`, Tab | 1686 | 28 |
| `nushell` | nushell | 80x24 | its own | `str `, Tab | 3768 | 12 |

## How they were made

On 2026-10-28, on macOS, every program from nixpkgs
`58deea654ec5dd0644f31c840a07c29fa0592528`, aarch64-darwin: zsh 5.9.2,
bash 5.3p20, fish 4.9.3, nushell 0.116.1 and git 2.56.0, with git's
`contrib/completion/git-completion.bash` for bash and zsh's own `_git`
for zsh.

- The environment emptied (`env -i`) and then only `PATH` (git and
  `/usr/bin:/bin`), a scratch `HOME`, `USER=user`, `LOGNAME=user`,
  `LANG=C.UTF-8`, `TERM=xterm-256color`, `GIT_CONFIG_GLOBAL=/dev/null`,
  `GIT_CONFIG_NOSYSTEM=1`, the `XDG_` directories under that `HOME`, and
  each shell's switches from the tables.
- `~/repo`, the directory each shell started in, is a git repository on
  `main` with one empty commit, made with a fixed author and date, so its
  hash is `166f6e52`.
- zsh read `zdotdir/.zshrc` (`ZDOTDIR`); bash ran as
  `bash --noprofile --rcfile bashrc -i`; fish read `fish/config.fish`
  from `~/.config/fish`; nushell ran as
  `nu --config nu/config.nu --env-config nu/env.nu --no-history`.
- WezTerm's marks are its shell integration, `wezterm.sh`, at commit
  `76b606ec597a3c0263fa60321548637451c0a547`
  (sha256 `8635895db5fd8828e6f7654e015ddb8564ccec21179d2aca627e0329edf9b408`,
  as `tests/resize/README.md` fetches it), sourced unmodified through
  `WEZTERM_SH`, with `WEZTERM_SHELL_SKIP_USER_VARS=1` and
  `WEZTERM_SHELL_SKIP_CWD=1`, so no user, host or directory is sent. It
  is not committed. Its marks carry `aid=` and the shell's process id.
- `scripts/capture-tui.py --seconds 9 --cols 80 --key-delay 0.3`, with
  `--rows` from the table and 2 seconds before the first key for zsh, 3
  for the others.
- fish waits for its `DA1` before it draws a prompt, so its queries were
  answered with what baud's core answers (`Term::reply`): `CSI ? 0 u`,
  `DCS > | baud(1.0.0-beta.1) ST`, `DCS 0 + r ST` to each `XTGETTCAP`,
  `CSI ? 6 c`; its `OSC 11` got no answer, as the core gives none. fish
  and nushell ask for the cursor's position, and were told `CSI 1 ; 1 R`:
  the row is the prompt's in a fresh window, the column is the first
  whatever was typed. nushell's recording is a fresh window for that
  reason.
