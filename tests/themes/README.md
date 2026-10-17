# Theme file fixtures

Hand-made, for `baud-app/src/palette.rs`'s tests; none is from a published
theme collection.

- `day.yaml`: a whole base16 light scheme in the current nested form,
  declared `light`.
- `sepia.yml`: the same colours declaring `variant: "dark"`, so the declared
  variant disagrees with the page and text; and the `.yml` extension.
- `broken.yaml`: `day.yaml` without `base0F`, which the base16 reader refuses.
- `night`: a partial ghostty theme with no extension -- the eight normals,
  entry 42, the page, the text, and one key baud does not read.

The content detector's fixtures (note AR), for `palette.rs` and
`baud-domain/tests/themedetect.rs`. Those in another terminal's format all
carry one hand-made dark scheme, so each format's reading can be compared
with the others':

- `dawn`: `day.yaml` with no extension, so the name implies nothing.
- `night.yaml`: `night` named as YAML, which its content is not.
- `alacritty.toml`: an alacritty theme, double-quoted, with
  `[colors.search.matches]`, whose `background` a line-by-line ghostty
  reading took as the page.
- `kitty.conf`, `foot.ini` (`[colors-dark]`), `Xresources` (`*.color0:`) and
  `colors` (16 `#rrggbb` lines, as pywal's `colors` file).
- `tinted8.yaml`: tinted8's shape, `scheme:` holding `system: "tinted8"`.
- `mixed`: YAML lines and a ghostty line in one file.
- `slots-eq`: base16's slot names in `key = value` syntax.
- `alacritty.yml`: the same colours as alacritty's YAML config before 0.13,
  spelled `0x`, with one `indexed_colors` entry.
- `night.itermcolors` and `windows-terminal.json`: the same colours as an
  iTerm2 preset in sRGB (each component a byte over 255) and as a Windows
  Terminal scheme, which has no cursor text or selection text colour; the
  JSON keeps a comment and a trailing comma, as Windows Terminal's reader
  allows.
