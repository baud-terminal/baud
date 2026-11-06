# prompt

Real bash 5.3 sessions captured through a pty, for the prompt on the
window's last row (`baud-core/tests/prompt_at_bottom.rs`). The shell ran
with `--noprofile --rcfile bashrc -i`, a scratch `HOME` and a folder
holding three empty files, `alpha`, `beta` and `gamma`; `bashrc` sets the
`OSC 133` marks and nothing else.

| file | how | what |
|---|---|---|
| `bash-session` | the `capture-tui` example, `--rows 12 --cols 60`, with the keys `clear; ls`, `echo one`, Control-L, `echo two`, `exit` | a fresh prompt, a prompt after `clear`, Control-L's redraw |
| `bash-grow` | the `capture-resize` example through 60x12, 60x20, 80x20 | readline's redraw, its `A` included, after each `SIGWINCH` |

Each `.steps` file is `COLSxROWS <bytes>` lines, as in `tests/resize/`.
