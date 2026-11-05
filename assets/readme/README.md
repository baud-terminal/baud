# The README's pictures

`logo-dark.gif` and `logo-light.gif` are the README's header: the
lockup the launch splash holds, as the logo files draw it, drawn by
baud's CPU renderer at twice its 300 units on GitHub's own page colour,
#0d1117 for the dark theme and #ffffff for the light, 840 by 580 and
shown 420 wide, so the lockup is its 300. baud's white plasma moves round
and through it as if each glyph were a solid wall: in front of the
letters or behind them, changing side only through the counters of b, a
and d, the gaps between the letters, the u's open top or round the ends;
behind a letter it is hidden but for what shows through its holes;
against a stroke it squashes flat and runs along it; where it passes in
front, a keyline of page parts it from the letter and the letter is cut
under its core, as the strand over another in a knot. A swirling current
(curl noise) carries it, and a short tail of wisps slides round the
letters. Its path is a seeded random chain of moves, never a lap, and
the block blinks as baud's cursor blinks by default, each frame's phase
asked of the app's own caret (`baud-app/src/logo_film.rs`).

**It plays once, for 4.5 s, then rests.** WCAG 2.2's 2.2.2 Pause, Stop,
Hide (Level A) covers moving or blinking content that starts by itself,
lasts more than five seconds and is shown beside other content, and asks
for a way to pause, stop or hide it ([[source: wcag22]]); a README
carries no control of its own, and GitHub's setting to pause animated
images does not reach an image inside `<picture>`, so the logo stops by
itself. It moves for the most whole blink cycles that end before five
seconds, nine at the default's 250 ms shown and 250 hidden: 216 frames,
20 ms each and 30 closing each phase, fading in from the bare logo over
0.3 s and out to it over 0.4, then a last frame of the logo with the
cursor shown, which the file rests on. It carries no loop block. The
4.5 s are the plasma's ten-second chain from 1.00 s, the start on a
blink cycle with the most of the chain's events between the fades --
two wall hits, a graze, a weave through two slots and two surges -- and
of those the least light under the fades.

| file | bytes | frames | motion | most flashes at a pixel in a second, general / red | area, strict, of 21,824 | the word's worst contrast |
|---|---|---|---|---|---|---|
| `logo-dark.gif` | 2,671,015 | 217 | 4.5 s, then shown | 9 / 0 | 630.5, 1,495 | 6.79 : 1 |
| `logo-light.gif` | 2,646,127 | 217 | 4.5 s, then shown | 9 / 0 | 761.25, 2,304.5 | 12.61 : 1 |
| `logo-dark.svg` | 1,178,009 | 215 images over the logo | 4.5 s, then shown | 9 / 0 | 604, 1,500.5 | |
| `logo-light.svg` | 1,100,085 | 215 images over the logo | 4.5 s, then shown | 9 / 0 | 762, 2,337.8 | |

Both pass WCAG 2.3.1 ([[source: wcag22]]) by its second half: a few
pixels flash more than three times in a second where strands cross them,
over an area far under the general and red flash thresholds. The
contrast is the word's own ink against the ground 2 to 7 pixels round
it, the worst frame of both files. WCAG 1.4.3 exempts a logotype ("Text
that is part of a logo or brand name has no contrast requirement",
[[source: wcag22]]); the script's floor borrows the 4.5 to 1 it asks of
text, so the plasma never washes the word out.

**The README shows the GIF; the SVGs are for the site.** An SVG is the
bare logo as a PNG with each frame over it as a WebP of what it changes,
shown and hidden by SMIL `<set>` at the GIF's own times, all hidden at
4.5 s so it rests on the logo: under half the GIF's bytes, in full colour
where a GIF frame has 256. GitHub serves a file of the repository with
`Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline';
sandbox`; headless Chromium, given those headers and the SVG in an
`<img>`, draws its frames, plays them at their times and rests on the
logo, but nothing here has shown it in Safari, Firefox or GitHub's
mobile app, where an animated SVG made of embedded pictures is not
known to play, and a GIF plays everywhere the README is read. The GIF is
also the look the plasma was chosen in.

**The plasma's path was chosen against `logo-walls.png`**: the bare
lockup on the dark half, block shown, as the renderer drew it then,
from which `scripts/readme-logo/geometry.py` reads the glyphs' signed
distance field, normals and counters. Today's renderer draws it 3,930
pixels apart by at most 2 levels, 3 pixels of its half-coverage mask; the
path is a random search that takes every pixel of the walls as given,
and against today's own drawing the same seed draws a chain that fails
its targets (31% of the time behind the letters where the targets ask
40 to 50, two counter passes where they ask three), so the walls are
kept, pinned by their SHA-256, and every run holds today's drawing to
them at those counts, or the path must be chosen again.

