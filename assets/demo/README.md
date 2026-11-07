# The demo's films

Two films come from one program, `baud-app/examples/demo.rs`, which types a
real session into baud's window at 40 ms a key and records it.

| Piece | What it is | Made by |
|---|---|---|
| the tour | Take 1: baud in a real shell, seventeen scenes, every command typed, the `#` lines typed at the prompt as narration. An MP4 with sound, uploaded to YouTube and not kept here. | `cargo run --release --example demo -- --record assets/demo/tour.mp4 --forbidden LIST --controls FILE` |
| the trailer | Take 2, `trailer.gif`, not filmed yet: baud plays a stretch of the tour from YouTube, at its own speed, in its own window with mpv and yt-dlp, about 20 s. The README's film. | `cargo run --release --example demo -- --trailer --tour URL --start T --end T --record assets/demo/trailer.gif --forbidden LIST --controls FILE`, the stretch from the README jumps to the dial-up lights read off the tour's take; the GIF must pass the WCAG 2.3.1 flash census or is removed |
| the plasma logo | The README's logo film. mpv plays its MP4 in the tour's scene 10. | `baud-pictures logo`; its MP4, `baud-pictures logo --mp4 --out DIR`, which the demo runs into its own scratch directory |
| the UFO logo | The second logo film, which kitten draws in the tour's scene 9. Not made yet: the plasma logo's GIF stands in for it (`UFO_LOGO_FROM` in `baud-app/examples/demo/session.rs`). | to come |
| the photo | "Canazei Granite Ridges", elementary OS's wallpaper, CC0. Not stored here: the demo fetches it by the address in `photo.toml` and refuses it unless it hashes to the pin there. | `photo.toml` |
| the links | What the tour's scene 8 prints. | `links.txt` |

Until the trailer is filmed the README shows no film, only its logo.

## What is here, and what appears after a take

- `README.md`, this file.
- `LICENSES.md`: each piece the films show, its licence, the page it was
  read at, and the credit line for the video's description.
- `photo.toml`: the photo's address, hash, width and licence.
- `links.txt`: a web address and a file's, which the tour prints and points
  at.
- `trailer.gif`, written by the trailer's take: the README's film.
- `trailer.gif.provenance.toml`, written by the same take: the commit
  filmed, every step with its time, each tool and pinned file. Checked by
  `cargo run --example demo -- --verify assets/demo/trailer.gif`.
- `tour.mp4.provenance.toml`, written by the tour's take: the commit filmed,
  that baud was built with `--features take`, the MP4's and its timeline's
  SHA-256, every step with its time, and the video's description, an end
  card naming the commit and the feature followed by the credit line.
  `tour.mp4` and `tour.timeline.tsv` are written beside it and ignored by
  git.

No MP4 and no audio is committed here. The tour has no music. Its sound
effects are not chosen yet, the keys among them, which are to be recorded
from a real keyboard; `scripts/demo-sound.sh` mixes them into the tour's MP4
over silence, from a directory it is given, by the timeline's rows.

## The tour, scene by scene

| # | Typed | Seen |
|---|---|---|
| 1 | (baud opens) | the plasma splash carries the logo off; the nine lights in the title bar |
| 2 | `# are you ready for this?` | the opening line |
| 3 | `# baud's own config. watch the cursor.`, `hx ~/.config/baud/config.toml`, the arrows down four and up two, `ge` | each arrow leaves the plasma trail; the config shows `advertise_sixel = true` |
| 4 | `/plasma`, `cliquid`, Escape, `:w`, `gg`, `ge` | one word saved, baud reloads, the trail is liquid |
| 5 | `:open README.md`, `/character`, Ctrl-d three times, `ge`, `gg`, `:q` | the README read in helix |
| 6 | `# every language. right to left too.`, `cat hello.txt` | six languages and emoji |
| 7 | `# copy. paste.`, a drag over `copy`, Command-C, `# `, Command-V | the copy's words, then the word pasted at the prompt |
| 8 | `# links and files you can click`, `cat links.txt` | both underlined; the pointer over the web address draws its box, Command shows where it goes |
| 9 | `# pictures`, `kitten icat canazei.jpg`, `kitten icat ufo-logo.gif` | the photo, then the logo; FAX lit |
| 10 | `# and video`, `mpv logo.mp4` | the plasma logo as video |
| 11 | `# now dial out. watch the lights.`, `ssh demo` | OH, then CD, then RD and SD with the data |
| 12 | `fastfetch` | the container's card, baud's mark as a picture |
| 13 | `btop` | its CPU graphs |
| 14 | `# one more thing`, the fireworks' `printf` | iTerm2's fireworks, over ssh |
| 15 | `exit` | the badge clears, CD goes dark |
| 16 | `# the finale`, `sed -i '' s/liquid/ufo/ ~/.config/baud/config.toml`, `clear` | the trail turns to the saucer by live reload, and it flies the cursor to the top |
| 17 | `# that's baud. github.com/baud-terminal/baud` | held 2 s |

The steps themselves are `SESSION` in `baud-app/examples/demo/session.rs`,
and `trailer` there builds the trailer's.
