# The README's pictures

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

A recording for the README is made by one command, which plays the
session in baud's window, films it and encodes the GIF:
`cargo run --release --example demo -- --record assets/readme/hero.gif`
(`baud-app/examples/demo.rs`). It writes a file named
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
