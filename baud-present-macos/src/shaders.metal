
#include <metal_stdlib>
using namespace metal;

struct U { float2 viewport; float2 cell; float2 grid; float2 mask; float2 color; float2 pad; };
struct Inst { short2 xy; ushort2 wh; ushort2 uv; uint color; uint kind; };

struct BgOut { float4 pos [[position]]; };

vertex BgOut bg_vertex(uint vid [[vertex_id]]) {
    float2 c = float2(vid & 1u, vid >> 1u);
    BgOut o;
    o.pos = float4(c * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

fragment float4 bg_fragment(BgOut in [[stage_in]],
                            constant U& u [[buffer(0)]],
                            texture2d<float, access::read> bg [[texture(0)]]) {
    float2 cf = (in.pos.xy - u.pad) / u.cell;
    if (cf.x >= u.grid.x || cf.y >= u.grid.y) return float4(0.0);
    float4 c = bg.read(uint2(cf));
    return float4(c.rgb * c.a, c.a);
}

struct GOut { float4 pos [[position]]; float2 uv; float4 color; uint kind [[flat]]; };

vertex GOut glyph_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                         device const Inst* insts [[buffer(0)]],
                         constant U& u [[buffer(1)]]) {
    Inst i = insts[iid];
    float2 c = float2(vid & 1u, vid >> 1u);
    float2 p = float2(i.xy) + c * float2(i.wh);
    float2 n = p / u.viewport;
    GOut o;
    o.pos = float4(n.x * 2.0 - 1.0, 1.0 - n.y * 2.0, 0.0, 1.0);
    float2 side = i.kind == 1u ? u.color : u.mask;
    o.uv = (float2(i.uv) + c * float2(i.wh)) / side;
    uint col = i.color;
    o.color = float4(float((col >> 16) & 0xffu), float((col >> 8) & 0xffu),
                     float(col & 0xffu), float(col >> 24)) / 255.0;
    o.kind = i.kind;
    return o;
}

fragment float4 glyph_fragment(GOut in [[stage_in]],
                               texture2d<float> mask [[texture(0)]],
                               texture2d<float> color [[texture(1)]],
                               sampler s [[sampler(0)]]) {
    if (in.kind == 0u) {
        float a = in.color.a * mask.sample(s, in.uv).r;
        return float4(in.color.rgb * a, a);
    }
    if (in.kind == 1u) {
        float4 c = color.sample(s, in.uv);
        return float4(c.rgb * c.a, c.a);
    }
    return float4(in.color.rgb * in.color.a, in.color.a);
}

// ---- The cell path: the raw grid, resolved per cell in the vertex stage.

struct CU {
    uint2 viewport; uint2 cell; uint2 grid; uint2 sides; uint2 cursor;
    uint cursor_kind; uint baseline; uint underline; uint strike;
    uint thickness; uint map_slots; uint map_shift; uint stride; uint rot;
    uint ring;
    uint sel_on; uint sel_from_col; uint sel_from_row; uint sel_to_col;
    uint sel_to_row;
    // Smooth scroll: the screen is shifted down by `pan` pixels, and
    // `slide_rows` extra rows are drawn above the window -- logical row -1,
    // the line that just scrolled off, still in the ring one row up. Both
    // zero when settled, so the non-slide draw is unchanged.
    uint pan; uint slide_rows;
};
struct Style { uint fg; uint bg; uint ul; uint shapes; };
struct Entry { uint key; uint kind; ushort4 rect; short2 at; uint pad; };

struct FlatOut { float4 pos [[position]]; float4 color; };

static float4 unpack(uint c) {
    return float4(float((c >> 16) & 0xffu), float((c >> 8) & 0xffu),
                  float(c & 0xffu), float(c >> 24)) / 255.0;
}

static float4 clip(float2 p, constant CU& u) {
    float2 n = p / float2(u.viewport);
    return float4(n.x * 2.0 - 1.0, 1.0 - n.y * 2.0, 0.0, 1.0);
}

// The cell at draw row `dr`, column `col`: the grid is read in place,
// under the core's rotation, at its live stride, modulo the ring -- the
// buffer's rows, which on the primary screen are the screen plus the
// scrollback behind it. `dr` counts the extra `slide_rows` above the
// window, so logical row L = dr - slide_rows and the ring row is
// `(rot + L) % ring` -- for L = -1 (the leaving row) that is `rot - 1`, the
// line just retired, by the same modulo. `slide_rows = 0` leaves it
// exactly the plain read.
static uint2 cell_at(constant CU& u, device const uint2* cells, uint col, uint dr) {
    uint prow = (u.rot + dr + u.ring - u.slide_rows) % u.ring;
    return cells[prow * u.stride + col];
}

// Whether the cell is inside the selection: row-major, both ends inclusive.
static bool selected(constant CU& u, uint col, uint row) {
    if (u.sel_on == 0u) return false;
    bool after_from = row > u.sel_from_row || (row == u.sel_from_row && col >= u.sel_from_col);
    bool before_to = row < u.sel_to_row || (row == u.sel_to_row && col <= u.sel_to_col);
    return after_from && before_to;
}

// The cell's colours after the selection's swap and then the block
// cursor's, which the CPU path applies in the same order after the
// rendition rules and before anything is drawn.
static void colors(constant CU& u, device const Style* styles, uint2 cell,
                   uint col, uint row, thread uint& fgw, thread uint& bgw) {
    Style s = styles[cell.y & 0xffffu];
    fgw = s.fg; bgw = s.bg;
    // Opaque on BOTH sides of a swap. Every background carries the
    // window's dim now, so the foreground a swap hands back would be
    // drawn at that alpha -- a glyph at 0.22 over its own cursor block.
    // This must match `Renderer::convert_row`, and the pixel gate
    // compares the two arms to say so.
    if (selected(u, col, row)) {
        uint t = fgw; fgw = bgw | 0xff000000u; bgw = t | 0xff000000u;
    }
    if (u.cursor_kind == 1u && col == u.cursor.x && row == u.cursor.y) {
        uint t = fgw; fgw = bgw | 0xff000000u; bgw = t | 0xff000000u;
    }
}

vertex FlatOut cell_bg_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const uint2* cells [[buffer(0)]],
                              device const Style* styles [[buffer(1)]],
                              constant CU& u [[buffer(2)]]) {
    uint col = iid % u.grid.x, dr = iid / u.grid.x;
    int lrow = int(dr) - int(u.slide_rows);
    uint fgw, bgw;
    colors(u, styles, cell_at(u, cells, col, dr), col, uint(lrow), fgw, bgw);
    float2 c = float2(vid & 1u, vid >> 1u);
    float2 p = float2((float(col) + c.x) * float(u.cell.x),
                      (float(lrow) + c.y) * float(u.cell.y) + float(u.pan));
    FlatOut o;
    o.pos = clip(p, u);
    float4 bg = unpack(bgw);
    o.color = float4(bg.rgb * bg.a, bg.a);
    return o;
}

