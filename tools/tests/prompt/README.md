# prompt

Real shell sessions captured through a pty, for the prompt on the
window's last row (`crates/baud-core/tests/prompt_at_bottom.rs`). Each shell ran
with a scratch `HOME` in a folder holding three empty files, `alpha`,
`beta` and `gamma`.

- bash 5.3 ran with `--noprofile --rcfile bashrc -i`; `bashrc` sets the
  `OSC 133` marks and nothing else, each ended by BEL.
- fish 4.9.3 (nixpkgs `d5cc6833999746cffdfe0598f7bc801a57b887a9`,
  `/nix/store/434074fbkb71qi6srp278cjlzainhfbi-fish-4.9.3`) ran as
  `fish -i`, reading `fish/config.fish` from `~/.config/fish`: no greeting,
  the prompt `demo> `, the title `demo` and no `OSC 7`. fish sends its own
  `OSC 133` marks, each ended by ST: `A;click_events=1`, `B`,
  `C;cmdline_url=...` and `D;<status>`.

| file | how | what |
|---|---|---|
| `bash-session` | the `capture-tui` example, `--rows 12 --cols 60`, with the keys `clear; ls`, `echo one`, Control-L, `echo two`, `exit` | a fresh prompt, a prompt after `clear`, Control-L's redraw |
| `bash-grow` | the `capture-resize` example through 60x12, 60x20, 80x20 | readline's redraw, its `A` included, after each `SIGWINCH` |
| `fish-session` | `capture-tui`, `--rows 12 --cols 60 --first-key 3 --key-delay 0.6`, with bash-session's keys | a fresh prompt, a prompt after `clear`, Control-L's two prompts |
| `fish-grow` | `capture-resize` through 60x12, 60x20, 80x20, its default timings | fish's redraw, `A` and `B`, after each `SIGWINCH` |

fish draws no prompt until its `DA1` is answered, so both fish captures
answered its queries with what baud's core answers (`Term::reply`), each
by `--answer`: `CSI ? u` with `CSI ? 0 u`, `CSI > 0 q` with
`DCS > | baud(1.0.0-unstable) ST`, each `XTGETTCAP` (`indn`,
`query-os-name`) with `DCS 0 + r ST`, `CSI 0 c` with `CSI ? 1 ; 2 c`, and
`CSI 6 n` with `CSI 1 ; 1 R`; its `OSC 11` got no answer, as the core
gives none. fish asks `CSI 6 n` after each command too, where the cursor
is not at row 1, so that reply is not the one baud would give; the
session captured with `CSI 7 ; 1 R` instead is the same bytes, so fish's
output here does not depend on it.

Each `.steps` file is `COLSxROWS <bytes>` lines, as in `tools/tests/resize/`.
