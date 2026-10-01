# CoreText text probes

Small Swift programs that measure what macOS CoreText does to glyph masks.
They are the only source for the constants in
`docs/research/notes/j-text-antialiasing.md`, which is why they are kept:
Apple publishes none of these numbers.

Build any of them the same way. Xcode's `swiftc` is needed, and the nix SDK
breaks it, so `SDKROOT` is unset:

```
export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
env -u SDKROOT swiftc -sdk "$(/usr/bin/xcrun --sdk macosx --show-sdk-path)" -O sweep.swift -o sweep
./sweep            # Menlo; pass a PostScript name for another font
```

| probe | measures | what it established |
|---|---|---|
| `sweep.swift` | stem gain from font smoothing against the fill's grey level, at 13 and 20 px, and at 2x against 1x | five dilation levels, with steps at encoded fills 0.39 / 0.65 / 0.815 / 0.945 (linear 0.125 / 0.375 / 0.625 / 0.875); the gain depends on device pixels, not on how the scale was reached |
| `fine.swift` | stem gain per level at 6-34 px | total stroke gain (0.0115 + 0.0050 n) per ppem, capped at about 0.60 px |
| `profile.swift` | the coverage profile across a stem edge, smoothing off and on, at four phases; and AppKit's default (an `NSAttributedString` into a bitmap) | smoothing moves each edge outward, it does not reshape a coverage curve; AppKit's default equals level 0 for black text and level 4 for white |
| `fgbg.swift` | stem gain with the fill fixed and the background varied, and the reverse | the level follows the text colour only, never the background |
| `blend.swift` | a glyph's pixels against its alpha-only mask, black on white and white on black, in sRGB and linear-sRGB contexts | CoreText's grayscale blend is OVER on the stored values: `bg + a(fg - bg)`, the same arithmetic as baud's native blend |
| `zones.swift` | flat-edge positions (x-height, cap, baseline, descender) at 1x per ppem | the 1x vertical snap: only at integer device ppem up to 18, x-height overshoot rounded, cap snapped except at 14 and 15 px, baseline and descender untouched |
| `snapdet.swift` | per-glyph top-ink height at an integer ppem and at ppem + 0.001; `COLS=1` compares width profiles, `SCALE=2` renders at 2x | the snap needs an exact integer device ppem, is vertical only, and happens for every font tried (Menlo, Monaco, Andale Mono, Courier, Helvetica) |
| `appkit-smoothing.swift` | AppKit text ink under `AppleFontSmoothing` absent and 0-3, set in the argument domain | absent, 1, 2 and 3 draw the same smoothed text and 0 draws it plain; a bare CoreGraphics context ignores the key |

The anisotropy of the dilation (tops +2d, bottoms +0, sides +d, square
corners) was fitted from pixel dumps by a Rust harness against these
probes' CoreText renders; it is not reproduced by any one probe here. Two
generators write the fixtures `baud-font`'s smoothing is gated against:

| generator | writes | for |
|---|---|---|
| `smooth-fixtures.swift <font> <dir>` | `ct-smooth-<ppem>.bin`: CoreText's masks of every printable ASCII glyph and five block elements, smoothing off and at each level, 9.001-48 px | `baud-font/tests/smooth.rs`, run against `baud-font/tests/fonts/JetBrainsMono-Regular.ttf` |
| `smooth-cubics.swift <out>` | `cg-cubics.bin`: cubic circles and blobs, some left open, filled by CoreGraphics at flatness 1 | the same test's cubic gate: JetBrains Mono has no cubics |
| `smooth-colours.swift <font> <out>` | `ct-colours.bin`: CoreText's full block in 21 coloured fills over black, smoothing off and on, at 13.001, 20 and 28 px | the same test's level gate: a grey fixture cannot tell Rec.709 from Rec.601 |

The `.001` sizes avoid CoreText's integer-ppem vertical snap. Regenerate
both on a new macOS and expect the gate to say whether CoreText moved.
