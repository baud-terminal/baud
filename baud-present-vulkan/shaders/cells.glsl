// The cell path's shared part (`design-c4`): the grid read in place, the
// resolved renditions, the glyph map, the overlay, the frame's uniform and
// every helper the cell stages share: the declarations of
// `baud-present-macos/src/shaders.metal`, by the same names and with the same
// arithmetic.
// The buffers are the pipeline layout's, so the helpers read them by name
// where the Metal ones take them as arguments.

// `CU`, the frame's uniform, field for field the Metal path's: five uvec2
// at 8-byte alignment (40 bytes) and 28 uints, 152 bytes under std140.
layout(std140, set = 0, binding = 4) uniform CU {
    uvec2 viewport; uvec2 cell; uvec2 grid; uvec2 sides; uvec2 cursor;
    uint cursor_kind; uint baseline; uint underline; uint strike;
    uint thickness; uint map_slots; uint map_shift; uint stride; uint rot;
    uint ring;
    uint sel_on; uint sel_from_col; uint sel_from_row; uint sel_to_col;
    uint sel_to_row;
    // Smooth scroll: the screen is shifted down by `pan` pixels, and
    // `slide_rows` extra rows are drawn above the window -- logical row -1,
    // the line that just scrolled off, still in the ring one row up. Both
    // zero when settled.
    uint pan; uint slide_rows;
    // The cursor colour, packed 0xFFRRGGBB, or `PAINT_PAGE` for the cell's
    // page, or 0 for none: the cell's text. `paint()` reads it.
    uint cursor_color;
    // The blink shapes this frame hides: `SHAPE_BLINK` in `SGR 5`'s off
    // phase, `SHAPE_RAPID_BLINK` in `SGR 6`'s.
    uint blink_hidden;
    // How much of a blinking cursor this frame draws, 255 all of it.
    uint cursor_alpha;
    // The selection's colours, the page and the text, packed 0xFFRRGGBB, or
    // `PAINT_TEXT`/`PAINT_PAGE` for the cell's own, or 0 for none -- both
    // none is reverse video -- each with the contrast curve row a glyph in a
    // colour takes.
    uint sel_bg; uint sel_fg; uint sel_bg_curve; uint sel_fg_curve;
    // The text under a block cursor: packed 0xFFRRGGBB, or `PAINT_TEXT` for
    // the cell's own text, or 0 for none -- the cell's page -- and the curve
    // row a glyph in a colour takes.
    uint cursor_text; uint cursor_text_curve;
    // The blink shapes this frame draws faint rather than hides, and the
    // side of its cell a bar cursor is drawn on, 1 the right.
    uint blink_faint; uint cursor_side;
} u;

// A resolved rendition, nine words.
struct Style { uint fg; uint bg; uint ul; uint shapes; uint curves; uint glass; uint gcurves;
               uint line; uint lcurves; };

// A glyph map entry, six words: the key, the kind, the atlas rectangle as
// two words of two u16 (x | y << 16, z | w << 16), the placement as one
// word of two i16 (x low, y high), and padding. `entry_rect` and
// `entry_at` read the halves.
struct Entry { uint key; uint kind; uint rect_xy; uint rect_zw; uint at; uint pad; };

// The grid, read in place: each cell two words, the content and then the
// rendition in the low 16 bits and the cell's flags in the high 16. Read
// only through `cell_content`, `cell_rendition` and `cell_flags`.
layout(std430, set = 0, binding = 0) readonly buffer Cells {
    uvec2 cells[];
};
layout(std430, set = 0, binding = 1) readonly buffer Styles {
    Style styles[];
};
layout(std430, set = 0, binding = 2) readonly buffer Entries {
    Entry entries[];
};
// One byte a cell, four to a word, the first in the low eight bits; read
// through `overlay_byte`.
layout(std430, set = 0, binding = 3) readonly buffer Overlay {
    uint overlay[];
};
// The first rendition-table slot of each draw row's band: a cell's
// rendition is its row's base plus its own index.
layout(std430, set = 0, binding = 5) readonly buffer SegBase {
    uint seg_base[];
};

// Which part a pass draws: the background pass's `ground_pass`, the glyph
// pass's `part`.
layout(push_constant) uniform Push {
    uint part;
};

// A cell's content word.
uint cell_content(uvec2 c) {
    return c.x;
}

// A cell's rendition: its slot in the rendition table, `base` the draw
// row's `seg_base`.
uint cell_rendition(uvec2 c, uint base) {
    return base + (c.y & 0xffffu);
}

// A cell's flags.
uint cell_flags(uvec2 c) {
    return c.y >> 16;
}

