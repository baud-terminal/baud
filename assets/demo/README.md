# The demo's films

Two films come from one program, `baud-app/examples/demo.rs`, which types a
real session into baud's window at 40 ms a key and records it.

| Piece | What it is | Made by |
|---|---|---|
| the tour | Take 1: baud in a real shell, the storyline's scenes 6 to 16, every command typed, the `#` lines typed at the prompt as narration. An MP4, silent or with keyboard clicks, uploaded to YouTube and not kept here. | `cargo run --release --example demo -- --record assets/demo/tour.mp4 --forbidden LIST --controls FILE` |
| the trailer | Take 2, `trailer.gif`, not filmed yet: baud plays a stretch of the tour from YouTube, at its own speed, in its own window with mpv and yt-dlp, about 20 s. The README's film. | `cargo run --release --example demo -- --trailer --tour URL --start T --end T --record assets/demo/trailer.gif --forbidden LIST --controls FILE`, the stretch from the README jumps to the dial-up lights read off the tour's take; the GIF must pass the WCAG 2.3.1 flash census or is removed |
| the UFO logo | The second logo film, `ufo-logo-dark.gif` and `ufo-logo-light.gif`: the ufo trail's saucer, in amber, threads the letters: it hovers behind the word, tilts and darts from gap to counter, in front of the letters and behind them, lights the `b` with its beam, beams the block up, streaks off left behind the word, comes back from the right and sets the block down. Its MP4s, `baud-pictures logo --ufo --mp4 --out DIR`. | `baud-pictures logo --ufo` |

Until the trailer is filmed the README shows no film, only its logo.

## What is here, and what appears after a take

- `README.md`, this file.
- `LICENSES.md`: each piece the films show, its licence, the page it was
  read at, and the credit lines for the video's descriptions.
- `ufo-logo-dark.gif` and `ufo-logo-light.gif`, the UFO logo, each with
  its `.provenance.toml`: the GIF's SHA-256, the flight's, the walls'
  pin, the encoder and its settings, and WCAG 2.3.1's census of the GIF.
  `baud-pictures logo --ufo --check` makes both again and compares them
  byte for byte; `baud-bench/tests/ufo_logo_flashes.rs` takes the census
  of the committed GIFs.
- `trailer.gif`, written by the trailer's take: the README's film.
- `trailer.gif.provenance.toml`, written by the same take: the commit
  filmed, every step with its time, each tool and pinned file. Checked by
  `cargo run --example demo -- --verify assets/demo/trailer.gif`.
- `tour.mp4.provenance.toml`, written by the tour's take: the commit filmed,
  that baud was built with `--features take`, the MP4's and its timeline's
  SHA-256, every step with its time, and the video's two descriptions, an
  end card naming the commit and the feature followed by the credit line,
  the clicks version's then the keyboard sounds' credit line.
  `tour.mp4` and `tour.timeline.tsv` are written beside it and ignored by
  git, as are its two versions.

No MP4 and no audio is committed here. The tour has no music and no sound
effect. It is delivered in two versions, and one is chosen after watching:
`scripts/demo-sound.sh assets/demo/tour.mp4 assets/demo/tour.timeline.tsv
PACK/` writes `tour.silent.mp4`, silent, and `tour.clicks.mp4`, a
keyboard's click under every key the timeline says was typed, from the
Mechvibes sound pack `LICENSES.md` names, which is fetched into PACK/ by
hand and never committed.

## The tour, scene by scene

The tour is the storyline's scenes 6 to 16; 1 to 5 are the README film's.

| # | Typed | Seen |
|---|---|---|
| 6 | `# now dial out. watch the lights.`, `ssh demo` | OH, then CD, then RD and SD with the data |
| 7 | `fastfetch` | the container's card, baud's mark as a picture |
| 8 | `# watch the cursor.`, `hx README.md`, the arrows down four and up two, `/character`, Ctrl-d three times, `ge`, `gg` | the plasma trail down and up; the README's jumps |
| 9 | `/Nix`, a Command-click on `https://nixos.org`, a short scroll, `q`, `:q` | nixos.org in Chawan, in a baud window of its own; back to helix, and helix closes |
| 10 | `# every language. right to left too.`, `cat hello.txt` | six languages and emoji |
| 11 | `# copy. paste.`, a drag over `copy`, Command-C, `# `, Command-V | the copy's words, then the word pasted at the prompt |
| 12 | `btop` | its CPU graphs |
| 13 | `# one more thing`, the fireworks' `printf` | iTerm2's fireworks, over ssh |
| 14 | `exit` | the badge clears, CD goes dark |
| 15 | `# the finale`, `sed -i '' s/plasma/ufo/ ~/.config/baud/config.toml`, `clear` | the trail turns to the saucer by live reload, and it flies the cursor to the top |
| 16 | `# that's baud. github.com/baud-terminal/baud` | held 2 s |

The steps themselves are `SESSION` in `baud-app/examples/demo/session.rs`,
and `trailer` there builds the trailer's.
