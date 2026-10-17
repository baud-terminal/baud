# The README's pictures

Every one is filmed from baud itself by `scripts/readme-capture.sh`, which
remakes the set and checks it: each clip loops, no frame is blank, nothing
on screen names the machine, and every file is under 5 MB. They share one
window, 80x24 at 16 pt (776x548 pt), in Dark, with baud's defaults
except a steady cursor; the capture also fixes BAUD_MOTION=full (the trail
shows whatever Reduce Motion says) and BAUD_SCROLLER=overlay (the fading
scrollbar).

| file | what it shows | made by |
|---|---|---|
| `text.png` | emoji sequences, flags, Chinese, Japanese, Nerd Font icons and JetBrains Mono ligatures, in a table whose right border stays straight | `scripts/readme-demo-text.sh`, `scripts/readme-text.txt` |
| `hero.gif` | btop, then a git log scrolling smoothly, a photo drawn by `kitten icat`, and the plasma trail | `scripts/readme-demo-hero.sh` |
| `images.gif` | a photo, then a smaller one beside text, through `kitten icat` | `scripts/readme-demo-images.sh` |
| `tools.gif` | the wheel into the scrollback with the overlay scrollbar, a selection, and a narrowing resize that rewraps (the recording keeps the full window's frame, so the narrowed window shows black to its right) | `scripts/readme-demo-tools.sh`, `scripts/readme-pointer.swift` |
| `trail.gif` | the plasma trail on two diagonal jumps, cut from `docs/cursor/plasma.mp4` (776x488 pt, the window that clip was taken at) | `scripts/trail-clips.sh` |

The 1280x640 social card is not committed: it waits for the owner's
tagline, then `ONLY=social TAGLINE='first|second' scripts/readme-capture.sh`
makes it from `hero.gif` and the lockup.

The photos are two elementary OS wallpapers, pinned by nixpkgs revision and
sha256. Both are Unsplash photographs published before 8 June 2017 and so
CC0, as elementary's `debian/copyright` records: "Canazei Granite Ridges"
(unsplash.com/photos/yrwpJwDNSHE) and "Sunset by the Pier"
(unsplash.com/photos/ces8_Bo7bhQ).