// Byte `i` of the overlay.
uint overlay_byte(uint i) {
    return (overlay[i >> 2] >> ((i & 3u) * 8u)) & 0xffu;
}

// An entry's atlas rectangle: x, y, width, height.
uvec4 entry_rect(Entry e) {
    return uvec4(e.rect_xy & 0xffffu, e.rect_xy >> 16, e.rect_zw & 0xffffu, e.rect_zw >> 16);
}

// An entry's placement from the cell origin, each half sign-extended.
ivec2 entry_at(Entry e) {
    return ivec2(int(e.at << 16) >> 16, int(e.at) >> 16);
}

// What the glyph, shape and ring stages hand the glyph fragment stage.
struct GOut { vec4 pos; vec2 uv; vec4 color; uint kind; uint curve; };

// A stage that defines `CELL_GOUT` before including this file gets the
// glyph fragment stage's inputs as its outputs, and `gout_write` to set
// them from a `GOut`.
#ifdef CELL_GOUT
layout(location = 0) out vec2 out_uv;
layout(location = 1) out vec4 out_color;
layout(location = 2) flat out uint out_kind;
layout(location = 3) flat out uint out_curve;

void gout_write(GOut o) {
    gl_Position = o.pos;
    out_uv = o.uv;
    out_color = o.color;
    out_kind = o.kind;
    out_curve = o.curve;
}
#endif

// A paint word: 0 none, the cell's text or page by name, else an opaque
// colour, which is never either.
const uint PAINT_TEXT = 1u;
const uint PAINT_PAGE = 2u;
uint paint(uint w, uint none, uint text, uint page) {
    return w == 0u ? none : w == PAINT_TEXT ? text : w == PAINT_PAGE ? page : w;
}

vec4 unpack(uint c) {
    return vec4(float((c >> 16) & 0xffu), float((c >> 8) & 0xffu),
                float(c & 0xffu), float(c >> 24)) / 255.0;
}

// Pixel `p` in clip space. Vulkan's clip space has y down, so y is
// `2y/h - 1` with no flip.
vec4 clip(vec2 p) {
    vec2 n = p / vec2(u.viewport);
    return vec4(n.x * 2.0 - 1.0, n.y * 2.0 - 1.0, 0.0, 1.0);
}

// The cell at draw row `dr`, column `col`: the grid is read in place,
// under the core's rotation, at its live stride, modulo the ring -- the
// buffer's rows, which on the primary screen are the screen plus the
// scrollback behind it. `dr` counts the extra `slide_rows` above the
// window, so logical row L = dr - slide_rows and the ring row is
// `(rot + L) % ring` -- for L = -1 (the leaving row) that is `rot - 1`, the
// line just retired, by the same modulo.
uvec2 cell_at(uint col, uint dr) {
    uint prow = (u.rot + dr + u.ring - u.slide_rows) % u.ring;
    return cells[prow * u.stride + col];
}

// The right half of a double-width character, and a cell nothing was
// written to: the content words past U+10FFFF.
const uint WIDE_TAIL = 0x110001u;
const uint EMPTY_CELL = 0x110000u;

// The stress mark's skip trie, `STRESS_SKIP_INDEX` and
// `STRESS_SKIP_BLOCKS`, read a byte at a time: only in a stage that defines
// `CELL_STRESS` (the shape stage), so the others do not carry the tables.
#ifdef CELL_STRESS
#include "stress_skips.glsl"
#endif

// The cell holds a cluster whose base the stress mark leaves bare.
const uint MASK_BARE = 16u;
// The cell is on a line that is not single-width, which the records draw
// whole. The background pass draws column `col` from the cell under
// `col / 2`, and the decoration pass draws nothing there.
const uint MASK_SIZED = 4u;
// A record draws the cell.
const uint MASK_RECORD = 1u;
// The cell is on a row drawn in another order, and whether it is selected
// is the second bit, set from the logical cell it shows.
const uint MASK_BIDI = 32u;
const uint MASK_SELECTED = 64u;

// The record and sized bits of the cell at column `col` of logical row
// `lrow`: the screen's row of the overlay, and for the leaving row above
// the window (`lrow` -1) the row after the screen's, which carries only
// those two bits.
uint sized_bits(uint col, int lrow) {
    uint row = lrow >= 0 ? uint(lrow) : u.grid.y;
    return overlay_byte(row * u.grid.x + col) & (MASK_RECORD | MASK_SIZED);
}

