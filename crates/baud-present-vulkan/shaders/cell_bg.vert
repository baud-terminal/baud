#version 450
#extension GL_GOOGLE_include_directive : require
// `cell_bg_vertex`: each cell's background, a four-vertex strip over the
// cell, read from the grid in place; `part` is the pass's `ground_pass`.
#include "cells.glsl"

layout(location = 0) out vec4 out_color;

void main() {
    // gl_InstanceIndex counts from the draw's firstInstance, as the
    // instance id counts from the draw's base instance.
    uint vid = uint(gl_VertexIndex), iid = uint(gl_InstanceIndex);
    uint ground_pass = part;
    uint col = iid % u.grid.x, dr = iid / u.grid.x;
    int lrow = int(dr) - int(u.slide_rows);
    uint base = seg_base[dr];
    // On a sized line the column shows the character under half of it.
    bool sized = (sized_bits(col, lrow) & MASK_SIZED) != 0u;
    uint at = sized ? col / 2u : col;
    uvec2 cell = cell_at(at, dr);
    bool set = ground_pass != GROUND_EVERY && ground_set(cell, base, at, uint(lrow));
    if (ground_pass == GROUND_ONLY_SET && !set) {
        gl_Position = vec4(2.0, 2.0, 0.0, 1.0);
        out_color = vec4(0.0);
        return;
    }
    // A cell of a picture drawn in text keeps its background solid under
    // the dim, inside `colors()`.
    uint fgw, bgw, curw;
    colors(cell, base, at, lrow, fgw, bgw, curw);
    vec2 c = vec2(float(vid & 1u), float(vid >> 1u));
    vec2 p = vec2((float(col) + c.x) * float(u.cell.x),
                  (float(lrow) + c.y) * float(u.cell.y) + float(u.pan));
    gl_Position = clip(p);
    vec4 bg = unpack(bgw);
    // No blend follows: each channel is rounded to its byte here, the
    // nearest to `c a / 255` as `premultiply_chan` rounds it, so the store
    // holds an exact k/255 and never meets a half.
    vec3 rgb = floor(bg.rgb * bg.a * 255.0 + 0.5) / 255.0;
    out_color = ground_pass == GROUND_CLEAR_SET && set ? vec4(0.0) : vec4(rgb, bg.a);
}