Three tests hold the committed files, each decoding them with baud's
own decoders: `logo_film::tests::the_readme_logos_move_under_five_seconds_and_rest_shown`
(`baud-app`) requires each GIF to play once, the plasma to move, the
block read off its pixels to blink the default's phases, under five
seconds of motion and a last frame that is the first, the bare logo,
cursor shown; `logo_film::tests::the_readme_svgs_play_the_gifs_times_and_rest_shown`
requires each SVG to hold nothing that runs or fetches, nothing that
repeats, its frames on the film's grid, never two at once, its motion
ending where the GIF's does, and its base the GIF's last frame, cursor
shown; `baud-bench/tests/readme_logo_flashes.rs` takes WCAG 2.3.1's
census of each GIF at 100 samples a second and 2 pixels a point, the
last frame held 3 s, and requires both verdicts to pass, no red flash,
and no more flashes at a pixel or strict area than the files measured.
`scripts/make-readme-logo.py` films both halves, encodes them with
gifski 1.34.0 from `readme-film.sh`'s nixpkgs and Pillow from
`make-brand-pictures.py`'s, every pixel within 3 levels of the page set
to it first so the encoder cannot haze the page (0.0004% of the dark
GIF's page pixels are off it, by at most 13 levels), checks both
encoders write the same bytes twice, takes the SVGs' census, writes the
files and runs the tests; with `--check` it compares the files byte for
byte instead of writing them. The generator's arithmetic is IEEE's own
and `scripts/readme-logo/det.py`'s, and it runs with the platform's
`sin`, `exp` and the like taken away (`scripts/readme-logo/nolibm.py`),
so its frames do not depend on a platform's libm.

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
and the GPU's memory in use of it, the disks with their sizes and volume
names, the uptime and the load average, with the battery's charge, the time
and each core's load and temperature. asciiquarium's fish and sea are Joan
Stark's art (`[[source: asciiquarium-1-1]]`), which she gives "for personal
use only" (`[[source: jgs-guidelines]]`), and neo's katakana are drawn by
the macOS system font baud falls back to. Encoded with gifski at 1164 px
wide (16.9 MB), with no frame carrying traces of another, so it loops
clean. It is to be filmed again, as the take below.

The next take is about 19 seconds, at 80x24 cells and 14 pt
(`baud-app/examples/demo/session.rs`). The README shows the GIF 776 px
wide whatever the window's size, so its text's size there is set by the
columns: about 776 / (0.6 x 80), 16 px, GitHub's body size, where 110
columns gave 11.5. Shot by shot:

1. The launch: the logo drawn out of the plasma and carried off, then
   `baud ❯`.
2. helix opens baud's own config, `~/.config/baud/config.toml`. `ge` jumps
   to its last line, `trail = "plasma"`, in the plasma trail. `/plasma`,
   `cliquid`, Escape and `:w` change the word to `liquid` and save the
   file, and baud reads it again by itself. `gg` jumps back up in the
   liquid trail, a glass gel in the cursor's colour, and `ge` down again.
   No hidden script switches the trail: the edit on screen is all of it,
   and the liquid trail stays for the rest of the take.
3. `cat hello.txt`: baud defined in Hebrew and Arabic, drawn right to
   left, and in Chinese, Japanese, Korean and Hindi, each definition
   quoted from a dictionary, a standard, ITU's own translation or a
   book's glossary and named in the demo's source, then a line of emoji
   with six flags, a file the demo writes.
4. `kitten icat --transfer-mode=stream canazei.jpg`: a photo drawn in the
   grid below the command in kitty's graphics protocol, and the prompt
   under it. The transfer mode is typed rather than hidden: kitten's
   default, detect, timed out in baud at e73843fa and that timeout is not
   yet settled (`scripts/readme-capture.sh`).
5. `ssh demo`, into a container on the Mac it is filmed on: its badge,
   `demo`, at the top right, and the CD light while ssh holds its
   connection.
6. `fastfetch` there: baud's ❯ █ mark drawn as a picture, sent through ssh
   in iTerm2's inline image protocol, beside fastfetch's whole default
   card of the VM the container runs in.
7. `btop` there, its cpu box alone, a graph for each of the VM's CPUs, as
   many as the Mac's, sliding in over the shell and out again on `q`.
8. iTerm2's fireworks, asked for by a `printf` typed in full in the
   container.
9. `exit`: the badge cleared, CD dark.
10. `clear`: the cursor to the top in the liquid trail, and the empty prompt
   the clip loops to.

baud's mark, missing from this GIF's titlebar, is in the next take's: it
had been placed for the window's default size, above the top of a window
the grid had made shorter. baud's window opens centred on the primary
display, the one the recorder films (`BAUD_SCREEN=primary`), where in one
try it opened on a second display, the one with the key window; and the
session plays only once the window has kept one frame for half a second.

The container holds four packages, busybox, OpenSSH's sshd, fastfetch and
btop, in an image the repository's flake makes with nixpkgs' `dockerTools`
at its pinned nixpkgs (`nix build .#demo-container`,
`nix/demo-container/`), with baud's mark as a picture drawn from
`assets/logo/baud-mark-amber-tight.svg` by librsvg in the image's own
build. The packages come from cache.nixos.org, so a Mac makes it with no
Linux builder, and the demo refuses the image unless its stream hashes to
the SHA-256 it pins (`baud-app/examples/demo/container.rs`). docker runs it
in a colima VM of the demo's own, the profile `baud-demo`, made with every
CPU of the Mac and deleted with its data at the end, after Ctrl-C too;
colima's other profiles and docker's context are left as they were. Its
host is named `demo` and its user `baud`, and its shell marks each prompt
with `OSC 133` as baud's own shell does, sets the badge as the session
starts and clears it as it ends, in the `C.UTF-8` locale. fastfetch's
config lists fastfetch's own 29 default modules, since a config that sets
the logo and lists none prints none, so it prints its whole card of what
Linux in the VM sees: a generic virtual platform, the kernel, the uptime,
the shell, the tty, the CPUs as "Virtualized Apple Silicon" and their
count, the memory, swap and disk, the container's address and the locale;
the modules for an OS release, packages, a display, a desktop, themes,
fonts, a cursor, a GPU, a battery and a power adapter find none there and
print nothing. btop shows its cpu box alone. What they printed is searched
after the run for this machine's names.

Nothing of the person's own is read. baud is started with an environment
of its own with no `XDG_*` directory in it, so neither baud nor helix reads
the person's config: baud finds the scratch home's, the file helix edits.
ssh reads a config, a key and a known host made for the session in its
scratch directory, so it neither asks nor writes.

It lights all nine of baud's activity lights in the title bar: CS from
the launch, the line clear, RD from the launch, SD with the first key, OH
while helix is in front, AA when helix asks the terminal what it
supports, HS on a read that fills the tty's 1024 bytes, CD while ssh holds
its connection to the container, FAX while the photo or the mark is on
screen, and SYN while btop holds synchronized output open.
CS goes out only while bytes are owed a program that the kernel has not
taken: a program that stops reading its input while more than a kilobyte
is owed it. Keys typed at a person's pace never fill that, so CS stays lit
through the session.

`cargo run --release --example demo -- --headless DIR` plays the session
without a window, through baud's core and CPU renderer, reading the config
again when helix saves it, and fails unless each of the nine lights
lights, FAX first in the photo's shot, the trail is plasma at `ge` and
liquid at `gg` and the second `ge`, and `gg` lights a liquid. It writes a
still of every hold into DIR, and every frame, 100 a second, into DIR as a
lossless film,
`session.mkv`. **The title bar in those frames is composed, not filmed**:
no window is opened, so the strip above the text is drawn over each frame
(`baud-app/src/film/chrome.rs`) at the app's places -- the traffic lights
as plain circles, baud's lockup from its SVG, and the nine activity lights
drawn by baud's renderer as the app draws them, each at the state the
session's own lights showed at that frame -- on one grey, the middle of
the standard material's range, where the real window's strip shows the
desktop through it. Only `--record` films the window itself.
baud-bench's flash census counts the film's trails', photo's,
fireworks' and clear's shots against WCAG 2.3.1
(`baud-bench/tests/session_flashes.rs`), and each must pass. Every
character the programs printed must be drawn by baud's own faces or a
pinned one: btop draws its graphs in braille, which baud's own font has
none of, so Noto Sans Symbols 2 is pinned for it.

The window is solid. `[glass] dim_dark` and `dim_light` are 1.0, so
nothing behind the window shows through the text. `[font] changes` and
`[window] operations` are `"off"`, so no program can change the font, its
size, or the window's size or place. It stays at 80x24 cells and 14 pt.
The title bar above the text stays the window's material: no setting
makes it opaque.

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
sha256; the next take's demo fetches the same file by its URL at the
wallpapers' tag 8.0.0, checks the same sha256, and scales it to 850 px
wide. It is an Unsplash photograph published before 8 June 2017 and so CC0,
as elementary's `debian/copyright` records: "Canazei Granite Ridges"
(unsplash.com/photos/yrwpJwDNSHE).
