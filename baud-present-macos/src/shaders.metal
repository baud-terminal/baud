
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

// An effect trail's two record kinds, `Kind::Band` and `Kind::Particle`.
#define KIND_BAND 4u
#define KIND_PARTICLE 5u

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
    // A particle is a shape over its record: the effect's fragment function
    // reads where in it a pixel is, -1 to 1 on each axis.
    if (i.kind == KIND_PARTICLE) o.uv = c * 2.0 - 1.0;
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

// ---- The cursor trail's quad: `trail::coverage` in `baud-render`, the
// same rule. The record is the quad's bounds, drawn by `glyph_vertex`; this
// covers the pixels inside the corners by the non-zero winding rule, a pixel
// of anti-aliasing across each edge, and leaves the cursor's own rectangle
// bare. `origin` is where the frame sits in the target, so a pixel's centre
// in the frame is its position less it.
// The trail's smear: `mode` 0 is kitty's quad, 1 sfte's sweep, whose tail
// is the rectangle the corners hold and whose head is the hole.
struct Quad { float2 c[4]; float4 hole; float2 origin; float mode; float pad; };

fragment float4 quad_fragment(GOut in [[stage_in]], constant Quad& q [[buffer(0)]]) {
    float2 p = in.pos.xy - q.origin;
    if (p.x >= q.hole.x && p.x < q.hole.z && p.y >= q.hole.y && p.y < q.hole.w) {
        return float4(0.0);
    }
    if (q.mode > 0.5) {
        // sfte's `_sfte_render_trail`: the box swept from the tail's centre
        // to the cursor's, at `128 t` of 255 -- `trail::coverage`'s sweep.
        float2 c0 = (q.c[3] + q.c[1]) * 0.5;
        float2 r = (q.hole.zw - q.hole.xy) * 0.5;
        float2 ab = q.hole.xy + r - c0;
        float l2 = dot(ab, ab);
        if (l2 < 0.001) {
            return float4(0.0);
        }
        float t = clamp(dot(p - c0, ab) / l2, 0.0, 1.0);
        float2 off = abs(p - c0 - t * ab);
        if (off.x > r.x || off.y > r.y) {
            return float4(0.0);
        }
        float a = in.color.a * (128.0 * t / 255.0);
        return float4(in.color.rgb * a, a);
    }
    float d = INFINITY;
    int winding = 0;
    for (uint i = 0u; i < 4u; i++) {
        float2 a = q.c[i];
        float2 b = q.c[(i + 1u) & 3u];
        float2 ba = b - a;
        float2 pa = p - a;
        float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
        d = min(d, length(pa - ba * h));
        float side = ba.x * pa.y - ba.y * pa.x;
        if (a.y <= p.y) {
            if (b.y > p.y && side > 0.0) winding += 1;
        } else if (b.y <= p.y && side < 0.0) {
            winding -= 1;
        }
    }
    float s = winding != 0 ? -d : d;
    float a = in.color.a * clamp(0.5 - s, 0.0, 1.0);
    return float4(in.color.rgb * a, a);
}

// ---- The effect trails, `baud-render/src/effect.rs`: a fragment function
// each over the same records -- bands over the effect's reach, then particles
// -- in the cursor's colour the records carry, intensity its opacity. Each is
// a pure function of the pixel's place, the age since ignition and the seed;
// every number but a noise's own shape rides in `EffectU`, which Rust fills
// -- the shared prefix, then the effect's own `k` -- so there is one spelling
// of each. `origin` is where the frame sits in the target.
struct EffectU {
    float2 c[4]; float4 hole; float2 origin; float2 tail; float2 dir;
    float len; float unit; float age; float life; uint seed; float reach;
    float k[8];
};

