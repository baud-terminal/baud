#version 450
#extension GL_GOOGLE_include_directive : require
// `cell_ring_vertex`: the hollow cursor, `cursor_kind` 4, four solids round
// the cursor's cell -- instance 0 its top and 1 its bottom across the cell,
// 2 and 3 its sides between them -- each the bar's line thick and at most
// half the cell's narrower side, in the bar's colour. No colour swaps
// (`colors` swaps only for kind 1). A sized line's cursor is its records'.
#define CELL_GOUT
#include "cells.glsl"

GOut cell_ring_vertex(uint vid, uint iid) {
    GOut o;
    o.pos = vec4(0.0); o.uv = vec2(0.0); o.color = vec4(0.0); o.kind = 2u; o.curve = 0u;
    if (u.cursor_kind != 4u || u.cursor.x >= u.grid.x) return o;
    uint col = u.cursor.x;
    int lrow = int(u.cursor.y);
    if ((sized_bits(col, lrow) & MASK_SIZED) != 0u) return o;
    uint dr = uint(lrow + int(u.slide_rows));
    uint base = seg_base[dr];
    float rowy = float(lrow) * float(u.cell.y) + float(u.pan);
    uvec2 cell = cell_at(col, dr);
    uint fgw, bgw, curw;
    colors(cell, base, col, lrow, fgw, bgw, curw);
    uint t = min(max(u.thickness, 2u), min(u.cell.x, u.cell.y) / 2u);
    float x = float(col * u.cell.x), w = float(u.cell.x), h = float(u.cell.y), ft = float(t);
    float side = float(u.cell.y - min(2u * t, u.cell.y));
    vec2 at; vec2 wh;
    if (iid == 0u) {
        at = vec2(x, rowy); wh = vec2(w, ft);
    } else if (iid == 1u) {
        at = vec2(x, rowy + h - ft); wh = vec2(w, ft);
    } else if (iid == 2u) {
        at = vec2(x, rowy + ft); wh = vec2(ft, side);
    } else {
        at = vec2(x + w - ft, rowy + ft); wh = vec2(ft, side);
    }
    uint colw = scaled(paint(u.cursor_color, curw, curw, bgw | 0xff000000u), u.cursor_alpha);
    vec2 c = vec2(float(vid & 1u), float(vid >> 1u));
    o.pos = clip(at + c * wh);
    o.color = unpack(colw);
    return o;
}

void main() {
    // gl_InstanceIndex counts from the draw's firstInstance, as the
    // instance id counts from the draw's base instance: the four sides are
    // instances 0 to 3 of a draw whose first instance is 0.
    gout_write(cell_ring_vertex(uint(gl_VertexIndex), uint(gl_InstanceIndex)));
}
