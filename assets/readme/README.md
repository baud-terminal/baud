# The README's pictures

`logo-dark.gif` and `logo-light.gif` are the README's header: the
lockup the launch splash holds, as the logo files draw it, drawn by
baud's CPU renderer at twice its 300 units on GitHub's own page colour,
#0d1117 for the dark theme and #ffffff for the light, 840 by 580 and
shown 420 wide, so the lockup is its 300. Only the block cursor moves as drawn; in the encoded file gifski's palette, picked frame by frame, also moves at most 528 glyph-edge pixels by at most 27 levels across the first three frame changes, and none after them:
it blinks as baud's cursor blinks by default, each phase asked of the
app's own caret and drawn by the cursor's own drawing
(`baud-app/src/logo_film.rs`), so the logo follows the default blink.

At the default, 250 ms shown and 250 hidden with nothing drawn in the
hidden phase, each file is nineteen frames of 25 hundredths of a second:
shown and hidden by turns nine times, 4.5 s of blinking, then a
nineteenth frame with the cursor shown, which the file rests on. It
plays once: it carries no loop block. The blink is the most whole
cycles that end before five seconds, because WCAG 2.2's 2.2.2 Pause,
Stop, Hide (Level A) covers blinking that starts automatically, lasts
more than five seconds and is presented in parallel with other content,
and asks for a way to pause, stop or hide it ([[source: wcag22]]); a
README carries no control of its own, so the logo stops by itself
instead. It does not rely on GitHub's own setting to pause animated
images.

| half | bytes | frames | blink, read off the decoded file | most flashes at a pixel in a second, general / red |
|---|---|---|---|---|
| dark | 10,902 | 19 | 9 x (250 ms shown, 250 hidden), then shown | 2 / 0 |
| light | 11,372 | 19 | 9 x (250 ms shown, 250 hidden), then shown | 2 / 0 |

Two tests hold the committed files, each decoding them with baud's own
GIF decoder: `logo_film::tests::the_readme_logos_blink_the_default_and_rest_shown`
(`baud-app`) requires that outside the cursor no frame differs from
the first beyond the encoder's palette, reads shown and hidden off the
block's pixels, and requires the default's phases, under five seconds of
blinking and a last frame that is the first, the cursor shown; `baud-bench/tests/readme_logo_flashes.rs` takes
WCAG 2.3.1's census of the frames at 100 a second and 2 pixels a point,
the last held 3 s, and requires both verdicts to pass and no pixel to
flash more than three times in a second. `scripts/make-readme-logo.py`
films both halves, encodes them with gifski 1.34.0 at quality 80 from
`readme-film.sh`'s nixpkgs, Python and Pillow from
`make-brand-pictures.py`'s, checks gifski writes the same bytes twice,
writes the files and runs both tests; with `--check` it compares the
files byte for byte instead of writing them.

`hero.gif` is a screen recording of a real baud session, its window filmed
as it ran: a shell, every command typed into it at real speed, glass and
smooth scrolling as they are, at 110x33 cells and 12 pt, in Dark. It shows
the launch logo carried off by the plasma, btop (refreshing every 100 ms,
its process and network boxes hidden), `cat README.md` and a scroll back
through it, helix finding the tagline and jumping to the end and back with
the plasma trail, a photo drawn by `kitten icat`, neo, asciiquarium, and
`clear` sending the cursor to the top with the trail. The shell's prompt is
a plain `baud ❯`, naming no user or host, but btop's two boxes, about 1 to
4 s in, name the machine: the chip over the core list, the memory's size
and the GPU's memory in use of it, the disks with their sizes and volume names, the
uptime and the load average, with the battery's charge, the time and each
core's load and temperature. It is to be filmed again. The session's btop
(`baud-app/examples/demo/scratch.rs`) now draws its cpu box alone, titled
`CPU`, with no GPU line and no uptime; of that list a new take still shows
the load average, which no key of btop 1.4.7 removes
(`[[source: btop-draw-147]]`), the battery, the time and the cores.
Encoded with gifski at 1164 px wide (16.9 MB), with no frame carrying
traces of another, so it loops clean. neo's katakana are drawn by the
macOS system font baud falls back to, as baud's own font has none; a
recording played with the example draws them from Noto Sans CJK SC,
which it configures.

The next take differs from this GIF in what it shows and in how it is
filmed. pipes-rs 1.6.4 plays where asciiquarium did: asciiquarium's fish
and sea are Joan Stark's art (`[[source: asciiquarium-1-1]]`), which she
gives "for personal use only" (`[[source: jgs-guidelines]]`), so a GIF
of them is not hers to share. pipes-rs is under the Blue Oak Model
License 1.0.0 (`[[source: pipes-rs-1-6-4-license]]`), from the same
pinned nixpkgs, and draws its six pipes from the box-drawing characters
┃━┏┓┗┛ (`[[source: pipes-rs-1-6-4-kind-rs]]`): no art in it. On `q` it
leaves the alternate screen (`[[source: pipes-rs-1-6-4-terminal-rs]]`),
so the screen before it comes back and `clear` carries the cursor from
there. pipes.sh 1.3.0, which recorded
`tests/recordings/ascii/pipes.raw`, is MIT but ends with `tput reset`
(`[[source: pipes-sh-1-3-0]]`), a full reset that clears the screen
first; cbonsai 1.4.2 is GPL-3.0 and draws a pot of its own
(`[[source: cbonsai-1-4-2-c]]`). baud's mark, missing from this GIF's
titlebar, is in the next take's: it had been placed for the window's
default size, above the top of a window the grid had made shorter.
baud's window opens centred on the primary display, the one the recorder
films (`BAUD_SCREEN=primary`), where in one try it opened on a second
display, the one with the key window; and the session plays only once
the window has kept one frame for half a second.

