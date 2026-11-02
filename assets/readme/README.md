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
back to, as baud's own font has none.

`text.png` is made by `scripts/readme-capture.sh`, which remakes it and
checks it, in one window, 80x24 at 16 pt (776x548 pt), in Dark, with baud's
defaults except a steady cursor and the fonts.

| file | what it shows | made by |
|---|---|---|
| `text.png` | emoji sequences, flags, Chinese, Japanese, Nerd Font icons and JetBrains Mono ligatures, in a table whose right border stays straight; drawn from open faces only, baud's own with Noto Color Emoji and Noto Sans CJK SC, so no Apple font drew any of it (the icons row does show the Nerd Font's Apple logo) | `scripts/readme-demo-text.sh`, `scripts/readme-text.txt` |

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