fragment float4 flat_fragment(FlatOut in [[stage_in]]) { return in.color; }

vertex GOut cell_glyph_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const uint2* cells [[buffer(0)]],
                              device const Style* styles [[buffer(1)]],
                              constant CU& u [[buffer(2)]],
                              device const Entry* entries [[buffer(3)]],
                              device const uchar* overlay [[buffer(4)]]) {
    GOut o;
    o.pos = float4(0.0); o.uv = float2(0.0); o.color = float4(0.0); o.kind = 0u;
    uint col = iid % u.grid.x, dr = iid / u.grid.x;
    int lrow = int(dr) - int(u.slide_rows);
    // The overlay is a screen-cell mask; index it by the screen row, not
    // the draw row, and never for the leaving row above the window.
    if (lrow >= 0 && overlay[uint(lrow) * u.grid.x + col] != 0) return o;
    uint2 cell = cell_at(u, cells, col, dr);
    uint content = cell.x;
    if (content > 0x10FFFFu) return o;
    uint mask = u.map_slots - 1u;
    uint i = (content * 0x9E3779B1u) >> u.map_shift;
    Entry e; bool found = false;
    for (uint n = 0u; n < u.map_slots; n++) {
        e = entries[i];
        if (e.key == 0xFFFFFFFFu) break;
        if (e.key == content) { found = true; break; }
        i = (i + 1u) & mask;
    }
    if (!found || e.kind > 2u) return o;
    uint fgw, bgw;
    colors(u, styles, cell, col, uint(lrow), fgw, bgw);
    float2 c = float2(vid & 1u, vid >> 1u);
    float2 origin = float2(float(col * u.cell.x) + float(e.at.x),
                           float(lrow) * float(u.cell.y) + float(u.baseline)
                               + float(u.pan) + float(e.at.y));
    float2 wh = float2(e.rect.zw);
    o.pos = clip(origin + c * wh, u);
    float side = e.kind == 1u ? float(u.sides.y) : float(u.sides.x);
    o.uv = (float2(e.rect.xy) + c * wh) / side;
    o.color = unpack(fgw);
    o.kind = e.kind;
    return o;
}