#ifdef CELL_STRESS
// Whether a cell's word takes `SGR 64`'s mark -- not a blank, not a wide
// tail (its lead marks both cells), not a codepoint the trie leaves bare.
// A cluster, above U+10FFFF, takes it unless the overlay marks it bare
// (`bare`).
bool takes_stress_mark(uint content, bool bare) {
    if (content == EMPTY_CELL || content == WIDE_TAIL) return false;
    if (content > 0x10FFFFu) return !bare;
    uint block = stress_skip_index(content >> 8);
    uint byte = stress_skip_blocks(block * 32u + ((content & 0xffu) >> 3));
    return ((byte >> (content & 7u)) & 1u) == 0u;
}
#endif

// A resolved rendition's shape bits.
const uint SHAPE_BLINK = 256u;
const uint SHAPE_CONCEAL = 512u;
const uint SHAPE_RAPID_BLINK = 4096u;
// The rendition draws the default background colour.
const uint SHAPE_DEFAULT_BG = 8192u;
const uint SHAPE_OVERLINE = 16384u;
const uint SHAPE_DOUBLE_OVERLINE = 32768u;
const uint SHAPE_FRAMED = 65536u;
const uint SHAPE_STRESS = 131072u;
const uint SHAPE_ENCIRCLED = 262144u;
// The glyph map holds an encircled run's three parts under this key plus
// 1 (the left end), 2 (the right end) and 3 (a cell alone), above every
// codepoint, face style and span.
const uint RING_KEY = 134217728u;
// The face style's two bits in a resolved slot's shapes, and where they
// sit in a glyph-map key, above the 21-bit codepoint.
const uint FACE_SHIFT = 10u;
const uint KEY_STYLE_SHIFT = 21u;
// Set in the key when the character spans two cells -- the next cell is
// its wide tail: its glyph is centred in them, a slot of its own.
const uint KEY_SPAN_SHIFT = 26u;
// The font a resolved slot's text is drawn in: the alternative font in its
// low four bits and Fraktur above. In a key the Fraktur bit sits between
// the style and the span and the alternative in the top four bits, clear
// of RING_KEY's bit; the primary font adds nothing.
const uint FONT_SHIFT = 19u;
const uint KEY_FRAKTUR_SHIFT = 23u;
const uint KEY_FONT_SHIFT = 28u;

// Whether the cell's own text -- its glyph, underline and strike -- is
// hidden: `SGR 8`, or `SGR 5` or `SGR 6` in its own rate's off phase, which
// `blink_hidden` names by the shape it hides. Hidden text draws its
// background and nothing of its own. The cursor's shape still draws.
bool hidden(uint shapes) {
    uint blinks = u.blink_hidden & (SHAPE_BLINK | SHAPE_RAPID_BLINK);
    return (shapes & (SHAPE_CONCEAL | blinks)) != 0u;
}

// Whether the cell's own text draws faint: `SGR 5` or `SGR 6` in its own
// rate's off phase while the frame draws that phase faint, `blink_faint`.
// Concealed text is hidden first.
bool faint(uint shapes) {
    uint blinks = u.blink_faint & (SHAPE_BLINK | SHAPE_RAPID_BLINK);
    return !hidden(shapes) && (shapes & blinks) != 0u;
}

// Whether the cell beside column `col` of draw row `dr` -- to its left or
// its right -- carries a run of `shape` on: it has the shape and its text
// draws. The row's edge carries nothing on.
bool carries(uint col, uint dr, bool left, uint shape) {
    bool edge = left ? col == 0u : col + 1u >= u.grid.x;
    if (edge) return false;
    uvec2 n = cell_at(left ? col - 1u : col + 1u, dr);
    uint ns = styles[cell_rendition(n, seg_base[dr])].shapes;
    return (ns & shape) != 0u && !hidden(ns);
}

// Whether the cell is inside the selection, both ends inclusive: a stream
// (`sel_on` 1) in row-major order, a block (`sel_on` 2) by its row range
// and its column range apart. A wide tail takes its lead's answer. On a row
// drawn in another order the overlay holds the answer for the logical cell
// (`MASK_BIDI`).
bool selected(uint content, uint col, uint row) {
    if (u.sel_on == 0u) return false;
    if (row < u.grid.y) {
        uint m = overlay_byte(row * u.grid.x + col);
        if ((m & MASK_BIDI) != 0u) return (m & MASK_SELECTED) != 0u;
    }
    if (content == WIDE_TAIL && col > 0u) col -= 1u;
    if (u.sel_on == 2u) {
        return row >= u.sel_from_row && row <= u.sel_to_row &&
               col >= u.sel_from_col && col <= u.sel_to_col;
    }
    bool after_from = row > u.sel_from_row || (row == u.sel_from_row && col >= u.sel_from_col);
    bool before_to = row < u.sel_to_row || (row == u.sel_to_row && col <= u.sel_to_col);
    return after_from && before_to;
}

