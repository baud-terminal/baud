#version 450
#extension GL_GOOGLE_include_directive : require
// `cell_shape_vertex`: a cell's shapes, `SHAPE_SLOTS` four-vertex strips a
// cell -- its underline, its strike, a double underline's second line, the
// cursor when it is an underline or a bar, the overline (or a frame's top),
// a double overline's second line, a frame's bottom and its two sides, the
// stress mark, an end of an encircled run (a mask from the glyph map, as a
// glyph is), and then `SEGMENTS` quads for a curly, dotted or dashed
// underline, each a run of thickness-wide solids across the cell. A shape
// not drawn is a zero-area quad.
#define CELL_GOUT
#define CELL_STRESS
#include "cells.glsl"

GOut cell_shape_vertex(uint vid, uint iid) {
    GOut o;
    o.pos = vec4(0.0); o.uv = vec2(0.0); o.color = vec4(0.0); o.kind = 2u; o.curve = 0u;
    uint which = iid % SHAPE_SLOTS, ci = iid / SHAPE_SLOTS;
    uint col = ci % u.grid.x, dr = ci / u.grid.x;
    int lrow = int(dr) - int(u.slide_rows);
    uint base = seg_base[dr];
    // A sized line's decorations and cursor are its records'.
    if ((sized_bits(col, lrow) & MASK_SIZED) != 0u) return o;
    // The top of this row's cell, shifted down by the smooth-scroll pan.
    float rowy = float(lrow) * float(u.cell.y) + float(u.pan);
    uvec2 cell = cell_at(col, dr);
    Style s = styles[cell_rendition(cell, base)];
    if (which != CURSOR_SLOT && hidden(s.shapes)) return o;
    uint fgw, bgw, curw;
    colors(cell, base, col, lrow, fgw, bgw, curw);
    vec2 at; vec2 wh; uint colw;
    float thick = float(max(u.thickness, 2u));
    if (which == 0u) {
        if ((s.shapes & 1u) == 0u) return o;
        at = vec2(float(col * u.cell.x), rowy + float(u.underline));
        wh = vec2(u.cell.x, u.thickness);
        colw = (s.shapes & 4u) != 0u ? fgw : rule(s, bgw);
    } else if (which == 1u) {
        if ((s.shapes & 2u) == 0u) return o;
        at = vec2(float(col * u.cell.x), rowy + float(u.strike));
        wh = vec2(u.cell.x, u.thickness);
        colw = fgw;
    } else if (which == 2u) {
        // The second line of a double underline, below the first by a gap and
        // clamped inside the cell.
        if ((s.shapes & 8u) == 0u) return o;
        float th = float(u.thickness);
        float gap = max(th, 1.0) + 1.0;
        float low = float(u.cell.y) - th;
        float y = min(float(u.underline) + gap, max(low, float(u.underline)));
        at = vec2(float(col * u.cell.x), rowy + y);
        wh = vec2(u.cell.x, u.thickness);
        colw = (s.shapes & 4u) != 0u ? fgw : rule(s, bgw);
    } else if (which == CURSOR_SLOT) {
        if (col != u.cursor.x || lrow != int(u.cursor.y) || u.cursor_kind < 2u
            || u.cursor_kind > 3u) return o;
        if (u.cursor_kind == 2u) {
            at = vec2(float(col * u.cell.x), rowy + float(u.cell.y) - thick);
            wh = vec2(u.cell.x, thick);
        } else {
            // On the right of the cell where its character runs right to
            // left (`cursor_side`).
            float x = float(col * u.cell.x)
                    + (u.cursor_side == 1u ? max(float(u.cell.x) - thick, 0.0) : 0.0);
            at = vec2(x, rowy);
            wh = vec2(thick, u.cell.y);
        }
        // The bar or underline cursor draws in the cursor colour, a colour
        // or the cell's page, else the text, at the fade's opacity.
        colw = scaled(paint(u.cursor_color, curw, curw, bgw | 0xff000000u), u.cursor_alpha);
    } else if (which < FIXED_SLOTS) {
        // The overline, the frame, the double overline's second line, the
        // stress mark and an encircled run, in the text's colour as the
        // strike is: the overline, the frame's top and the top between an
        // encircled run's ends are one line, and the frame's bottom and
        // that run's bottom are one.
        float th = float(u.thickness);
        float th1 = max(th, 1.0);
        bool framed = (s.shapes & SHAPE_FRAMED) != 0u;
        bool encircled = (s.shapes & SHAPE_ENCIRCLED) != 0u;
        bool lc = encircled && carries(col, dr, true, SHAPE_ENCIRCLED);
        bool rc = encircled && carries(col, dr, false, SHAPE_ENCIRCLED);
        bool between = lc && rc;
        float x = float(col * u.cell.x);
        if (which == 4u) {
            if (!framed && !between && (s.shapes & SHAPE_OVERLINE) == 0u) return o;
            at = vec2(x, rowy);
            wh = vec2(u.cell.x, th);
        } else if (which == 5u) {
            if ((s.shapes & SHAPE_DOUBLE_OVERLINE) == 0u) return o;
            float low = float(u.cell.y) - th;
            at = vec2(x, rowy + min(th1 + 1.0, max(low, 0.0)));
            wh = vec2(u.cell.x, th);
        } else if (which == 9u) {
            // `SGR 64`'s stress mark: a filled square two thicknesses on a
            // side, centred across the character (both cells of a wide
            // one), a side's length below the cell's top, clamped inside.
            // A cluster's answer is the overlay's (`MASK_BARE`). The leaving
            // row above the window has no overlay, and its cells hold their
            // bases, so the table judges them.
            bool bare = lrow >= 0 && (overlay_byte(uint(lrow) * u.grid.x + col) & MASK_BARE) != 0u;
            if ((s.shapes & SHAPE_STRESS) == 0u || !takes_stress_mark(cell_content(cell), bare)) return o;
            float d = 2.0 * th1;
            bool wide = col + 1u < u.grid.x && cell_content(cell_at(col + 1u, dr)) == WIDE_TAIL;
            float span = float(wide ? 2u * u.cell.x : u.cell.x);
            at = vec2(x + floor(max(span - d, 0.0) * 0.5),
                      rowy + min(d, max(float(u.cell.y) - d, 0.0)));
            wh = vec2(d, d);
        } else if (which == 10u) {
            // An end of an encircled run: its part of the outline, the
            // mask the glyph map holds under `RING_KEY`, placed at the cell
            // origin as a procedural glyph is, in the text's colour and
            // bent by the glyph's curve.
            if (!encircled || between) return o;
            uint key = RING_KEY + (lc ? 2u : (rc ? 1u : 3u));
            uint mask = u.map_slots - 1u;
            uint i = (key * 0x9E3779B1u) >> u.map_shift;
            // Read only when `found`.
            Entry e = Entry(0u, 0u, 0u, 0u, 0u, 0u); bool found = false;
            for (uint n = 0u; n < u.map_slots; n++) {
                e = entries[i];
                if (e.key == 0xFFFFFFFFu) break;
                if (e.key == key) { found = true; break; }
                i = (i + 1u) & mask;
            }
            if (!found || e.kind > 2u) return o;
            vec2 c = vec2(float(vid & 1u), float(vid >> 1u));
            ivec2 eat = entry_at(e);
            uvec4 rect = entry_rect(e);
            vec2 origin = vec2(x + float(eat.x), rowy + float(u.baseline) + float(eat.y));
            vec2 rwh = vec2(rect.zw);
            o.pos = clip(origin + c * rwh);
            float side = e.kind == 1u ? float(u.sides.y) : float(u.sides.x);
            o.uv = (vec2(rect.xy) + c * rwh) / side;
            o.color = unpack(fgw);
            o.kind = e.kind;
            o.curve = glyph_curve(s, cell_content(cell), col, lrow);
            return o;
        } else if (which == 6u) {
            if (!framed && !between) return o;
            at = vec2(x, rowy + float(u.cell.y) - th1);
            wh = vec2(u.cell.x, th);
        } else {
            if (!framed) return o;
            // A side only where the run of framed cells starts or ends:
            // the row's edge, or a neighbour that does not carry it on.
            bool left = which == 7u;
            if (carries(col, dr, left, SHAPE_FRAMED)) return o;
            at = vec2(left ? x : x + float(u.cell.x) - th1, rowy);
            wh = vec2(th1, u.cell.y);
        }
        colw = fgw;
    } else {
        // Segment k of a styled underline, each a solid thickness square.
        uint k = which - FIXED_SLOTS;
        bool curly = (s.shapes & 16u) != 0u, dotted = (s.shapes & 32u) != 0u,
             dashed = (s.shapes & 64u) != 0u;
        if (!(curly || dotted || dashed)) return o;
        float th = max(float(u.thickness), 1.0);
        float dx = float(k) * th;
        if (dx >= float(u.cell.x)) return o;
        bool on = curly ? true : (dotted ? (k % 2u == 0u) : (k % 3u != 2u));
        if (!on) return o;
        float dy = (curly && (k % 2u == 1u)) ? th : 0.0;
        float low = float(u.cell.y) - th;
        float y = min(float(u.underline) + dy, max(low, float(u.underline)));
        at = vec2(float(col * u.cell.x) + dx, rowy + y);
        wh = vec2(th, th);
        colw = (s.shapes & 4u) != 0u ? fgw : rule(s, bgw);
    }
    vec2 c = vec2(float(vid & 1u), float(vid >> 1u));
    o.pos = clip(at + c * wh);
    o.color = unpack(colw);
    return o;
}

void main() {
    // gl_InstanceIndex counts from the draw's firstInstance, as the
    // instance id counts from the draw's base instance.
    gout_write(cell_shape_vertex(uint(gl_VertexIndex), uint(gl_InstanceIndex)));
}
