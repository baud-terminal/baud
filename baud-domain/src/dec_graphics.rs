//! DEC Special Graphics, generated from the source pinned in
//! `docs/research/sources/MANIFEST.toml`.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-dec-graphics.py`;
//! `dec_graphics_table_matches_the_pinned_source` fails if this file and the
//! pinned source disagree.
//!
//! Indexed by `byte - 0x60`, covering `` ` `` through `~`, which is the whole
//! of the set that differs from ASCII.

/// `[[source: foot-vt100-0]]`'s `vt100_0[]`.
pub static DEC_GRAPHICS: [u32; 31] = [
    0x25c6, // ` -> ◆  BLACK DIAMOND
    0x2592, // a -> ▒  MEDIUM SHADE
    0x2409, // b -> ␉  SYMBOL FOR HORIZONTAL TABULATION
    0x240c, // c -> ␌  SYMBOL FOR FORM FEED
    0x240d, // d -> ␍  SYMBOL FOR CARRIAGE RETURN
    0x240a, // e -> ␊  SYMBOL FOR LINE FEED
    0x00b0, // f -> °  DEGREE SIGN
    0x00b1, // g -> ±  PLUS-MINUS SIGN
    0x2424, // h -> ␤  SYMBOL FOR NEWLINE
    0x240b, // i -> ␋  SYMBOL FOR VERTICAL TABULATION
    0x2518, // j -> ┘  BOX DRAWINGS LIGHT UP AND LEFT
    0x2510, // k -> ┐  BOX DRAWINGS LIGHT DOWN AND LEFT
    0x250c, // l -> ┌  BOX DRAWINGS LIGHT DOWN AND RIGHT
    0x2514, // m -> └  BOX DRAWINGS LIGHT UP AND RIGHT
    0x253c, // n -> ┼  BOX DRAWINGS LIGHT VERTICAL AND HORIZONTAL
    0x23ba, // o -> ⎺  HORIZONTAL SCAN LINE-1
    0x23bb, // p -> ⎻  HORIZONTAL SCAN LINE-3
    0x2500, // q -> ─  BOX DRAWINGS LIGHT HORIZONTAL
    0x23bc, // r -> ⎼  HORIZONTAL SCAN LINE-7
    0x23bd, // s -> ⎽  HORIZONTAL SCAN LINE-9
    0x251c, // t -> ├  BOX DRAWINGS LIGHT VERTICAL AND RIGHT
    0x2524, // u -> ┤  BOX DRAWINGS LIGHT VERTICAL AND LEFT
    0x2534, // v -> ┴  BOX DRAWINGS LIGHT UP AND HORIZONTAL
    0x252c, // w -> ┬  BOX DRAWINGS LIGHT DOWN AND HORIZONTAL
    0x2502, // x -> │  BOX DRAWINGS LIGHT VERTICAL
    0x2264, // y -> ≤  LESS-THAN OR EQUAL TO
    0x2265, // z -> ≥  GREATER-THAN OR EQUAL TO
    0x03c0, // { -> π  GREEK SMALL LETTER PI
    0x2260, // | -> ≠  NOT EQUAL TO
    0x00a3, // } -> £  POUND SIGN
    0x00b7, // ~ -> ·  MIDDLE DOT
];
