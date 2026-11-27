#version 450
#extension GL_GOOGLE_include_directive : require
// `cell_glyph_vertex`: each cell's glyph, a four-vertex strip over the
// glyph's atlas rectangle, read from the grid in place and looked up in the
// glyph map; `part` names the rows of the glyph the pass draws
// (`PART_INSIDE` or `PART_ABOVE`). The outputs are the glyph fragment
// stage's inputs.
#define CELL_GOUT
#include "cells.glsl"

GOut cell_glyph_vertex(uint vid, uint iid) {
    GOut o;
    o.pos = vec4(0.0); o.uv = vec2(0.0); o.color = vec4(0.0); o.kind = 0u; o.curve = 0u;
    uint col = iid % u.grid.x, dr = iid / u.grid.x;
    int lrow = int(dr) - int(u.slide_rows);
    uint base = seg_base[dr];
    // The overlay is a screen-cell mask; index it by the screen row, not
    // the draw row, and the leaving row above the window by the row after
    // the screen's (`sized_bits`). Bit 1 (`MASK_RECORD`) is a cell a record
    // draws; bit 2 is a picture's, never on the leaving row.
    if ((sized_bits(col, lrow) & MASK_RECORD) != 0u) return o;
    uvec2 cell = cell_at(col, dr);
    uint content = cell_content(cell);
    if (content > 0x10FFFFu) return o;
    uint shapes = styles[cell_rendition(cell, base)].shapes;
    if (hidden(shapes)) return o;
    // The contrast curve row for the side the swaps leave the glyph on:
    // the slot's foreground half, or its background half when swapped.
    uint curve = glyph_curve(styles[cell_rendition(cell, base)], content, col, lrow);
    // Two cells when the next cell is this one's wide tail: the grid says
    // the width there, not the cell's flags.
    uint two = (col + 1u < u.grid.x && cell_content(cell_at(col + 1u, dr)) == WIDE_TAIL) ? 1u : 0u;
    uint key = content | (((shapes >> FACE_SHIFT) & 3u) << KEY_STYLE_SHIFT)
                       | (((shapes >> (FONT_SHIFT + 4u)) & 1u) << KEY_FRAKTUR_SHIFT)
                       | (two << KEY_SPAN_SHIFT)
                       | (((shapes >> FONT_SHIFT) & 15u) << KEY_FONT_SHIFT);
    uint mask = u.map_slots - 1u;
    uint i = (key * 0x9E3779B1u) >> u.map_shift;
    // `e` is read only when `found`; the initialiser is never seen.
    Entry e = Entry(0u, 0u, 0u, 0u, 0u, 0u); bool found = false;
    for (uint n = 0u; n < u.map_slots; n++) {
        e = entries[i];
        if (e.key == 0xFFFFFFFFu) break;
        if (e.key == key) { found = true; break; }
        i = (i + 1u) & mask;
    }
    if (!found || e.kind > 2u) return o;
    uint fgw, bgw, curw;
    colors(cell, base, col, lrow, fgw, bgw, curw);
    vec2 c = vec2(float(vid & 1u), float(vid >> 1u));
    uvec4 rect = entry_rect(e);
    ivec2 eat = entry_at(e);
    float rowy = float(lrow) * float(u.cell.y) + float(u.pan);
    vec2 origin = vec2(float(col * u.cell.x) + float(eat.x),
                       rowy + float(u.baseline) + float(eat.y));
    vec2 wh = vec2(rect.zw);
    // The rows of the glyph this part draws, cut at the row's top: whole
    // pixels on both sides, so the two parts draw every pixel once.
    float y0 = origin.y, y1 = origin.y + wh.y;
    float a = part == PART_ABOVE ? y0 : max(y0, rowy);
    float b = part == PART_ABOVE ? min(y1, rowy) : y1;
    if (b <= a) return o;
    vec2 at = vec2(origin.x, a);
    vec2 size = vec2(wh.x, b - a);
    o.pos = clip(at + c * size);
    float side = e.kind == 1u ? float(u.sides.y) : float(u.sides.x);
    o.uv = (vec2(rect.xy) + vec2(0.0, a - y0) + c * size) / side;
    o.color = unpack(fgw);
    o.kind = e.kind;
    o.curve = curve;
    return o;
}

void main() {
    // gl_InstanceIndex counts from the draw's firstInstance, as the
    // instance id counts from the draw's base instance.
    gout_write(cell_glyph_vertex(uint(gl_VertexIndex), uint(gl_InstanceIndex)));
}