// `level` of the way from `other` to `w`, every channel and alpha alike:
// `(x l + y (255 - l) + 127) / 255` in integers.
uint mix_level(uint w, uint other, uint level) {
    uint out_ = 0u;
    for (uint s = 0u; s < 32u; s += 8u) {
        uint x = (w >> s) & 0xffu, y = (other >> s) & 0xffu;
        out_ |= ((x * level + y * (255u - level) + 127u) / 255u) << s;
    }
    return out_;
}

// `w` with its alpha scaled by `level`, rounded.
uint scaled(uint w, uint level) {
    uint a = ((w >> 24) * level + 127u) / 255u;
    return (w & 0x00ffffffu) | (a << 24);
}

// `w` half way to `other`, each colour channel rounded half up, `w`'s alpha
// kept: `SGR 2`'s mix.
uint faint_on(uint w, uint other) {
    uint out_ = w & 0xff000000u;
    for (uint s = 0u; s < 24u; s += 8u) {
        uint x = (w >> s) & 0xffu, y = (other >> s) & 0xffu;
        out_ |= ((x + y + 1u) / 2u) << s;
    }
    return out_;
}

// An underline in a colour of its own as this frame draws it: faint with
// the text.
uint rule(Style s, uint bgw) {
    return faint(s.shapes) ? faint_on(s.ul, bgw) : s.ul;
}

// Whether the cell's page is drawn solid under the dim: its rendition's
// opaque background, bit 128 of its shapes, or a picture drawn in text
// (bit 2 of the overlay, never on the leaving row above the window).
bool solid(uint shapes, uint col, int lrow) {
    return (shapes & 128u) != 0u
        || (lrow >= 0 && (overlay_byte(uint(lrow) * u.grid.x + col) & 2u) != 0u);
}

// Whether the cell is a box-drawing line, which the contrast floor leaves
// as asked: Unicode's Box Drawing block.
const uint LINE_FIRST = 0x2500u;
const uint LINE_LAST = 0x257Fu;
bool is_line(uint content) {
    return content >= LINE_FIRST && content <= LINE_LAST;
}

// The contrast curve row a cell's glyph takes under `block`, while it is
// drawn unswapped: a line's (`lcurves`), else the glass text's (`gcurves`)
// over a page left dimmed; else the solid text's, each by the swaps'
// parity -- the selection's swap and the block cursor's each exchange the
// two colours, and a cursor colour replaces only the background.
uint curve_under(Style s, uint content, uint col, int lrow, bool block) {
    bool sel = selected(content, col, uint(lrow));
    bool swap = sel != block;
    // Under a highlight colour the glyph is in the selection's text, or,
    // swapped again by a block, its page: a colour's row where one is set,
    // else the cell's own text's or page's.
    if (sel && (u.sel_bg | u.sel_fg) != 0u) {
        uint page = u.sel_bg != 0u ? u.sel_bg : PAINT_TEXT;
        uint text = u.sel_fg != 0u ? u.sel_fg
                  : (page == PAINT_TEXT ? PAINT_PAGE : PAINT_TEXT);
        uint word = swap ? text : page;
        uint row = swap ? u.sel_fg_curve : u.sel_bg_curve;
        return word == PAINT_TEXT ? (s.curves & 0xffffu)
             : word == PAINT_PAGE ? (s.curves >> 16) : row;
    }
    bool own = !swap && !sel;
    uint curves = own && is_line(content) ? s.lcurves
                : own && !solid(s.shapes, col, lrow) ? s.gcurves
                : s.curves;
    return swap ? (curves >> 16) : (curves & 0xffffu);
}

// The curve row of a cell's glyph: a fading block counts from half way,
// and a block's text colour is its own.
uint glyph_curve(Style s, uint content, uint col, int lrow) {
    bool block = u.cursor_kind == 1u && col == u.cursor.x && uint(lrow) == u.cursor.y
                 && u.cursor_alpha >= 128u;
    // A block's text colour: a colour's own row, or the cell's solid text
    // as the selection leaves it, the curve the cell takes with no block.
    if (block && u.cursor_text == PAINT_TEXT) {
        if (!selected(content, col, uint(lrow))) return s.curves & 0xffffu;
        return curve_under(s, content, col, lrow, false);
    }
    if (block && u.cursor_text != 0u) return u.cursor_text_curve;
    return curve_under(s, content, col, lrow, block);
}