The next take also ssh's into a container on the Mac it is filmed on,
after `cat hello.txt`: `ssh demo`, `fastfetch` there, and `exit` back to
baud's prompt. The container holds three packages, busybox, OpenSSH's
sshd and fastfetch, in an image the repository's flake makes with
nixpkgs' `dockerTools` at its pinned nixpkgs (`nix build
.#demo-container`, `nix/demo-container/`). The packages come from
cache.nixos.org, so a Mac makes it with no Linux builder, and the demo
refuses the image unless its stream hashes to the SHA-256 it pins
(`baud-app/examples/demo/container.rs`). docker runs it in colima, its
host named `demo` and its user `baud`; the demo starts colima if it is
stopped and stops it again, and removes the container and its image,
after Ctrl-C too. ssh
reads a config, a key and a known host made for the session in its
scratch directory, and nothing of the person's own, so it neither asks
nor writes. fastfetch is told its modules: the user and host, the kernel
the VM boots, the grid's size, the terminal's two colours and the
palette, beside Tux. None of them reads the chip, the memory, a disk, an
address, the battery or the uptime.

It lights eight of baud's nine activity lights in the title bar: RD from
the launch, SD with the first key, OH while btop is in front, HS on
btop's frames, which fill the tty's 1024-byte read, SYN while btop holds
synchronized output open, CD while ssh holds its connection to the
container, AA when fastfetch asks the terminal for its colours, and FAX
while the photo is on screen. CS lights only when the kernel refuses a
write: a program that stops reading its input while more than a kilobyte
is owed it. Keys typed at a person's pace never fill that, so no step of
the session lights CS. `cargo run --release --example demo -- --headless
DIR` plays the session without a window, through baud's core and CPU
renderer, and fails unless each of the eight lights. It also writes a
still of every hold into DIR.

The window is solid. `[glass] dim_dark` and `dim_light` are 1.0, so
nothing behind the window shows through the text. `[font] changes` and
`[window] operations` are `"off"`, so no program can change the font,
its size, or the window's size or place. It stays at 110x33 cells and
12 pt. The title bar
above the text stays the window's material: no setting makes it opaque.

A recording for the README is made by one command, which plays the
session in baud's window, films it and encodes the GIF:
`cargo run --release --example demo -- --record assets/readme/hero.gif
--forbidden LIST --controls FILE` (`baud-app/examples/demo.rs`). LIST is
what must never be filmed, kept outside the repository because it names
what it forbids, and FILE an example of each of its patterns. The
session's bytes are searched for LIST once every pattern has been found in
its example, and a hit fails the recording. It writes a file named
hero.gif.provenance.toml beside the GIF: the commit of baud that was
built and filmed and whether its tree was clean, every step with its
time since the launch, each tool's nixpkgs store path, the encoder's
settings and the SHA-256 of the GIF, the raw film and every pinned file.
Anyone can check that the GIF is the one it describes with
`cargo run --example demo -- --verify assets/readme/hero.gif`. The
`hero.gif` here was recorded before that command existed, and has no
provenance file yet.

`social.png` is a 1280x640 card for the repository's social preview,
which GitHub takes only as an upload in the repository's settings, never
from the tree. It is drawn, not filmed: the lockup (`assets/logo/`'s,
its ❯ and cursor as baud draws them) and the README's
tagline on the brand's dark ground, over a strip of amber paper tape
punched with `baud`, nine times, in the five-unit code of ITU-T S.1
([[source: itu-t-s1]]). Its Table 1 gives each letter's five elements, and
a hole is punched where it has Z (clause 3.2), element 1 on the top track.
A smaller feed hole is in every column, between the second and third
tracks, as five-hole tape is usually drawn: no text read for it places
that track. A feed hole is 0.046/0.072 of a code hole, the two diameters
ANSI X3.18 gives for one-inch tape ([[source: ansi-x3-18-1967]]).
`scripts/make-brand-pictures.py` draws it and its source, `social.svg`,
from JetBrains Mono's outlines (`JetBrainsMono-OFL.txt` beside them), the
PNG the SVG drawn by its pinned resvg, and reads the tape back off the PNG
before it writes. Nothing in it comes from
a machine or a recording. The card before it was cut from a frame of the
hero's btop and showed the machine's chip, memory and disks;
`scripts/readme-capture.sh` no longer makes it.

The photo is an elementary OS wallpaper, pinned by nixpkgs revision and
sha256. It is an Unsplash photograph published before 8 June 2017 and so CC0,
as elementary's `debian/copyright` records: "Canazei Granite Ridges"
(unsplash.com/photos/yrwpJwDNSHE).
