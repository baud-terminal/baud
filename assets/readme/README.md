# The README's pictures

`hero.gif` is a screen recording of a real baud session, its window filmed
as it ran: a shell, every command typed into it at real speed, glass and
smooth scrolling as they are, at 110x33 cells and 12 pt, in Dark. It shows
the launch logo carried off by the plasma, btop (refreshing every 100 ms,
its process and network boxes hidden), `cat README.md` and a scroll back
through it, helix finding the tagline and jumping to the end and back with
the plasma trail, a photo drawn by `kitten icat`, neo, asciiquarium, and
`clear` sending the cursor to the top with the trail. The shell's prompt is
a plain `baud ❯`, so nothing names the machine. Encoded with gifski at
1164 px wide (16.9 MB), with no frame carrying traces of another, so it
loops clean. neo's katakana are drawn by the macOS system font baud falls
back to, as baud's own font has none; a recording played with the example
draws them from Noto Sans CJK SC, which it configures.

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
from the tree: the lockup and the tagline over a darkened frame of the
hero's btop, taken from the earlier `hero.gif`; `ONLY=social
scripts/readme-capture.sh` remade it from that clip, so remaking it now
needs the frame's time set for the recording.

The photo is an elementary OS wallpaper, pinned by nixpkgs revision and
sha256. It is an Unsplash photograph published before 8 June 2017 and so CC0,
as elementary's `debian/copyright` records: "Canazei Granite Ridges"
(unsplash.com/photos/yrwpJwDNSHE).