// The cell's colours after the selection's swap and then the block
// cursor's. Hiding comes after both and is not a colour: the glyph and
// shape passes skip a hidden cell's text. `base` is the cell's draw row's
// `seg_base`.
void colors(uvec2 cell, uint base, uint col, int lrow, out uint fgw, out uint bgw,
            out uint curw) {
    Style s = styles[cell_rendition(cell, base)];
    uint row = uint(lrow);
    // A page drawn solid -- an opaque background or a picture -- takes the
    // solid text, `fg`; a page left dimmed takes the text the floor lifted
    // against it, `glass`; a line takes `line`.
    if (solid(s.shapes, col, lrow)) {
        fgw = s.fg; bgw = s.bg | 0xff000000u;
    } else {
        fgw = s.glass; bgw = s.bg;
    }
    // What a bar or underline cursor falls back to: the text before the
    // line rule.
    curw = fgw;
    // A box-drawing line draws the text as asked, `line`, on either page.
    if (is_line(cell_content(cell))) fgw = s.line;
    // Opaque on both sides of a swap, and the text a swap hands to the
    // background is the solid one.
    uint ink = s.fg;
    if (selected(cell_content(cell), col, row)) {
        // The page the selection's where set, else the text; the text the
        // selection's where set, else the cell's own on a page that is not
        // the cell's text, else the page -- reverse video when neither is
        // set. Either may name the cell's own text or page.
        uint text0 = ink | 0xff000000u, page0 = bgw | 0xff000000u;
        uint page = paint(u.sel_bg, text0, text0, page0);
        uint none = (u.sel_bg == 0u || u.sel_bg == PAINT_TEXT) ? page0 : text0;
        fgw = paint(u.sel_fg, none, text0, page0);
        bgw = page; ink = fgw;
        curw = fgw;
    }
    if (u.cursor_kind == 1u && col == u.cursor.x && row == u.cursor.y) {
        uint f0 = fgw, b0 = bgw;
        uint text0 = ink | 0xff000000u, page0 = bgw | 0xff000000u;
        fgw = page0; bgw = text0;
        // The cursor colour fills the block instead of the swapped text,
        // and the cursor text colour is the glyph's, each a colour or the
        // cell's own.
        bgw = paint(u.cursor_color, text0, text0, page0);
        fgw = paint(u.cursor_text, page0, text0, page0);
        // A block part way through a fade: each colour `cursor_alpha` of the
        // way from the cell without it.
        if (u.cursor_alpha < 255u) {
            fgw = mix_level(fgw, f0, u.cursor_alpha);
            bgw = mix_level(bgw, b0, u.cursor_alpha);
        }
    }
    // A faint off phase, after every swap: the glyph half way to the page
    // it is left on.
    if (faint(s.shapes)) fgw = faint_on(fgw, bgw);
}

// Whether the cell draws a background that is not the default one: its
// rendition's `SHAPE_DEFAULT_BG` unset, or selected, or under a block
// cursor. `base` is the cell's draw row's `seg_base`.
bool ground_set(uvec2 cell, uint base, uint col, uint row) {
    Style s = styles[cell_rendition(cell, base)];
    bool block = u.cursor_kind == 1u && col == u.cursor.x && row == u.cursor.y;
    return (s.shapes & SHAPE_DEFAULT_BG) == 0u || selected(cell_content(cell), col, row) || block;
}

// `ground_pass`, for a frame with a kitty image below the text: 0 draws
// every cell; 2 draws the default backgrounds and the others the
// transparent zero, the pass before the image; 1 draws the cells whose
// background is not the default one alone, a quad of no area for every
// other, the pass again over the image, blended.
const uint GROUND_EVERY = 0u;
const uint GROUND_ONLY_SET = 1u;
const uint GROUND_CLEAR_SET = 2u;

// Which part of a glyph a pass draws: the rows from its cell's row top down
// (`PART_INSIDE`, before the shape pass), or the rows above it
// (`PART_ABOVE`, after): each row's glyphs and then its decorations, rows
// top to bottom, so a glyph reaching up from the row below lies over the
// decorations of the row it reaches.
const uint PART_INSIDE = 0u;
const uint PART_ABOVE = 1u;

// The shape pass's slots a cell: the fixed shapes (underline, strike,
// double underline, cursor, overline, double overline, frame bottom, frame
// sides, stress mark, encircled end) and then SEGMENTS quads of a curly,
// dotted or dashed underline.
const uint SHAPE_SLOTS = 27u;
const uint FIXED_SLOTS = 11u;
// The cursor's slot, which a frame with no decoration on its table draws
// alone.
const uint CURSOR_SLOT = 3u;
const uint SEGMENTS = SHAPE_SLOTS - FIXED_SLOTS;