// PCG's output permutation over one step of its LCG, the GPU hash Jarzynski
// and Olano recommend; `effect::Hash` is the same function.
static inline uint fx_hash(uint v) {
    uint s = v * 747796405u + 2891336453u;
    uint w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

// A number in [0, 1) from the hash's top 23 bits, through a float's mantissa:
// exact, with no conversion instruction.
static inline float fx_unit(uint h) {
    return as_type<float>((h >> 9u) | 0x3f800000u) - 1.0;
}

static inline float fx_lattice(int2 i, uint seed) {
    return fx_unit(fx_hash(uint(i.x) + fx_hash(uint(i.y) + seed)));
}

// Value noise on the hash's lattice, with the quintic fade.
static inline float fx_noise(float2 p, uint seed) {
    float2 f = floor(p);
    int2 i = int2(f);
    float2 x = p - f;
    float2 w = x * x * x * (x * (x * 6.0 - 15.0) + 10.0);
    float a = fx_lattice(i, seed);
    float b = fx_lattice(i + int2(1, 0), seed);
    float c = fx_lattice(i + int2(0, 1), seed);
    float d = fx_lattice(i + int2(1, 1), seed);
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}

// Four octaves, each rotated and at a detuned double of the last's scale.
static inline float fx_fbm(float2 p, uint seed) {
    float t = 0.0;
    float a = 0.5;
    for (uint o = 0u; o < 4u; o++) {
        t += a * fx_noise(p, seed + o * 0x9E3779B9u);
        p = float2(0.8 * p.x - 0.6 * p.y, 0.6 * p.x + 0.8 * p.y) * 2.02;
        a *= 0.5;
    }
    return t;
}

// The distance from `p` to the quad, negative inside it by the non-zero
// winding rule `quad_fragment` fills it by.
static inline float fx_distance(float2 p, constant EffectU& f) {
    float d = INFINITY;
    int winding = 0;
    for (uint i = 0u; i < 4u; i++) {
        float2 a = f.c[i];
        float2 b = f.c[(i + 1u) & 3u];
        float2 ba = b - a;
        float2 pa = p - a;
        float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
        d = min(d, length(pa - ba * h));
        float side = ba.x * pa.y - ba.y * pa.x;
        if (a.y <= p.y) {
            if (b.y > p.y && side > 0.0) winding += 1;
        } else if (b.y <= p.y && side < 0.0) {
            winding -= 1;
        }
    }
    return winding != 0 ? -d : d;
}

// Whether `p` is in the cursor's own rectangle, which every effect leaves
// bare, as the quad does.
static inline bool fx_in_hole(float2 p, constant EffectU& f) {
    return p.x >= f.hole.x && p.x < f.hole.z && p.y >= f.hole.y && p.y < f.hole.w;
}

// ---- The fire, `baud-render/src/fire.rs`: a flame rising off the quad's
// collapsing corners, and the embers it sheds. Its own numbers, `fire::PARAMS`:
// `k[0]` the flame's height, `k[1]` its rise, `k[2]` its drift.

// **The flame at `p`**: tongues where noise lifts a sample down into the
// quad, cooling with distance from it and burning out from the tail, and a
// halo on the page -- the cursor's colour at the heat's opacity,
// premultiplied, which is valid over a glass window and dark over a light
// theme's dark cursor alike.
static inline float4 fire_flame(float2 p, float4 color, constant EffectU& f) {
    if (fx_in_hole(p, f)) return float4(0.0);
    float height = f.k[0];
    float rise = f.k[1];
    float drift = f.k[2];
    float k = f.age / f.life;
    if (k >= 1.0) return float4(0.0);
    float c = f.unit;
    float d0 = fx_distance(p, f) / c;
    if (d0 > f.reach) return float4(0.0);
    float2 q = p / c;
    float2 s = float2(q.x * 2.6, (q.y + f.age * rise) * 0.7) - f.dir * (f.age * drift);
    float n = fx_fbm(s, f.seed);
    float lift = height * c * smoothstep(0.25, 0.85, n);
    float d = fx_distance(p + float2(0.0, lift), f) / c;
    float body = max(1.0 - smoothstep(-0.10, 0.20, d),
                     0.8 * (1.0 - smoothstep(-0.25, 0.05, d0)));
    float cool = saturate(max(d0, 0.0) / height);
    float u = clamp(dot(p - f.tail, f.dir) / max(f.len, 1.0), 0.0, 1.0);
    float burn = smoothstep(k - 0.15, k + 0.35, u) * (1.0 - k * k);
    float heat = saturate(body * (1.0 - 0.85 * cool) * (0.50 + 0.60 * n) * burn);
    float halo = 0.12 * exp(-2.5 * max(d0, 0.0)) * burn;
    float a = color.a * saturate(smoothstep(0.05, 0.95, heat) + halo);
    return float4(color.rgb * a, a);
}

// **An ember**: a soft disc filling its record, the cursor's colour at the
// ember's glow, which the record's alpha carries.
static inline float4 fire_ember(GOut in) {
    float a = 1.0 - smoothstep(0.15, 1.0, length(in.uv));
    a = in.color.a * a * a;
    return float4(in.color.rgb * a, a);
}

fragment float4 fire_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    if (in.kind == KIND_PARTICLE) return fire_ember(in);
    return fire_flame(in.pos.xy - f.origin, in.color, f);
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
    // `OSC 12` cursor colour, packed 0xFFRRGGBB, or 0 for none.
    uint cursor_color;
    // 1 in `SGR 5`'s off phase: the frame's, not a slot's, so a resolved
    // style is the same in both phases.
    uint blink_hidden;
    // The struct is 8-aligned by its `uint2`s, so it is 120 bytes and the
    // Rust side binds that many: `CellUniforms`, which checks it.
    uint pad;
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

// `Content::WIDE_TAIL`: the right half of a double-width character, the
// first value past U+10FFFF but one. `metallib.rs` pins it to the Rust
// constant, because the two are one fact spelled twice.
#define WIDE_TAIL 0x110001u

// `Shapes::BLINK` and `Shapes::CONCEAL`, one fact spelled twice and pinned
// by `the_hidden_text_bits_match_the_shader`.
#define SHAPE_BLINK 256u
#define SHAPE_CONCEAL 512u

// Whether the cell's own text -- its glyph, underline and strike -- is
// hidden: `SGR 8`, or `SGR 5` in its off phase. Hidden text draws its
// background and nothing of its own, not a glyph in the background's colour,
// which on glass lands at another alpha and shows. The cursor's shape still
// draws. Must match `Resolved::text`.
static bool hidden(constant CU& u, uint shapes) {
    return (shapes & SHAPE_CONCEAL) != 0u ||
           (u.blink_hidden != 0u && (shapes & SHAPE_BLINK) != 0u);
}

// Whether the cell is inside the selection, both ends inclusive: a stream
// (`sel_on` 1) in row-major order, a block (`sel_on` 2) by its row range
// and its column range apart. A wide tail takes its lead's answer, so a
// block's edge never cuts a character in two. Must match
// `Highlight::selects`; the pixel oracle compares the two arms.
static bool selected(constant CU& u, uint content, uint col, uint row) {
    if (u.sel_on == 0u) return false;
    if (content == WIDE_TAIL && col > 0u) col -= 1u;
    if (u.sel_on == 2u) {
        return row >= u.sel_from_row && row <= u.sel_to_row &&
               col >= u.sel_from_col && col <= u.sel_to_col;
    }
    bool after_from = row > u.sel_from_row || (row == u.sel_from_row && col >= u.sel_from_col);
    bool before_to = row < u.sel_to_row || (row == u.sel_to_row && col <= u.sel_to_col);
    return after_from && before_to;
}

// The cell's colours after the selection's swap and then the block
// cursor's, which the CPU path applies in the same order after the
// rendition rules (`Resolved::drawn`). Hiding comes after both and is not a
// colour: the glyph and shape passes skip a hidden cell's text.
static void colors(constant CU& u, device const Style* styles, uint2 cell,
                   uint col, uint row, thread uint& fgw, thread uint& bgw) {
    Style s = styles[cell.y & 0xffffu];
    fgw = s.fg; bgw = s.bg;
    // `[glass] app_backgrounds = "opaque"`: a background the application
    // set is drawn as it chose it, not as glass. The whole test is the
    // rendition's, baked into the slot as bit 128 (`Shapes::OPAQUE_BG`).
    // Must match `Renderer::convert_row`.
    if ((s.shapes & 128u) != 0u) {
        bgw |= 0xff000000u;
    }
    // Opaque on BOTH sides of a swap. Every background carries the
    // window's dim now, so the foreground a swap hands back would be
    // drawn at that alpha -- a glyph at 0.22 over its own cursor block.
    // This must match `Renderer::convert_row`, and the pixel gate
    // compares the two arms to say so.
    if (selected(u, cell.x, col, row)) {
        uint t = fgw; fgw = bgw | 0xff000000u; bgw = t | 0xff000000u;
    }
    if (u.cursor_kind == 1u && col == u.cursor.x && row == u.cursor.y) {
        uint t = fgw; fgw = bgw | 0xff000000u; bgw = t | 0xff000000u;
        // `OSC 12`: the block fills with the application's cursor colour
        // instead of the swapped foreground; the glyph stays `fgw`.
        if (u.cursor_color != 0u) bgw = u.cursor_color;
    }
}

vertex FlatOut cell_bg_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const uint2* cells [[buffer(0)]],
                              device const Style* styles [[buffer(1)]],
                              constant CU& u [[buffer(2)]],
                              device const uchar* overlay [[buffer(4)]]) {
    uint col = iid % u.grid.x, dr = iid / u.grid.x;
    int lrow = int(dr) - int(u.slide_rows);
    uint fgw, bgw;
    colors(u, styles, cell_at(u, cells, col, dr), col, uint(lrow), fgw, bgw);
    // A cell of a picture drawn in text keeps its background solid under
    // the dim (`MASK_PICTURE`, bit 2 of the overlay). After the swaps is
    // the same as before them: a swap already leaves both sides opaque.
    // Must match `Renderer::convert_row`.
    if (lrow >= 0 && (overlay[uint(lrow) * u.grid.x + col] & 2u) != 0u) {
        bgw |= 0xff000000u;
    }
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
    // Bit 1 (`MASK_RECORD`) is a cell a record draws; bit 2 is a picture's.
    if (lrow >= 0 && (overlay[uint(lrow) * u.grid.x + col] & 1u) != 0u) return o;
    uint2 cell = cell_at(u, cells, col, dr);
    uint content = cell.x;
    if (content > 0x10FFFFu) return o;
    if (hidden(u, styles[cell.y & 0xffffu].shapes)) return o;
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
    if (which != 3u && hidden(u, s.shapes)) return o;
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
        // `OSC 12`: the bar or underline cursor draws in the cursor colour.
        colw = (u.cursor_color != 0u) ? u.cursor_color : fgw;
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