// Shapes a cell: its underline, its strike, a second underline line for a
// double underline, the cursor when it is an underline or a bar, and then
// SEGMENTS quads for a curly, dotted or dashed underline (kitty's 4:3/4:4/4:5)
// -- each a run of thickness-wide solids across the cell, so the styles need
// no texel and composite exactly as the straight line does. A shape not drawn
// is a zero-area quad.
#define SHAPE_SLOTS 20u
#define SEGMENTS (SHAPE_SLOTS - 4u)
vertex GOut cell_shape_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const uint2* cells [[buffer(0)]],
                              device const Style* styles [[buffer(1)]],
                              constant CU& u [[buffer(2)]]) {
    GOut o;
    o.pos = float4(0.0); o.uv = float2(0.0); o.color = float4(0.0); o.kind = 2u;
    uint which = iid % SHAPE_SLOTS, ci = iid / SHAPE_SLOTS;
    uint col = ci % u.grid.x, dr = ci / u.grid.x;
    int lrow = int(dr) - int(u.slide_rows);
    // The top of this row's cell, shifted down by the smooth-scroll pan.
    float rowy = float(lrow) * float(u.cell.y) + float(u.pan);
    uint2 cell = cell_at(u, cells, col, dr);
    Style s = styles[cell.y & 0xffffu];
    uint fgw, bgw;
    colors(u, styles, cell, col, uint(lrow), fgw, bgw);
    float2 at; float2 wh; uint colw;
    float thick = float(max(u.thickness, 2u));
    if (which == 0u) {
        if ((s.shapes & 1u) == 0u) return o;
        at = float2(float(col * u.cell.x), rowy + float(u.underline));
        wh = float2(u.cell.x, u.thickness);
        colw = (s.shapes & 4u) != 0u ? fgw : s.ul;
    } else if (which == 1u) {
        if ((s.shapes & 2u) == 0u) return o;
        at = float2(float(col * u.cell.x), rowy + float(u.strike));
        wh = float2(u.cell.x, u.thickness);
        colw = fgw;
    } else if (which == 2u) {
        // The second line of a double underline, below the first by a gap and
        // clamped inside the cell.
        if ((s.shapes & 8u) == 0u) return o;
        float th = float(u.thickness);
        float gap = max(th, 1.0) + 1.0;
        float low = float(u.cell.y) - th;
        float y = min(float(u.underline) + gap, max(low, float(u.underline)));
        at = float2(float(col * u.cell.x), rowy + y);
        wh = float2(u.cell.x, u.thickness);
        colw = (s.shapes & 4u) != 0u ? fgw : s.ul;
    } else if (which == 3u) {
        if (col != u.cursor.x || lrow != int(u.cursor.y) || u.cursor_kind < 2u) return o;
        if (u.cursor_kind == 2u) {
            at = float2(float(col * u.cell.x), rowy + float(u.cell.y) - thick);
            wh = float2(u.cell.x, thick);
        } else {
            at = float2(float(col * u.cell.x), rowy);
            wh = float2(thick, u.cell.y);
        }
        colw = fgw;
    } else {
        // Segment k of a styled underline: the same arithmetic as the CPU
        // compositor's, so the two arms agree pixel for pixel.
        uint k = which - 4u;
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
        at = float2(float(col * u.cell.x) + dx, rowy + y);
        wh = float2(th, th);
        colw = (s.shapes & 4u) != 0u ? fgw : s.ul;
    }
    float2 c = float2(vid & 1u, vid >> 1u);
    o.pos = clip(at + c * wh, u);
    o.color = unpack(colw);
    return o;
}
