
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

// The background pass on a frame with a kitty image below `INT32_MIN/2`:
// the default backgrounds as `bg_fragment` draws them, and the cells whose
// background is not the default one (`ground`, 255 for `Ground::Set`,
// `Renderer::grounds`) the transparent zero, replacing, so what is under
// the image there is what is under the window. The CPU's `clear_set`.
fragment float4 bg_ground_fragment(BgOut in [[stage_in]],
                                   constant U& u [[buffer(0)]],
                                   texture2d<float, access::read> bg [[texture(0)]],
                                   texture2d<float, access::read> ground [[texture(1)]]) {
    float2 cf = (in.pos.xy - u.pad) / u.cell;
    if (cf.x >= u.grid.x || cf.y >= u.grid.y) return float4(0.0);
    if (ground.read(uint2(cf)).r != 0.0) return float4(0.0);
    float4 c = bg.read(uint2(cf));
    return float4(c.rgb * c.a, c.a);
}

// The background pass again, over a kitty image below `INT32_MIN/2`, for
// the cells whose background is not the default one alone, blended over
// the image at the background's own alpha, as over the desktop where
// there is no image, and as kitty blends its backgrounds over an image
// (`draw_cells_with_layers()`, `[[source: kitty-shaders-c]]`, drawing the
// non-default cells with bit 1 of kitty's background bitfield,
// `[[source: kitty-cell-slang]]`). The CPU's `cover_set`.
fragment float4 bg_set_fragment(BgOut in [[stage_in]],
                                constant U& u [[buffer(0)]],
                                texture2d<float, access::read> bg [[texture(0)]],
                                texture2d<float, access::read> ground [[texture(1)]]) {
    float2 cf = (in.pos.xy - u.pad) / u.cell;
    if (cf.x >= u.grid.x || cf.y >= u.grid.y) discard_fragment();
    if (ground.read(uint2(cf)).r == 0.0) discard_fragment();
    float4 c = bg.read(uint2(cf));
    return float4(c.rgb * c.a, c.a);
}

// `curve` is the text-contrast curve row a mask is bent by
// (`baud_domain::contrast`): a pixel looks its coverage up in that row.
struct GOut { float4 pos [[position]]; float2 uv; float4 color; uint kind [[flat]]; uint curve [[flat]]; };

// An effect trail's two record kinds, `Kind::Band` and `Kind::Particle`.
#define KIND_BAND 4u
#define KIND_PARTICLE 5u

vertex GOut glyph_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                         device const Inst* insts [[buffer(0)]],
                         constant U& u [[buffer(1)]]) {
    Inst i = insts[iid];
    // The kind word: the kind in the low half, a mask's contrast curve row
    // in the high half (`Instance::to_bytes`).
    uint kind = i.kind & 0xffffu;
    float2 c = float2(vid & 1u, vid >> 1u);
    float2 p = float2(i.xy) + c * float2(i.wh);
    float2 n = p / u.viewport;
    GOut o;
    o.pos = float4(n.x * 2.0 - 1.0, 1.0 - n.y * 2.0, 0.0, 1.0);
    float2 side = kind == 1u ? u.color : u.mask;
    o.uv = (float2(i.uv) + c * float2(i.wh)) / side;
    // A particle is a shape over its record: the effect's fragment function
    // reads where in it a pixel is, -1 to 1 on each axis.
    if (kind == KIND_PARTICLE) o.uv = c * 2.0 - 1.0;
    uint col = i.color;
    o.color = float4(float((col >> 16) & 0xffu), float((col >> 8) & 0xffu),
                     float(col & 0xffu), float(col >> 24)) / 255.0;
    o.kind = kind;
    o.curve = i.kind >> 16;
    return o;
}

// The source alpha of a pixel `cov` of 255 covered in a colour of alpha
// `color_a`: the two bytes' product, rounded to a byte as the CPU's
// `Target::over_translucent` and `Target::quad` round it through
// `baud_domain::premultiply_chan`, `(c a + 127) / 255`. Taken as
// a float product it falls between bytes, and a blend from it rounds to
// another byte than the CPU's on a sub-opaque colour. An opaque colour,
// every glyph's today, is the coverage itself and takes no arithmetic.
static inline float alpha_byte(float color_a, uint cov) {
    if (color_a >= 1.0) {
        return float(cov) / 255.0;
    }
    uint ca = uint(color_a * 255.0 + 0.5);
    return float((ca * cov + 127u) / 255u) / 255.0;
}

// A coverage in [0, 1] as the byte the CPU's `Target::quad` takes,
// `(c * 255).round()`: half away from zero, as `floor(x + 0.5)` is for x
// at least zero.
static inline uint coverage_byte(float c) {
    return uint(floor(c * 255.0 + 0.5));
}

fragment float4 glyph_fragment(GOut in [[stage_in]],
                               texture2d<float> mask [[texture(0)]],
                               texture2d<float> color [[texture(1)]],
                               sampler s [[sampler(0)]],
                               device const uchar* curves [[buffer(0)]]) {
    if (in.kind == 0u) {
        // The exact coverage, bent by the glyph's contrast curve: a lookup
        // into the compiled table, never arithmetic per pixel.
        uint cov = uint(mask.sample(s, in.uv).r * 255.0 + 0.5);
        float a = alpha_byte(in.color.a, curves[in.curve * 256u + cov]);
        return float4(in.color.rgb * a, a);
    }
    if (in.kind == 1u) {
        float4 c = color.sample(s, in.uv);
        return float4(c.rgb * c.a, c.a);
    }
    return float4(in.color.rgb * in.color.a, in.color.a);
}

// An image placement's pixel: drawn pixel (i, j) of its visible rectangle
// -- `uv`, which `glyph_vertex` hands over in pixels because `draw_images`
// sets the uniform's `color` to one -- shows texel (xs[i], ys[j]), the
// tables `Renderer::image_map` filled from `ImageDraw::nearest`. Read, not
// sampled, so the GPU shows the texel the CPU blit does. Straight in the
// texture and premultiplied once here, as `glyph_fragment`'s colour branch.
fragment float4 image_fragment(GOut in [[stage_in]],
                               texture2d<float, access::read> img [[texture(1)]],
                               device const uint* xs [[buffer(0)]],
                               device const uint* ys [[buffer(1)]]) {
    uint2 d = uint2(in.uv);
    float4 c = img.read(uint2(xs[d.x], ys[d.y]));
    return float4(c.rgb * c.a, c.a);
}

// A replacing placement's pixel (`ImageComposite::Replace`): the texel
// `image_fragment` reads, written in place of what is under it by a
// pipeline with blending off -- but a clear texel is discarded, leaving
// what is under it, as the CPU's store skips it (`Target::blit_replacing`).
fragment float4 image_replace_fragment(GOut in [[stage_in]],
                                       texture2d<float, access::read> img [[texture(1)]],
                                       device const uint* xs [[buffer(0)]],
                                       device const uint* ys [[buffer(1)]]) {
    uint2 d = uint2(in.uv);
    float4 c = img.read(uint2(xs[d.x], ys[d.y]));
    if (c.a == 0.0) discard_fragment();
    return float4(c.rgb * c.a, c.a);
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
        float a = alpha_byte(in.color.a, coverage_byte(128.0 * t / 255.0));
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
    float a = alpha_byte(in.color.a, coverage_byte(clamp(0.5 - s, 0.0, 1.0)));
    return float4(in.color.rgb * a, a);
}

// ---- The effect trails, `baud-render/src/effect.rs`: a fragment function
// each over the same records -- bands over the effect's reach, then particles
// -- in the cursor's colour the records carry, intensity its opacity. Each is
// a pure function of the pixel's place, the age since ignition and the seed;
// every number but a noise's own shape rides in `EffectU`, which Rust fills
// -- the shared prefix, then the effect's own `k` -- so there is one spelling
// of each. `origin` is where the frame sits in the target.
// `cap` is how a band's light meets its record's alpha, `effect::Cap`: 0
// scaled by it, 1 clamped at it, `min(light, alpha)` -- the plasma's repeat
// pass. Three words pad the struct to 144 bytes.
struct EffectU {
    float2 c[4]; float4 hole; float2 origin; float2 tail; float2 dir;
    float len; float unit; float age; float life; uint seed; float reach;
    float k[8]; uint cap; uint pad[3];
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
// collapsing corners, and the embers it sheds. Its own numbers, `fire::params`:
// `k[0]` the flame's height, `k[1]` its rise, `k[2]` its drift, and `k[3]`
// how far its path has ridden a smooth scroll, which the noise is anchored
// on so a flame that rides keeps its shape. Zero for one that never rode.

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
    float2 q = (p - float2(0.0, f.k[3])) / c;
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

// **A particle** -- an ember, an ion: a soft disc filling its record, the
// cursor's colour at the particle's glow, which the record's alpha carries.
static inline float4 fx_particle(GOut in) {
    float a = 1.0 - smoothstep(0.15, 1.0, length(in.uv));
    a = in.color.a * a * a;
    return float4(in.color.rgb * a, a);
}

fragment float4 fire_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    if (in.kind == KIND_PARTICLE) return fx_particle(in);
    return fire_flame(in.pos.xy - f.origin, in.color, f);
}

// ---- The plasma, `baud-render/src/plasma.rs`: a tube filling the quad, a
// sheath around it, and three filaments braided along the axis into the
// cursor. Its own numbers, `plasma::params`: `k[0]` the braid's half width in
// cells, `k[1..3]` the axis, `k[3]` the tube, `k[4]` the sheath, `k[5]` its
// decay, `k[6]` the filaments' gain, `k[7]` the knots' rate. The filaments'
// shapes are the table below, the same one `plasma_reference` reads.

// sin(2 pi x), its argument folded into [-1, 1) first.
static inline float px_wave(float x) { return sinpi(2.0 * fract(x + 0.5) - 1.0); }

// Per filament: cycles a cell and a second of its wave, of its harmonic, the
// harmonic's share, its width in cells and its gain.
constant float3 PX_K  = float3(0.23, 0.31, 0.17);
constant float3 PX_F  = float3(1.10, 1.35, 0.80);
constant float3 PX_K2 = float3(0.71, 0.93, 0.53);
constant float3 PX_F2 = float3(-0.85, -1.05, -1.20);
constant float3 PX_M  = float3(0.22, 0.20, 0.25);
constant float3 PX_W  = float3(0.040, 0.034, 0.050);
constant float3 PX_G  = float3(1.00, 0.85, 0.65);

// **The plasma at `p`**: the tube and its sheath dimming together, and the
// braid behind the head -- nothing ahead of it -- converging into the cursor.
static inline float4 plasma_tube(float2 p, float4 color, constant EffectU& f) {
    if (fx_in_hole(p, f)) return float4(0.0);
    float k = f.age / f.life;
    if (k >= 1.0) return float4(0.0);
    float c = f.unit;
    float d0 = fx_distance(p, f) / c;
    if (d0 > f.reach) return float4(0.0);
    float env = (1.0 - k) * (1.0 - k);
    float inside = 1.0 - smoothstep(-0.03, 0.03, d0);
    float tube = f.k[3] * inside + f.k[4] * exp(-f.k[5] * (1.0 + 1.5 * k) * max(d0, 0.0));
    float fil = 0.0;
    if (d0 < 0.15) {
        float2 a = float2(f.k[1], f.k[2]);
        float2 r = p - 0.5 * (f.hole.xy + f.hole.zw);
        float s = -dot(r, a) / c;
        float w = (a.x * r.y - a.y * r.x) / c;
        // The ignition's hand, `plasma::Hand`: a leftward move's braid is
        // the rightward one's mirror image, its offset across negated.
        if (f.dir.x < 0.0) w = -w;
        float amp = f.k[0] * 0.62 * smoothstep(0.0, 2.0, s);
        uint salt = f.seed ^ 0xA511E9B3u;
        float kn = fx_unit(fx_hash(salt + 8u));
        for (uint i = 0u; i < 3u; i++) {
            float ph  = fx_unit(fx_hash(salt + i));
            float ph2 = fx_unit(fx_hash(salt + 16u + i));
            float along = amp * ((1.0 - PX_M[i]) * px_wave(PX_K[i] * s + ph + PX_F[i] * f.age)
                               + PX_M[i] * px_wave(PX_K2[i] * s + ph2 + PX_F2[i] * f.age));
            float wid = max(PX_W[i] * (1.0 - 0.5 * k), 1.1 / c);
            float e = (w - along) / wid;
            float knot = 0.72 + 0.28 * px_wave(0.45 * s + kn + float(i) / 3.0 + f.k[7] * f.age);
            fil += PX_G[i] * exp(-e * e) * knot;
        }
        fil = saturate(fil) * f.k[6] * (1.0 - smoothstep(-0.02, 0.15, d0))
            * smoothstep(0.0, 0.3, s);
    }
    float light = saturate(env * (tube + (1.0 - tube) * fil));
    float alpha = f.cap == 1u ? min(light, color.a) : color.a * light;
    return float4(color.rgb * alpha, alpha);
}

fragment float4 plasma_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    if (in.kind == KIND_PARTICLE) return fx_particle(in);
    return plasma_tube(in.pos.xy - f.origin, in.color, f);
}

// **The plasma by `multiply`** (`composite::Mix`): `plasma_fragment`'s
// premultiplied colour, and as the second source the factor its backdrop is
// taken at, `1 - as + cs` a channel, so the blend state's
// `OneMinusDestinationAlpha, Source1Color` forms W3C Compositing's
// `cs (1 - ab) + cb (1 - as + cs)` over the glass and an opaque page alike.
// Under `screen` the plasma is `plasma_fragment` itself, by `One,
// OneMinusSourceColor`: `cs + cb (1 - cs)`.
struct MixOut {
    float4 color [[color(0), index(0)]];
    float4 factor [[color(0), index(1)]];
};

fragment MixOut plasma_multiply_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    float4 c = in.kind == KIND_PARTICLE ? fx_particle(in)
                                        : plasma_tube(in.pos.xy - f.origin, in.color, f);
    MixOut o;
    o.color = c;
    o.factor = float4(1.0 - c.a + c.rgb, 1.0);
    return o;
}

// ---- The lightning, `baud-render/src/lightning.rs`: a bolt from the cell
// the cursor left to the cell it reached, over a box along the path (the
// uniform's corners, `lightning::region`) that does not move. Its own numbers,
// `lightning::PARAMS`: `k[0]` the core's half width and `k[1]` the glow's
// width in cells, `k[2]` the glow's peak, `k[3]` the leader's crossing, `k[4]`
// a pixel's rise and `k[5]` when the fade starts, in seconds, `k[6]` the
// corona's peak and `k[7]` its width in cells. The bolt's own shape is the
// `#define`s, which `baud-present-macos/tests/metallib.rs` holds to the Rust
// constants of the same names; `lightning::light` is this in Rust.
#define BOLT_JAG 0.8
#define BOLT_CAP 0.25
#define BOLT_S_MIN 0.30
#define BOLT_LEVELS 8u
#define BOLT_SALT 0x51ED270Bu
#define FORK_SALT 0xA24BAED4u
#define FORK_MIN 5.0
#define FORK_P 0.75

// The distance from `p` to the segment `a` to `b`.
static inline float fx_segment(float2 p, float2 a, float2 b) {
    float2 ba = b - a;
    float2 pa = p - a;
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

// As many levels as keep every segment at least BOLT_S_MIN cells long.
static inline uint bolt_levels(float len, float c) {
    uint l = 0u;
    while (l < BOLT_LEVELS && len >= (c * float(1u << (l + 1u))) * BOLT_S_MIN) l++;
    return l;
}

// Midpoint displacement at `u` as its Schauder sum: one tent a level, zero at
// both ends.
static inline float bolt_offset(float u, uint lv, constant EffectU& f) {
    float y = 0.0;
    for (uint l = 0u; l < lv; l++) {
        float n = float(1u << l);
        float x = u * n;
        float i = floor(x);
        float t = x - i;
        float tent = 1.0 - abs(2.0 * t - 1.0);
        float xi = 2.0 * fx_lattice(int2(int(i), int(l)), f.seed ^ BOLT_SALT) - 1.0;
        float d = min(BOLT_JAG * f.len / (2.0 * n), BOLT_CAP * f.unit);
        y += d * xi * tent;
    }
    return y;
}

static inline float2 bolt_vertex(int j, int m, uint lv, float2 n, constant EffectU& f) {
    float u = float(j) / float(m);
    return f.tail + f.dir * (f.len * u) + n * bolt_offset(u, lv, f);
}

// The distance to fork `k`, at a quarter of the path a fork, 25-45 degrees
// off it, two segments -- infinity where it did not grow.
static inline float bolt_fork(float2 p, uint k, uint lv, float2 n, constant EffectU& f) {
    uint h0 = fx_hash(k + fx_hash(f.seed ^ FORK_SALT));
    if (fx_unit(h0) >= FORK_P) return INFINITY;
    uint h1 = fx_hash(h0); uint h2 = fx_hash(h1); uint h3 = fx_hash(h2); uint h4 = fx_hash(h3);
    float q = 0.25 * float(k + 1u);
    float yr = bolt_offset(q, min(lv, 2u), f);
    float side = abs(yr) > 0.6 * f.unit ? -sign(yr) : (fx_unit(h1) < 0.5 ? 1.0 : -1.0);
    float2 v = f.dir + n * (side * (0.466 + 0.534 * fx_unit(h2)));
    float2 fd = v / sqrt(dot(v, v));
    float lf = min((0.18 + 0.14 * fx_unit(h3)) * f.len, 1.2 * f.unit);
    float2 r = f.tail + f.dir * (f.len * q) + n * yr;
    float bend = (fx_unit(h4) - 0.5) * 0.3 * lf;
    float2 e = r + fd * (0.5 * lf) + float2(-fd.y, fd.x) * bend;
    float2 t = r + fd * lf;
    return min(fx_segment(p, r, e), fx_segment(p, e, t));
}

// **The bolt at `p`**: its core, glow and forks as the leader reaches the
// pixel and the fade takes it, and the corona round the cursor.
static inline float4 bolt_light(float2 p, float4 color, constant EffectU& f) {
    if (fx_in_hole(p, f)) return float4(0.0);
    if (f.age >= f.life) return float4(0.0);
    float c = f.unit;
    if (fx_distance(p, f) / c > f.reach) return float4(0.0);
    float u = dot(p - f.tail, f.dir) / f.len;
    float tau = f.k[3] * clamp(u, 0.0, 1.0);
    float rise = smoothstep(tau, tau + f.k[4], f.age);
    if (rise <= 0.0) return float4(0.0);
    float fall = 1.0 - smoothstep(f.k[5], f.life, f.age);
    float2 n = float2(-f.dir.y, f.dir.x);
    uint lv = bolt_levels(f.len, c);
    int m = int(1u << lv);
    int a0 = max(clamp(int(floor(u * float(m))), 0, m - 1) - 1, 0);
    float2 w0 = bolt_vertex(a0, m, lv, n, f);
    float2 w1 = bolt_vertex(min(a0 + 1, m), m, lv, n, f);
    float2 w2 = bolt_vertex(min(a0 + 2, m), m, lv, n, f);
    float2 w3 = bolt_vertex(min(a0 + 3, m), m, lv, n, f);
    float dm = min(min(fx_segment(p, w0, w1), fx_segment(p, w1, w2)), fx_segment(p, w2, w3));
    float df = INFINITY;
    if (f.len >= FORK_MIN * c) {
        for (uint k = 0u; k < 3u; k++) df = min(df, bolt_fork(p, k, lv, n, f));
    }
    float2 hc = (f.hole.xy + f.hole.zw) * 0.5;
    float2 hh = (f.hole.zw - f.hole.xy) * 0.5;
    float dh = length(max(abs(p - hc) - hh, 0.0));
    float w = max(f.k[0] * c, 1.0) * (0.45 + 0.55 * fall);
    float gw = f.k[1] * c;
    float km = (dm / w) * (dm / w);
    float kf = (df / (0.7 * w)) * (df / (0.7 * w));
    float core = max(exp(-km * km), 0.7 * exp(-kf * kf));
    float gm = dm / gw;
    float gf = df / (0.75 * gw);
    float glow = f.k[2] * max(exp(-gm * gm), 0.6 * exp(-gf * gf));
    float gc = dh / (f.k[7] * c);
    // Unlike the spec, no gate of its own: every corona pixel is where the
    // leader arrives last, so the pixel's own rise gates it already.
    float corona = f.k[6] * exp(-gc * gc);
    float a = color.a * saturate(core * fall * fall + (glow + corona) * fall) * rise;
    return float4(color.rgb * a, a);
}

fragment float4 lightning_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    if (in.kind == KIND_PARTICLE) return fx_particle(in);
    return bolt_light(in.pos.xy - f.origin, in.color, f);
}

// ---- The ice, `baud-render/src/ice.rs`: a pane over the quad, and over the
// path a frost lace of six-fold crystals, a glint and a rime haze, melting
// tail first; shards are hexagonal plates. Its own numbers, `ice::PARAMS`:
// `k[0]` the crystals' spacing, `k[1]` the longest arm, `k[2]` the line's half
// width, `k[3]` the side branches' spacing, in cells; `k[4]` a crystal's
// growth, `k[5]` the hold, `k[6]` the melt's spread and `k[7]` a crystal's
// melt, in seconds. `ICE_LEAD` and the salts are held to the Rust constants
// by the metallib test; `ice::light` is this in Rust.
#define ICE_LEAD 0.01
#define ICE_SALT 0x1CE01CE0u
#define RIME_SALT 0x51ED0000u

static inline float ice_line(float d, float w, float aa) { return 1.0 - smoothstep(w - aa, w + aa, d); }

// When the frost reaches `u` of the path: kitty's leading corners, inverted.
static inline float ice_dep(float u) { return -ICE_LEAD * log2(max(1.0 - 0.999 * u, 1e-3)); }

// Crystal `k` at `rel`: rotated by a hashed angle through the tangent
// half-angle form, folded into one 30-degree wedge by two mirrors, then one
// arm, its side branches and its plate.
static inline float ice_crystal(float k, float2 rel, float2 D, float2 Pp, float lu,
                                float W, float aa, uint sbase, constant EffectU& f) {
    float S = f.k[0], ARM = f.k[1], B = f.k[3], GROW = f.k[4], HOLD = f.k[5], SPREAD = f.k[6], MELT = f.k[7];
    uint h1 = fx_hash(uint(int(k)) + sbase);
    uint h2 = fx_hash(h1), h3 = fx_hash(h2), h4 = fx_hash(h3), h5 = fx_hash(h4), h6 = fx_hash(h5);
    float along = (k + (fx_unit(h1) - 0.5) * 0.5) * S;
    float across = (fx_unit(h2) - 0.5) * 0.7;
    float win = smoothstep(-0.25, 0.0, along) * (1.0 - smoothstep(lu, lu + 0.25, along));
    if (win <= 0.0) return 0.0;
    float u = saturate(along / lu);
    float delay = 0.03 * fx_unit(h5);
    float x = saturate((f.age - ice_dep(u) - delay) / GROW);
    float grow = 1.0 - (1.0 - x) * (1.0 - x) * (1.0 - x);
    float m = HOLD + SPREAD * u + delay;
    float fade = 1.0 - smoothstep(m, m + MELT, f.age);
    float L = ARM * (0.45 + 0.55 * fx_unit(h4)) * grow * (0.35 + 0.65 * fade);
    float exist = saturate(L / (2.0 * W));
    if (exist <= 0.0 || fade <= 0.0) return 0.0;
    float2 r = rel - (D * along + Pp * across);
    float s = 0.57735027 * fx_unit(h3);
    float den = 1.0 + s * s;
    float cs = (1.0 - s * s) / den, sn = 2.0 * s / den;
    float2 w = abs(float2(cs * r.x + sn * r.y, -sn * r.x + cs * r.y));
    const float2 nA = float2(-0.8660254, 0.5), nB = float2(-0.5, 0.8660254);
    w -= 2.0 * max(dot(w, nA), 0.0) * nA;
    w -= 2.0 * max(dot(w, nB), 0.0) * nB;
    float dm = length(w - float2(clamp(w.x, 0.0, L), 0.0));
    float wm = W * (1.4 - 0.7 * saturate(w.x / max(L, 1e-4)));
    float mainA = 0.95 * ice_line(dm, wm, aa);
    const float2 e = float2(0.5, 0.8660254);
    float xr = w.x - w.y * 0.57735027;
    float xj = max(floor(xr / B + 0.5), 1.0) * B;
    float lj = min(0.6 * (L - xj), xj);
    float2 v = w - float2(xj, 0.0);
    float ds = length(v - clamp(dot(v, e), 0.0, max(lj, 0.0)) * e);
    float sideA = 0.60 * ice_line(ds, W, aa) * saturate(lj / (2.0 * W));
    float pr = 0.10 + 0.30 * fx_unit(h6) * fx_unit(h6);
    float dp = dot(w, float2(0.8660254, 0.5)) - pr * L * 0.8660254;
    float plateA = 0.30 * (1.0 - smoothstep(-aa, aa, dp));
    return max(mainA, max(sideA, plateA)) * exist * fade * win;
}

// **The ice at `p`**: the pane over the quad, and near the path the three
// nearest crystals with the glint, and the rime.
static inline float4 ice_frost(float2 p, float4 color, constant EffectU& f) {
    if (fx_in_hole(p, f)) return float4(0.0);
    float c = f.unit, lu = f.len / c;
    float S = f.k[0], HOLD = f.k[5], SPREAD = f.k[6], MELT = f.k[7];
    float2 rel = (p - f.tail) / c;
    float2 D = f.dir, Pp = float2(-D.y, D.x);
    float along = dot(rel, D);
    float dseg = length(rel - D * clamp(along, 0.0, lu));
    float dq = fx_distance(p, f);
    if (dseg > f.reach && dq / c > f.reach) return float4(0.0);
    float pa = saturate(1.0 - f.age / (0.6 * HOLD));
    float pane = pa * max(0.14 * (1.0 - smoothstep(-0.5, 0.5, dq)),
                          0.5 * (1.0 - smoothstep(0.5, 1.5, abs(dq))));
    float a = pane;
    if (dseg <= f.reach) {
        float W = max(f.k[2], 0.75 / c), aa = 0.5 / c;
        uint sbase = fx_hash(f.seed + ICE_SALT);
        float k0 = floor(along / S + 0.5);
        float frost = 0.0;
        for (int dk = -1; dk <= 1; dk++)
            frost = max(frost, ice_crystal(k0 + float(dk), rel, D, Pp, lu, W, aa, sbase, f));
        float gp = (f.age - 0.4 * HOLD) / (0.6 * HOLD) * (lu + 2.0 * S) - S;
        frost = saturate(frost * (1.0 + 0.3 * (1.0 - smoothstep(0.0, S, abs(along - gp)))));
        float u = saturate(along / lu);
        float front = (lu + 0.6) * (1.0 - exp2(-f.age / ICE_LEAD)) - 0.3;
        float dep = 1.0 - smoothstep(front - 0.3, front + 0.3, along);
        float mr = HOLD + SPREAD * u;
        float rfade = 1.0 - smoothstep(mr, mr + MELT, f.age);
        uint s2 = f.seed ^ RIME_SALT;
        float2 q2 = float2(0.8 * rel.x - 0.6 * rel.y, 0.6 * rel.x + 0.8 * rel.y);
        float n = 0.65 * fx_noise(rel * 2.3, s2) + 0.35 * fx_noise(q2 * 6.7, s2 + 0x9E3779B9u);
        float rime = 0.24 * (1.0 - smoothstep(0.15, 0.6, dseg)) * smoothstep(0.35, 0.75, n) * dep * rfade;
        a = max(a, max(frost, rime));
    }
    a *= color.a;
    return float4(color.rgb * a, a);
}

// iq's hexagon (MIT), for a shard: a plate with its flats at `r`.
static inline float ice_hex(float2 p, float r) {
    const float3 k = float3(-0.866025404, 0.5, 0.577350269);
    p = abs(p);
    p -= 2.0 * min(dot(k.xy, p), 0.0) * k.xy;
    p -= float2(clamp(p.x, -k.z * r, k.z * r), r);
    return length(p) * sign(p.y);
}

fragment float4 ice_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    if (in.kind == KIND_PARTICLE) {
        float d = ice_hex(in.uv, 0.70);
        float a = in.color.a * (1.0 - smoothstep(-0.08, 0.12, d))
                * (0.55 + 0.45 * smoothstep(-0.35, -0.02, d));
        return float4(in.color.rgb * a, a);
    }
    return ice_frost(in.pos.xy - f.origin, in.color, f);
}

// ---- The ufo, `baud-render/src/ufo.rs`: a saucer springing along the path
// onto kitty's quad's centroid, a wake, a tractor beam onto the cursor, and
// exhaust rings. Its bands cover only the box `ufo::region` names. Its own
// numbers, `ufo::PARAMS`: `k[0]` the hover in cells, `k[1]` the size, `k[2]`
// the beam's peak, `k[3]` the wake's, `k[4]` the rim lights' turns a second,
// `k[5]` the arc in cells, `k[6]` the bank in radians, `k[7]` the earliest
// age it leaves at. `ufo::light` is this in Rust.
struct UfoPose { float2 c; float cs, sn, s, vis, cr, sr, side, gap, beam, ext; };

static inline float ufo_cov(float d) { return clamp(0.5 - d, 0.0, 1.0); }

static inline float ufo_flight(constant EffectU& f) {
    return clamp(0.12 + 0.006 * f.len / f.unit, 0.16, 0.32);
}

static inline float ufo_omega(constant EffectU& f) { return 4.0 / ufo_flight(f); }

// When the saucer leaves: after its beam, 1.6 flights and a quarter second
// in, never before `k[7]`.
static inline float ufo_depart(constant EffectU& f) {
    return max(f.k[7], 1.6 * ufo_flight(f) + 0.25);
}

// **The saucer at `a` seconds**: a critically damped spring from the tail to
// the quad's centroid, an arc away from the cursor, a hover and a bob, a bank
// into the travel, and its departure.
static inline UfoPose ufo_pose(constant EffectU& f, float a) {
    float c = f.unit;
    float2 cq = (f.c[0] + f.c[1] + f.c[2] + f.c[3]) * 0.25;
    float w = ufo_omega(f);
    float sig = 1.0 - (1.0 + w * a) * exp(-w * a);
    float2 on = f.tail + (cq - f.tail) * sig;
    float2 g = (f.hole.xy + f.hole.zw) * 0.5 - on;
    float gap = length(g);
    float side = f.hole.y >= 1.35 * c ? 1.0 : -1.0;
    float2 pth = cq - f.tail;
    float pl = length(pth);
    float2 n = pl > 1e-3 ? float2(-pth.y, pth.x) / pl : float2(0.0, -side);
    if (n.y * side > 0.0) n = -n;
    float arc = min(f.k[5] * c, 0.12 * f.len) * 4.0 * sig * (1.0 - sig);
    float hov = 1.0 - smoothstep(0.1 * c, 0.6 * c, gap);
    float h1 = fx_unit(fx_hash(f.seed ^ 0x5bd1e995u));
    float h2 = fx_unit(fx_hash(f.seed));
    float bob = 0.035 * c * sinpi(2.0 * fract(2.2 * a + h1) - 1.0) * hov;
    float t = ufo_flight(f);
    float dep = ufo_depart(f);
    float tl = max(a - dep, 0.0);
    float kk = clamp(tl / 0.18, 0.0, 1.0);
    UfoPose P;
    P.c = on + n * arc + float2(0.0, -side * f.k[0] * c + bob - side * 46.0 * c * tl * tl);
    float th = f.k[6] * clamp(g.x / (3.0 * c), -1.0, 1.0)
             + 0.05 * sinpi(2.0 * fract(1.3 * a + h2) - 1.0) * hov;
    P.cs = cos(th);
    P.sn = sin(th);
    float grow = smoothstep(0.0, 0.08, a);
    P.vis = grow * (1.0 - smoothstep(0.0, 1.0, kk));
    P.s = f.k[1] * (0.5 + 0.5 * grow) * (1.0 - 0.6 * kk * kk);
    float rho = 2.0 * fract(f.k[4] * a + h2) - 1.0;
    P.cr = cospi(rho);
    P.sr = sinpi(rho);
    P.side = side;
    P.gap = gap;
    P.ext = smoothstep(1.3 * t, 1.3 * t + 0.1, a) * (1.0 - smoothstep(dep - 0.08, dep, a));
    P.beam = P.ext * (1.0 - smoothstep(0.15 * c, 0.6 * c, gap));
    return P;
}

// The hull: a vesica, pointed tips along x.
static inline float ufo_vesica(float2 q) {
    float2 p = abs(q.yx);
    if ((p.y - 0.53) * 1.221818 > p.x * 0.53) return length(p - float2(0.0, 0.53));
    return length(p + float2(1.221818, 0.0)) - 1.331818;
}

// The dome: iq's cut disk, radius 0.22 cut at its centre, y up.
static inline float ufo_dome(float2 p) {
    const float r = 0.22;
    p.x = abs(p.x);
    float s = max(-r * p.x * p.x + r * r * (r - 2.0 * p.y), -r * p.y);
    return s < 0.0 ? length(p) - r : (p.x < r ? -p.y : length(p - float2(r, 0.0)));
}

static inline float ufo_smin(float a, float b, float k) {
    k *= 4.0;
    float h = max(k - abs(a - b), 0.0) / k;
    return min(a, b) - h * h * k * 0.25;
}

constant float2 UFO_LIGHTS[5] = {
    float2(1.0, 0.0), float2(0.309017, 0.951057), float2(-0.809017, 0.587785),
    float2(-0.809017, -0.587785), float2(0.309017, -0.951057)
};

// **The saucer's body**: the hull, the dome and the rim lights.
static inline float ufo_body(float2 p, UfoPose P, float c) {
    float u = P.s * c;
    float2 d = p - P.c;
    float lim = 0.62 * u + 1.0;
    if (dot(d, d) > lim * lim) return 0.0;
    float2 q = float2(P.cs * d.x + P.sn * d.y, -P.sn * d.x + P.cs * d.y) / u;
    float dh = ufo_vesica(q) - 0.02;
    float sil = ufo_smin(dh, ufo_dome(float2(q.x, -0.04 - q.y)), 0.03);
    float light = 0.0;
    if (abs(q.y - 0.03) < 0.09) {
        for (uint i = 0u; i < 5u; i++) {
            float cphi = P.cr * UFO_LIGHTS[i].x - P.sr * UFO_LIGHTS[i].y;
            float sphi = P.sr * UFO_LIGHTS[i].x + P.cr * UFO_LIGHTS[i].y;
            float dl = length(q - float2(0.40 * sphi, 0.03)) - 0.045;
            light = max(light, smoothstep(0.0, 0.35, cphi) * ufo_cov(dl * u));
        }
    }
    return (0.50 * ufo_cov(sil * u) + ufo_cov(dh * u) * (0.30 + 0.20 * light)) * P.vis;
}

// **The tractor beam**: a trapezoid from under the saucer to the cursor's
// near edge, a brighter core, rings travelling down it.
static inline float ufo_beam(float2 p, UfoPose P, constant EffectU& f) {
    if (P.beam <= 0.0) return 0.0;
    float c = f.unit;
    float2 top = P.c + float2(0.0, P.side * 0.117 * c * P.s);
    float2 end = float2((f.hole.x + f.hole.z) * 0.5, P.side > 0.0 ? f.hole.y : f.hole.w);
    float2 ax = end - top;
    float ln = length(ax);
    if (ln < 1.0) return 0.0;
    float2 nn = ax / ln;
    float2 d = p - top;
    float along = dot(d, nn);
    float across = abs(d.x * nn.y - d.y * nn.x);
    float ff = clamp(along / ln, 0.0, 1.0);
    float wide = max(0.5 * (f.hole.z - f.hole.x) + 0.08 * c, 0.22 * c);
    float hw = 0.14 * c * P.s + (wide - 0.14 * c * P.s) * ff;
    float edge = clamp(hw - across + 0.5, 0.0, 1.0) * clamp(ln * P.ext - along + 0.5, 0.0, 1.0)
               * clamp(along + 0.5, 0.0, 1.0);
    if (edge <= 0.0) return 0.0;
    float core = 1.0 - smoothstep(0.0, 0.6, across / max(hw, 1.0));
    float fr = fract(along / (0.22 * c) - fract(5.0 * f.age));
    return f.k[2] * P.vis * P.beam * edge * (0.55 + 0.45 * core) * (0.6 + 0.4 * ff)
         * (1.0 - 0.88 * fr * (1.0 - fr));
}

// **The wake**: a tapered streak along the saucer's last 60 ms.
static inline float ufo_wake(float2 p, UfoPose P, constant EffectU& f) {
    float c = f.unit;
    if (f.age <= 0.0) return 0.0;
    UfoPose Q = ufo_pose(f, max(f.age - 0.06, 0.0));
    float2 v = P.c - Q.c;
    float l = length(v);
    if (l < 0.1 * c) return 0.0;
    float2 a = P.c - v / l * (0.2475 * c * P.s);
    float2 ba = Q.c - a;
    float2 pa = p - a;
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    float dd = length(pa - ba * h) - (0.055 * c + (0.012 * c - 0.055 * c) * h);
    return f.k[3] * P.vis * ufo_cov(dd) * (1.0 - h) * smoothstep(0.1 * c, 0.6 * c, l);
}

fragment float4 ufo_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    if (in.kind == KIND_PARTICLE) {
        float a = in.color.a * (1.0 - smoothstep(0.0, 0.22, abs(length(in.uv) - 0.72)));
        return float4(in.color.rgb * a, a);
    }
    float2 p = in.pos.xy - f.origin;
    if (fx_in_hole(p, f) || f.age >= ufo_depart(f) + 0.18) return float4(0.0);
    UfoPose P = ufo_pose(f, f.age);
    float a = ufo_wake(p, P, f);
    float b = ufo_beam(p, P, f);
    a = b + a * (1.0 - b);
    float s = ufo_body(p, P, f.unit);
    a = s + a * (1.0 - s);
    a *= in.color.a;
    return float4(in.color.rgb * a, a);
}

// ---- The orb, `baud-render/src/orb.rs`: a glowing sphere flying from the
// tail onto the cursor through kitty's leading corners, a halo, a comet of
// its last flight, and sparks as it lands. Its bands cover the line it flies
// on. Its own
// numbers, `orb::PARAMS`: `k[0]` its radius and `k[1]` its halo's width in
// cells, `k[2]` the halo's peak, `k[3]` the comet's, `k[4]` the comet's
// seconds, `k[5]` the appear, `k[6]` the fade, `k[7]` the linger after
// arrival. `orb::light` is this in Rust.

static inline float orb_flight(constant EffectU& f) {
    return clamp(0.10 + 0.004 * f.len / f.unit, 0.12, 0.25);
}

// How far along its flight the orb is at `a`: a smootherstep over the
// flight, from rest to rest, 1 exactly at its end.
static inline float orb_along(constant EffectU& f, float a) {
    float u = clamp(a / orb_flight(f), 0.0, 1.0);
    return u * u * u * (u * (u * 6.0 - 15.0) + 10.0);
}

// Where the orb flies to: the cursor's centre, displaced by the mean of how
// far the quad's corners facing the ignition's direction -- those past the
// cursor's centre along it -- still are from the cursor's. The cursor's
// corners are in kitty's order, as `c`.
static inline float2 orb_aim(constant EffectU& f) {
    float2 m = float2((f.hole.x + f.hole.z) * 0.5, (f.hole.y + f.hole.w) * 0.5);
    float2 hc[4] = {float2(f.hole.z, f.hole.y), float2(f.hole.z, f.hole.w),
                    float2(f.hole.x, f.hole.w), float2(f.hole.x, f.hole.y)};
    float2 s = float2(0.0);
    float n = 0.0;
    for (int i = 0; i < 4; i++) {
        if ((hc[i].x - m.x) * f.dir.x + (hc[i].y - m.y) * f.dir.y > 0.0) {
            s += f.c[i] - hc[i];
            n += 1.0;
        }
    }
    return n > 0.0 ? m + s / n : m;
}

// Where the orb is at `a`, on the line from the tail to its aim.
static inline float2 orb_centre(constant EffectU& f, float2 aim, float a) {
    return f.tail + (aim - f.tail) * orb_along(f, a);
}

// The distance from `p` to the line the orb flies on, tail to its aim.
static inline float orb_line(float2 p, constant EffectU& f, float2 aim) {
    float2 ba = aim - f.tail;
    float2 pa = p - f.tail;
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

static inline float4 orb_light(float2 p, float4 color, constant EffectU& f) {
    if (fx_in_hole(p, f)) return float4(0.0);
    float c = f.unit;
    float t = orb_flight(f);
    float end = t + f.k[7] + f.k[6];
    if (f.age >= end) return float4(0.0);
    float2 aim = orb_aim(f);
    if (orb_line(p, f, aim) / c > f.reach) return float4(0.0);
    float env = smoothstep(0.0, f.k[5], f.age) * (1.0 - smoothstep(t + f.k[7], end, f.age));
    float2 o = orb_centre(f, aim, f.age);
    float r = f.k[0] * c;
    float d = length(p - o);
    float cov = clamp(0.5 - (d - r), 0.0, 1.0);
    float k = min(d / r, 1.0);
    float hs = length(p - (o - 0.35 * r)) / (0.30 * r);
    float body = (0.45 + 0.30 * (1.0 - k * k) + 0.25 * exp(-hs * hs)) * cov;
    float hd = max(d - r, 0.0) / (f.k[1] * c);
    float halo = f.k[2] * exp(-hd * hd);
    float2 q = orb_centre(f, aim, max(f.age - f.k[4], 0.0));
    float2 ba = q - o;
    float2 pa = p - o;
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    float ds = length(pa - ba * h);
    float cd = ds / (r * (0.8 - 0.6 * h));
    float comet = f.k[3] * (1.0 - h) * (1.0 - h) * exp(-cd * cd);
    float a = color.a * saturate(max(max(body, halo), comet) * env);
    return float4(color.rgb * a, a);
}

fragment float4 orb_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    if (in.kind == KIND_PARTICLE) return fx_particle(in);
    return orb_light(in.pos.xy - f.origin, in.color, f);
}

// ---- The liquid, `baud-render/src/liquid.rs`: a glass gel along a spine
// from the back of kitty's quad to the cursor -- a necked tube, satellite
// beads and a head drop in one smooth union -- with a rim and a glint on a
// squircle bevel. The state is derived here from the prefix, as
// `liquid::state` derives it in Rust. Its own numbers, `liquid::PARAMS`:
// `k[0]` the tube's radius, `k[1]` the union's width, `k[2]` the head's gap,
// `k[3]` its wobble, `k[4]` the bevel, in cells; `k[5]` the body's, `k[6]` the
// rim's and `k[7]` the glint's opacity.
#define LQ_TRAIL 0.04
#define LQ_ZETA 0.28
#define LQ_OMEGA 40.840706
#define LQ_BACK 0.25
#define LQ_CROSS 0.6
#define LQ_LEAN 0.08
#define LQ_LEAN_AT 1.2
#define LQ_REST_X -0.5
#define LQ_REST_Y -0.866
#define LQ_COUNT_SALT 0x5bd1e995u
#define LQ_BEAD_SALT 0x9E3779B9u
#define LQ_BEAD_IN 0.02

struct LiquidS {
    float2 b; float2 h; float radius; float neck; float inv_len2; float lip;
    float2 half_box; float round; float k; float3 bead[3]; float2 light; float envelope; float end;
};

static inline float lq_tube(float2 d, float w, float h, constant EffectU& f) {
    return f.k[0] * length(float2(d.y * w, d.x * h));
}

// The trailing corners' contact with the head, and the wobble's amplitude.
static inline float2 lq_contact(float len, float c, float radius, constant EffectU& f) {
    float t = LQ_TRAIL * log2(max(len / (radius + f.k[2] * c), 1.0));
    return float2(t, f.k[3] * c * smoothstep(3.0, 12.0, len / c));
}

static inline float lq_end(float len, float c, float radius, constant EffectU& f) {
    float2 ta = lq_contact(len, c, radius, f);
    float tw = ta.y > 0.5 ? log(ta.y / 0.5) / (LQ_ZETA * LQ_OMEGA) : 0.0;
    return max(ta.x + tw + 0.05, 0.22);
}

static inline LiquidS liquid_state(constant EffectU& f) {
    LiquidS s;
    float c = f.unit;
    float2 d = f.dir;
    float hw = f.hole.z - f.hole.x, hh = f.hole.w - f.hole.y;
    s.h = (f.hole.xy + f.hole.zw) * 0.5;
    float2 m = (f.c[0] + f.c[1] + f.c[2] + f.c[3]) * 0.25;
    float lo = INFINITY, hi = INFINITY;
    for (uint i = 0u; i < 4u; i++) {
        float v = dot(f.c[i] - m, d);
        if (v < lo) { hi = lo; lo = v; } else if (v < hi) { hi = v; }
    }
    s.b = m + d * (0.5 * (lo + hi));
    float l = length(s.h - s.b);
    s.inv_len2 = l > 1e-3 ? 1.0 / (l * l) : 0.0;
    // The cell's extent: the cursor's, floored at the nominal cell.
    float r = lq_tube(d, max(hw, 0.5 * c), max(hh, c), f);
    s.radius = r * smoothstep(0.0, 2.0 * r, l);
    float neck_max = 1.45 * smoothstep(1.5, 4.0, f.len / c);
    s.neck = neck_max * smoothstep(0.03, 0.14, f.age);
    s.lip = l > 1e-3 ? 4.0 * s.neck * s.radius / l : 0.0;
    float nominal = lq_tube(d, 0.5 * c, c, f);
    float2 ta = lq_contact(f.len, c, nominal, f);
    float sw = f.age - ta.x;
    float wob = 0.0;
    if (sw > 0.0) {
        float wd = LQ_OMEGA * sqrt(1.0 - LQ_ZETA * LQ_ZETA);
        wob = ta.y * exp(-LQ_ZETA * LQ_OMEGA * sw) * sin(wd * sw);
    }
    float ax = abs(d.x), ay = abs(d.y);
    s.round = f.k[2] * c + f.k[3] * c + 1.0;
    s.half_box = float2(max(0.5 * hw + f.k[2] * c + wob * ax - LQ_CROSS * wob * ay, s.round),
                        max(0.5 * hh + f.k[2] * c + wob * ay - LQ_CROSS * wob * ax, s.round));
    for (uint j = 0u; j < 3u; j++) s.bead[j] = float3(0.0);
    if (neck_max > 1.0) {
        float xp = 0.5 - sin(asin(1.0 - 2.0 / neck_max) / 3.0);
        float tp = 0.03 + 0.11 * xp;
        float sg = f.age - tp;
        uint count = 2u + (fx_hash(f.seed ^ LQ_COUNT_SALT) & 1u);
        float2 n = float2(-d.y, d.x);
        for (uint j = 0u; j < count; j++) {
            if (sg < 0.0) break;
            uint h0 = fx_hash(f.seed ^ ((j + 1u) * LQ_BEAD_SALT));
            uint h1 = fx_hash(h0), h2 = fx_hash(h1), h3 = fx_hash(h2);
            float u0 = 0.5 + (fx_unit(h0) - 0.5) * 0.16;
            float tau = 0.045 + 0.04 * fx_unit(h2);
            float u = u0 + (1.0 - u0) * (1.0 - exp(-sg / tau));
            float rj = r * (0.30 - 0.07 * float(j)) * (0.85 + 0.3 * fx_unit(h1))
                     * (1.0 - smoothstep(0.85, 0.99, u)) * smoothstep(0.0, LQ_BEAD_IN, sg);
            float o = (fx_unit(h3) - 0.5) * 0.25 * r * exp(-sg / 0.06);
            s.bead[j] = float3(s.b + (s.h - s.b) * u + n * o, rj);
        }
    }
    // The light starts leaning into the move -- the rest light plus
    // LQ_LEAN_AT of the move's direction -- and turns back to the upper left
    // the short way round, by its angle: a blend of the two vectors over time
    // passes through nothing for a move opposite the rest light.
    float rest = atan2(LQ_REST_Y, LQ_REST_X);
    float turn = atan2(LQ_REST_Y + LQ_LEAN_AT * d.y, LQ_REST_X + LQ_LEAN_AT * d.x) - rest;
    if (turn > M_PI_F) turn -= 2.0 * M_PI_F; else if (turn <= -M_PI_F) turn += 2.0 * M_PI_F;
    float th = rest + turn * exp(-f.age / LQ_LEAN);
    s.light = float2(cos(th), sin(th));
    s.k = f.k[1] * c;
    s.end = lq_end(f.len, c, nominal, f);
    s.envelope = smoothstep(0.0, 0.035, f.age) * (1.0 - smoothstep(s.end - 0.10, s.end, f.age));
    return s;
}

// iq's quadratic smooth minimum, with its gradient.
static inline float3 lq_smin(float3 a, float3 b, float k) {
    float h = 1.0 - min(abs(a.x - b.x) / (4.0 * k), 1.0);
    float s = h * h * k;
    float w = 0.5 * h;
    return a.x < b.x ? float3(a.x - s, a.yz + (b.yz - a.yz) * w)
                     : float3(b.x - s, b.yz + (a.yz - b.yz) * w);
}

static inline float2 lq_dir(float2 v, float l) { return l > 1e-3 ? v / l : float2(0.0); }

static inline float3 liquid_field(float2 p, LiquidS s) {
    float2 ph = p - s.h;
    float2 sg = float2(ph.x >= 0.0 ? 1.0 : -1.0, ph.y >= 0.0 ? 1.0 : -1.0);
    float2 w = abs(ph) - (s.half_box - s.round);
    float g = max(w.x, w.y);
    float3 f;
    if (g > 0.0) {
        float2 wp = max(w, 0.0);
        float lw = length(wp);
        f = float3(lw - s.round, sg * wp / lw);
    } else {
        f = float3(g - s.round, w.x > w.y ? float2(sg.x, 0.0) : float2(0.0, sg.y));
    }
    if (s.inv_len2 > 0.0 && s.radius > 0.0) {
        float2 e = s.h - s.b;
        float u = clamp(dot(p - s.b, e) * s.inv_len2, 0.0, 1.0);
        float2 v = p - (s.b + e * u);
        float l = length(v);
        float q = 1.0 - 4.0 * s.neck * u * (1.0 - u);
        float sl = s.lip * (2.0 * u - 1.0);
        f = lq_smin(f, float3((l - s.radius * q) / sqrt(1.0 + sl * sl), lq_dir(v, l)), s.k);
    }
    for (uint i = 0u; i < 3u; i++) {
        float3 bd = s.bead[i];
        if (bd.z > 0.0) {
            float2 v = p - bd.xy;
            float l = length(v);
            f = lq_smin(f, float3(l - bd.z, lq_dir(v, l)), s.k);
        }
    }
    return f;
}

// **The gel at `p`**: its body, rim, glint and counter-glint.
static inline float4 liquid_light(float2 p, float4 color, constant EffectU& f) {
    if (fx_in_hole(p, f)) return float4(0.0);
    LiquidS s = liquid_state(f);
    float c = f.unit;
    if (f.age >= s.end) return float4(0.0);
    float2 e = s.h - s.b;
    float u0 = clamp(dot(p - s.b, e) * s.inv_len2, 0.0, 1.0);
    float ds = s.inv_len2 > 0.0 ? length(p - (s.b + e * u0)) : INFINITY;
    float2 o = max(max(f.hole.xy - p, p - f.hole.zw), 0.0);
    if (min(ds, length(o)) > f.reach * c) return float4(0.0);
    float3 fd = liquid_field(p, s);
    float cov = clamp(0.5 - fd.x, 0.0, 1.0);
    if (cov <= 0.0) return float4(0.0);
    float t = clamp(-fd.x / (f.k[4] * c), 0.0, 1.0);
    float sv = 1.0 - t;
    float s2 = sv * sv;
    float s3 = s2 * sv;
    float z = sqrt(sqrt(max(1.0 - s2 * s2, 0.0)));
    float z3 = z * z * z;
    float nz = z3 / sqrt(max(s3 * s3 + z3 * z3, 1e-12));
    float fr = 1.0 - nz;
    float rim = 0.04 + 0.96 * fr * fr;
    float gl = dot(fd.yz, fd.yz);
    float2 n = gl > 1e-12 ? fd.yz / sqrt(gl) : float2(0.0);
    float bt = clamp((t - 0.04) * 2.5, 0.0, 1.0);
    float band = 4.0 * bt * (1.0 - bt);
    float nl = dot(n, s.light);
    float sp = max(nl, 0.0);
    sp = sp * sp * sp * band;
    float bk = max(-nl, 0.0);
    bk = bk * bk * bk * band;
    float a = color.a * s.envelope * cov
            * saturate(f.k[5] + f.k[6] * rim + f.k[7] * sp + LQ_BACK * bk);
    return float4(color.rgb * a, a);
}

fragment float4 liquid_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    if (in.kind == KIND_PARTICLE) return fx_particle(in);
    return liquid_light(in.pos.xy - f.origin, in.color, f);
}

// ---- iTerm2's fireworks, `baud-render/src/fireworks.rs`: a five-pointed
// star a record, in its colour at its opacity, turned by its `u` (1/65536
// of a turn a step) and its image `v` sixteenths of a pixel a side, the
// hole round the cursor cut from every one. `fireworks::coverage` and
// `fireworks::hole` are the same formulas; the coverage is formed as the
// byte the CPU's `Target::fireworks` takes. `FwU` is `fireworks::uniform`:
// where the frame sits in the target, the hole's centre and radius.
struct FwU { float2 origin; float2 hole; float radius; float pad[3]; };
struct FwOut {
    float4 pos [[position]]; float4 color [[flat]];
    float2 mid [[flat]]; float2 turn [[flat]]; float image [[flat]];
};

#define FW_TIP 0.4935f
#define FW_NOTCH 0.205f
#define FW_DROP 0.04f

vertex FwOut fireworks_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const Inst* insts [[buffer(0)]],
                              constant U& u [[buffer(1)]]) {
    Inst i = insts[iid];
    float2 c = float2(vid & 1u, vid >> 1u);
    float2 p = float2(i.xy) + c * float2(i.wh);
    float2 n = p / u.viewport;
    FwOut o;
    o.pos = float4(n.x * 2.0 - 1.0, 1.0 - n.y * 2.0, 0.0, 1.0);
    uint col = i.color;
    o.color = float4(float((col >> 16) & 0xffu), float((col >> 8) & 0xffu),
                     float(col & 0xffu), float(col >> 24)) / 255.0;
    // The centre of the record's middle pixel, `fireworks::centre`.
    float half_side = float(i.wh.x / 2u);
    o.mid = float2(i.xy) + half_side + 0.5;
    float th = float(i.uv.x) * (6.28318530717958647692 / 65536.0);
    float cs;
    float sn = precise::sincos(th, cs);
    o.turn = float2(cs, sn);
    o.image = float(i.uv.y) / 16.0;
    return o;
}

// `fireworks::star_distance`: the signed distance to the star, y up, a tip
// at (0, tip), folded across its mirror lines at 18 and 54 degrees.
static inline float fw_star(float2 p, float tip, float notch) {
    float2 q = float2(abs(p.x), p.y);
    const float2 m18 = float2(0.9510565f, 0.309017f);
    const float2 m54 = float2(0.58778524f, 0.809017f);
    float d = -m18.y * q.x + m18.x * q.y;
    if (d < 0.0) q = float2(q.x + 2.0 * d * m18.y, q.y - 2.0 * d * m18.x);
    d = -m54.y * q.x + m54.x * q.y;
    if (d < 0.0) q = float2(q.x + 2.0 * d * m54.y, q.y - 2.0 * d * m54.x);
    q.x = abs(q.x);
    float ex = notch * m54.x;
    float ey = notch * m54.y - tip;
    float px = q.x;
    float py = q.y - tip;
    float h = clamp((px * ex + py * ey) / (ex * ex + ey * ey), 0.0, 1.0);
    float dx = px - ex * h;
    float dy = py - ey * h;
    float dist = sqrt(dx * dx + dy * dy);
    return (ex * py - ey * px < 0.0) ? -dist : dist;
}

fragment float4 fireworks_fragment(FwOut in [[stage_in]], constant FwU& f [[buffer(0)]]) {
    float2 p = in.pos.xy - f.origin;
    float2 q = p - in.mid;
    float c = in.turn.x;
    float s = in.turn.y;
    float x = q.x * c - q.y * s;
    float y = q.x * s + q.y * c - FW_DROP * in.image;
    float d = fw_star(float2(x, -y), FW_TIP * in.image, FW_NOTCH * in.image);
    float star = clamp(0.5 - d, 0.0, 1.0);
    float2 h = p - f.hole;
    float hole = clamp(sqrt(h.x * h.x + h.y * h.y) - f.radius + 0.5, 0.0, 1.0);
    float a = alpha_byte(in.color.a, coverage_byte(star * hole));
    return float4(in.color.rgb * a, a);
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
    // The cursor colour -- a program's `OSC 12`, else the configured one,
    // else a theme's -- packed 0xFFRRGGBB, or `PAINT_PAGE` for the cell's
    // page, or 0 for none: the cell's text. `paint()` reads it.
    uint cursor_color;
    // The blink shapes this frame hides -- `SHAPE_BLINK` in `SGR 5`'s off
    // phase, `SHAPE_RAPID_BLINK` in `SGR 6`'s: the frame's, not a slot's, so
    // a resolved style is the same in every phase. `BlinkPhases::hidden`.
    uint blink_hidden;
    // How much of a blinking cursor this frame draws, 255 all of it:
    // `Renderer::cursor_opacity`, below 255 part way through a fade. The
    // struct is 8-aligned by its `uint2`s, so it is 120 bytes and the Rust
    // side binds that many: `CellUniforms`, which checks it.
    uint cursor_alpha;
    // The selection's colours, `OSC 17`'s page and `OSC 19`'s text, packed
    // 0xFFRRGGBB, or `PAINT_TEXT`/`PAINT_PAGE` for the cell's own, or 0 for
    // none -- both none is reverse video -- each with the contrast curve row
    // a glyph in a colour takes.
    uint sel_bg; uint sel_fg; uint sel_bg_curve; uint sel_fg_curve;
    // The text under a block cursor: packed 0xFFRRGGBB, or `PAINT_TEXT` for
    // the cell's own text, or 0 for none -- the cell's page -- and the curve
    // row a glyph in a colour takes. 144 bytes with these.
    uint cursor_text; uint cursor_text_curve;
    // The blink shapes this frame draws faint rather than hides, under
    // dimmed flashing lights: `BlinkPhases::faint`. And the side of its
    // cell a bar cursor is drawn on, 1 the right where the character under
    // it runs right to left (`baud_render::CursorSide`), so the struct stays
    // a multiple of eight bytes: 152.
    uint blink_faint; uint cursor_side;
};

// A paint word: 0 none, the cell's text or page by name, else an opaque
// colour, which is never either (`Hilite::of`, `BlockPaint::of`).
#define PAINT_TEXT 1u
#define PAINT_PAGE 2u
static uint paint(uint w, uint none, uint text, uint page) {
    return w == 0u ? none : w == PAINT_TEXT ? text : w == PAINT_PAGE ? page : w;
}
struct Style { uint fg; uint bg; uint ul; uint shapes; uint curves; uint glass; uint gcurves;
               uint line; uint lcurves; };
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
// `Content::EMPTY`: nothing was written here, pinned beside it.
#define EMPTY_CELL 0x110000u

// The stress mark's skip trie, `baud_render::STRESS_SKIP_INDEX` and
// `STRESS_SKIP_BLOCKS` as `uchar` arrays of the same bytes, written into
// `OUT_DIR` by `baud-present-macos/build.rs` from what `baud-render`'s
// build compiles out of the pinned UCD files.
#include "stress_skips.metal"

// `baud_render::MASK_BARE`: the cell holds a cluster whose base the
// stress mark leaves bare, which the CPU judges from the cluster's base and
// marks; the shader sees only the cluster's slot. Pinned by
// `the_bare_mask_bit_matches_the_shader`.
#define MASK_BARE 16u
// `baud_render::MASK_SIZED`: the cell is on a line that is not
// single-width (DEC STD 070 5.6.1), which the records draw whole. The
// background pass draws column `col` from the cell under `col / 2`, so a
// character's background covers both its columns, and the decoration pass
// draws nothing there. Pinned by `the_sized_mask_bit_matches_the_shader`.
#define MASK_SIZED 4u
// `baud_render::MASK_RECORD`: a record draws the cell. Pinned by
// `the_record_mask_bit_matches_the_shader`.
#define MASK_RECORD 1u
// `baud_render::MASK_BIDI` and `MASK_SELECTED`: the cell is on a row drawn
// in another order, and whether it is selected is the second bit, which
// the overlay's walk set from the logical cell it shows. Pinned by
// `the_bidi_mask_bits_match_the_shader`.
#define MASK_BIDI 32u
#define MASK_SELECTED 64u

// The record and sized bits of the cell at column `col` of logical row
// `lrow`: the screen's row of the overlay, and for the leaving row above
// the window (`lrow` -1) the row after the screen's, which carries only
// those two bits and only when the leaving line is not single-width
// (`baud_render::Overlay::leaving`).
static uint sized_bits(constant CU& u, device const uchar* overlay, uint col, int lrow) {
    uint row = lrow >= 0 ? uint(lrow) : u.grid.y;
    return uint(overlay[row * u.grid.x + col]) & (MASK_RECORD | MASK_SIZED);
}

// `takes_stress_mark`: whether a cell's word takes `SGR 64`'s mark -- not
// a blank, not a wide tail (its lead marks both cells), not a codepoint
// the web's emphasis marks leave bare (the table above). A cluster, above
// U+10FFFF, takes it unless the overlay marks it bare (`bare`). Must match
// `baud_render::takes_stress_mark`.
static bool takes_stress_mark(uint content, bool bare) {
    if (content == EMPTY_CELL || content == WIDE_TAIL) return false;
    if (content > 0x10FFFFu) return !bare;
    uint block = uint(STRESS_SKIP_INDEX[content >> 8]);
    uint byte = uint(STRESS_SKIP_BLOCKS[block * 32u + ((content & 0xffu) >> 3)]);
    return ((byte >> (content & 7u)) & 1u) == 0u;
}

// `Shapes::BLINK`, `Shapes::CONCEAL` and `Shapes::RAPID_BLINK`, one fact
// spelled twice and pinned by `the_hidden_text_bits_match_the_shader`.
#define SHAPE_BLINK 256u
#define SHAPE_CONCEAL 512u
#define SHAPE_RAPID_BLINK 4096u
// `Shapes::DEFAULT_BG`: the rendition draws the default background colour,
// pinned by `the_default_background_bit_matches_the_shader`.
#define SHAPE_DEFAULT_BG 8192u
// `Shapes::OVERLINE`, `Shapes::DOUBLE_OVERLINE` and `Shapes::FRAMED`,
// pinned by `the_line_shape_bits_match_the_shader`.
#define SHAPE_OVERLINE 16384u
#define SHAPE_DOUBLE_OVERLINE 32768u
#define SHAPE_FRAMED 65536u
// `Shapes::STRESS` and `Shapes::ENCIRCLED`, pinned by the same test.
#define SHAPE_STRESS 131072u
#define SHAPE_ENCIRCLED 262144u
// `baud_render::boxes::Ring::KEY`: the glyph map holds an encircled run's
// three parts under this key plus 1 (the left end), 2 (the right end) and
// 3 (a cell alone), above every codepoint, face style and span. Pinned by
// `the_line_shape_bits_match_the_shader`.
#define RING_KEY 134217728u
// The face style's two bits in a resolved slot's shapes -- the style whose
// face the cell's text is drawn from, already fallen back by the stack --
// and where they sit in a glyph-map key, above the 21-bit codepoint. The
// CPU spells the same key (`CellKey::of`), pinned by
// `the_face_style_bits_match_the_shader`.
#define FACE_SHIFT 10u
#define KEY_STYLE_SHIFT 21u
// Set in the key when the character spans two cells -- the next cell is
// its wide tail: its glyph is centred in them, a slot of its own. Must
// match `CellKey::SPAN_SHIFT`.
#define KEY_SPAN_SHIFT 26u
// The font a resolved slot's text is drawn in (`Shapes::FONT_SHIFT`,
// `FontSlot`): the alternative font of SGR 11-19 in its low four bits and
// SGR 20's Fraktur above. In a key the Fraktur bit sits between the
// style and the span and the alternative in the top four bits, clear of
// RING_KEY's bit; the primary font adds nothing, so its keys are the ones
// a stack with no fonts builds. Pinned by
// `the_face_style_bits_match_the_shader`.
#define FONT_SHIFT 19u
#define KEY_FRAKTUR_SHIFT 23u
#define KEY_FONT_SHIFT 28u

// Whether the cell's own text -- its glyph, underline and strike -- is
// hidden: `SGR 8`, or `SGR 5` or `SGR 6` in its own rate's off phase, which
// `blink_hidden` names by the shape it hides. Hidden text draws its
// background and nothing of its own, not a glyph in the background's colour,
// which on glass lands at another alpha and shows. The cursor's shape still
// draws. Must match `Resolved::text`.
static bool hidden(constant CU& u, uint shapes) {
    uint blinks = u.blink_hidden & (SHAPE_BLINK | SHAPE_RAPID_BLINK);
    return (shapes & (SHAPE_CONCEAL | blinks)) != 0u;
}

// Whether the cell's own text draws faint: `SGR 5` or `SGR 6` in its own
// rate's off phase while the frame draws that phase faint, `blink_faint`.
// Concealed text is hidden first. Must match `Resolved::text`.
static bool faint(constant CU& u, uint shapes) {
    uint blinks = u.blink_faint & (SHAPE_BLINK | SHAPE_RAPID_BLINK);
    return !hidden(u, shapes) && (shapes & blinks) != 0u;
}

// Whether the cell beside column `col` of draw row `dr` -- to its left or
// its right -- carries a run of `shape` on: it has the shape and its text
// draws, since a hidden cell draws nothing of its own and the run's
// outline closes beside it. The row's edge carries nothing on. Must match
// `carries_on` in baud-render's renderer.rs (called from `Renderer::edges`
// and `ring_part`).
static bool carries(constant CU& u, device const uint2* cells,
                    device const Style* styles, uint col, uint dr, bool left,
                    uint shape) {
    bool edge = left ? col == 0u : col + 1u >= u.grid.x;
    if (edge) return false;
    uint2 n = cell_at(u, cells, left ? col - 1u : col + 1u, dr);
    uint ns = styles[n.y & 0xffffu].shapes;
    return (ns & shape) != 0u && !hidden(u, ns);
}

// Whether the cell is inside the selection, both ends inclusive: a stream
// (`sel_on` 1) in row-major order, a block (`sel_on` 2) by its row range
// and its column range apart. A wide tail takes its lead's answer, so a
// block's edge never cuts a character in two. On a row drawn in another
// order the overlay holds the answer for the logical cell (`MASK_BIDI`).
// Must match `Highlight::selects`; the pixel oracle compares the two arms.
static bool selected(constant CU& u, device const uchar* overlay, uint content, uint col,
                     uint row) {
    if (u.sel_on == 0u) return false;
    if (row < u.grid.y) {
        uint m = uint(overlay[row * u.grid.x + col]);
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
// `(x l + y (255 - l) + 127) / 255` in integers, `Argb::mix`'s sum, so the
// two arms agree to the bit on a fading cursor.
static uint mix_level(uint w, uint other, uint level) {
    uint out = 0u;
    for (uint s = 0u; s < 32u; s += 8u) {
        uint x = (w >> s) & 0xffu, y = (other >> s) & 0xffu;
        out |= ((x * level + y * (255u - level) + 127u) / 255u) << s;
    }
    return out;
}

// `w` with its alpha scaled by `level`, rounded: `Argb::scaled`.
static uint scaled(uint w, uint level) {
    uint a = ((w >> 24) * level + 127u) / 255u;
    return (w & 0x00ffffffu) | (a << 24);
}

// `w` half way to `other`, each colour channel rounded half up, `w`'s alpha
// kept: `Argb::faint_on`, `SGR 2`'s mix.
static uint faint_on(uint w, uint other) {
    uint out = w & 0xff000000u;
    for (uint s = 0u; s < 24u; s += 8u) {
        uint x = (w >> s) & 0xffu, y = (other >> s) & 0xffu;
        out |= ((x + y + 1u) / 2u) << s;
    }
    return out;
}

// An underline in a colour of its own as this frame draws it: faint with
// the text. `Drawn::rule`.
static uint rule(constant CU& u, Style s, uint bgw) {
    return faint(u, s.shapes) ? faint_on(s.ul, bgw) : s.ul;
}

// Whether the cell's page is drawn solid under the dim: its rendition's
// `[glass] app_backgrounds = "opaque"`, baked into the slot as bit 128
// (`Shapes::OPAQUE_BG`), or a picture drawn in text (`MASK_PICTURE`, bit 2
// of the overlay, never on the leaving row above the window).
static bool solid(constant CU& u, uint shapes, device const uchar* overlay,
                  uint col, int lrow) {
    return (shapes & 128u) != 0u
        || (lrow >= 0 && (overlay[uint(lrow) * u.grid.x + col] & 2u) != 0u);
}

// Whether the cell is a box-drawing line, which the contrast floor leaves
// as asked: Unicode's Box Drawing block. Must match `Line::of`, pinned by
// `the_line_range_matches_the_shader`.
#define LINE_FIRST 0x2500u
#define LINE_LAST 0x257Fu
static bool is_line(uint content) {
    return content >= LINE_FIRST && content <= LINE_LAST;
}

// The contrast curve row a cell's glyph takes under `block`, while it is
// drawn unswapped: a line's (`lcurves`), else the glass text's (`gcurves`)
// over a page left dimmed; else the solid text's, each by the swaps'
// parity -- the selection's swap and the block cursor's each exchange the
// two colours, and a cursor colour replaces only the background. Must match
// `Resolved::drawn`, `Curves::row` and `Swaps::ink`.
static uint curve_under(constant CU& u, Style s, device const uchar* overlay,
                        uint content, uint col, int lrow, bool block) {
    bool sel = selected(u, overlay, content, col, uint(lrow));
    bool swap = sel != block;
    // Under a highlight colour the glyph is in the selection's text, or,
    // swapped again by a block, its page: a colour's row where one is set,
    // else the cell's own text's or page's. `Resolved::hilite_curve`.
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
                : own && !solid(u, s.shapes, overlay, col, lrow) ? s.gcurves
                : s.curves;
    return swap ? (curves >> 16) : (curves & 0xffffu);
}

// The curve row of a cell's glyph: a fading block counts from half way,
// and a block's text colour is its own. `Resolved::hilite_curve`.
static uint glyph_curve(constant CU& u, Style s, device const uchar* overlay,
                        uint content, uint col, int lrow) {
    bool block = u.cursor_kind == 1u && col == u.cursor.x && uint(lrow) == u.cursor.y
                 && u.cursor_alpha >= 128u;
    // A block's text colour: a colour's own row, or the cell's solid text
    // as the selection leaves it, the curve the cell takes with no block.
    if (block && u.cursor_text == PAINT_TEXT) {
        if (!selected(u, overlay, content, col, uint(lrow))) return s.curves & 0xffffu;
        return curve_under(u, s, overlay, content, col, lrow, false);
    }
    if (block && u.cursor_text != 0u) return u.cursor_text_curve;
    return curve_under(u, s, overlay, content, col, lrow, block);
}

// The cell's colours after the selection's swap and then the block
// cursor's, which the CPU path applies in the same order after the
// rendition rules (`Resolved::drawn`). Hiding comes after both and is not a
// colour: the glyph and shape passes skip a hidden cell's text.
static void colors(constant CU& u, device const Style* styles,
                   device const uchar* overlay, uint2 cell,
                   uint col, int lrow, thread uint& fgw, thread uint& bgw,
                   thread uint& curw) {
    Style s = styles[cell.y & 0xffffu];
    uint row = uint(lrow);
    // A page drawn solid -- `[glass] app_backgrounds = "opaque"` or a
    // picture -- takes the solid text, `fg`; a page left dimmed takes the
    // text the floor lifted against it, `glass`; a line takes `line`.
    // Must match `Resolved::drawn`.
    if (solid(u, s.shapes, overlay, col, lrow)) {
        fgw = s.fg; bgw = s.bg | 0xff000000u;
    } else {
        fgw = s.glass; bgw = s.bg;
    }
    // What a bar or underline cursor falls back to: the text before the
    // line rule, `Drawn::cursor_fg`.
    curw = fgw;
    // A box-drawing line draws the text as asked, `line`, on either page.
    if (is_line(cell.x)) fgw = s.line;
    // Opaque on BOTH sides of a swap, and the text a swap hands to the
    // background is the solid one. Every background carries the
    // window's dim now, so the foreground a swap hands back would be
    // drawn at that alpha -- a glyph at 0.22 over its own cursor block.
    // This must match `Renderer::convert_row`, and the pixel gate
    // compares the two arms to say so.
    uint ink = s.fg;
    if (selected(u, overlay, cell.x, col, row)) {
        // `Hilite::paint`: the page `OSC 17`'s where set, else the text; the
        // text `OSC 19`'s where set, else the cell's own on a page that is
        // not the cell's text, else the page -- reverse video when neither
        // is set. Either may name the cell's own text or page.
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
        // cell's own: `BlockPaint`.
        bgw = paint(u.cursor_color, text0, text0, page0);
        fgw = paint(u.cursor_text, page0, text0, page0);
        // A block part way through a fade: each colour `cursor_alpha` of the
        // way from the cell without it, `Resolved::drawn`'s mix.
        if (u.cursor_alpha < 255u) {
            fgw = mix_level(fgw, f0, u.cursor_alpha);
            bgw = mix_level(bgw, b0, u.cursor_alpha);
        }
    }
    // A faint off phase, after every swap: the glyph half way to the page
    // it is left on, `Resolved::drawn`.
    if (faint(u, s.shapes)) fgw = faint_on(fgw, bgw);
}

// Whether the cell draws a background that is not the default one: its
// rendition's `SHAPE_DEFAULT_BG` unset, or selected, or under a block
// cursor. Must match `Resolved::ground`.
static bool ground_set(constant CU& u, device const Style* styles,
                       device const uchar* overlay, uint2 cell, uint col, uint row) {
    Style s = styles[cell.y & 0xffffu];
    bool block = u.cursor_kind == 1u && col == u.cursor.x && row == u.cursor.y;
    return (s.shapes & SHAPE_DEFAULT_BG) == 0u || selected(u, overlay, cell.x, col, row) || block;
}

// `ground_pass`, for a frame with a kitty image below `INT32_MIN/2`:
// 0 draws every cell; 2 draws the default backgrounds and the others the
// transparent zero, the pass before the image, as the instance path's
// `bg_ground_fragment`; 1 draws the cells whose background is not the
// default one alone, a quad of no area for every other, the pass again
// over the image, blended, as its `bg_set_fragment`.
#define GROUND_EVERY 0u
#define GROUND_ONLY_SET 1u
#define GROUND_CLEAR_SET 2u
vertex FlatOut cell_bg_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const uint2* cells [[buffer(0)]],
                              device const Style* styles [[buffer(1)]],
                              constant CU& u [[buffer(2)]],
                              device const uchar* overlay [[buffer(4)]],
                              constant uint& ground_pass [[buffer(5)]]) {
    uint col = iid % u.grid.x, dr = iid / u.grid.x;
    int lrow = int(dr) - int(u.slide_rows);
    // On a sized line the column shows the character under half of it.
    bool sized = (sized_bits(u, overlay, col, lrow) & MASK_SIZED) != 0u;
    uint at = sized ? col / 2u : col;
    uint2 cell = cell_at(u, cells, at, dr);
    bool set = ground_pass != GROUND_EVERY && ground_set(u, styles, overlay, cell, at, uint(lrow));
    if (ground_pass == GROUND_ONLY_SET && !set) {
        FlatOut none;
        none.pos = float4(2.0, 2.0, 0.0, 1.0);
        none.color = float4(0.0);
        return none;
    }
    // A cell of a picture drawn in text keeps its background solid under
    // the dim, inside `colors()`. Must match `Renderer::convert_row`.
    uint fgw, bgw, curw;
    colors(u, styles, overlay, cell, at, lrow, fgw, bgw, curw);
    float2 c = float2(vid & 1u, vid >> 1u);
    float2 p = float2((float(col) + c.x) * float(u.cell.x),
                      (float(lrow) + c.y) * float(u.cell.y) + float(u.pan));
    FlatOut o;
    o.pos = clip(p, u);
    float4 bg = unpack(bgw);
    o.color = ground_pass == GROUND_CLEAR_SET && set ? float4(0.0) : float4(bg.rgb * bg.a, bg.a);
    return o;
}

fragment float4 flat_fragment(FlatOut in [[stage_in]]) { return in.color; }

// Which part of a glyph a pass draws: the rows from its cell's row top down
// (`PART_INSIDE`, before the shape pass), or the rows above it
// (`PART_ABOVE`, after): each row's glyphs and then its decorations, rows
// top to bottom, as the instance path draws, so a glyph reaching up from
// the row below lies over the decorations of the row it reaches. The
// overlay's records are cut the same way (`Instance::cut_at`).
#define PART_INSIDE 0u
#define PART_ABOVE 1u
vertex GOut cell_glyph_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const uint2* cells [[buffer(0)]],
                              device const Style* styles [[buffer(1)]],
                              constant CU& u [[buffer(2)]],
                              device const Entry* entries [[buffer(3)]],
                              device const uchar* overlay [[buffer(4)]],
                              constant uint& part [[buffer(5)]]) {
    GOut o;
    o.pos = float4(0.0); o.uv = float2(0.0); o.color = float4(0.0); o.kind = 0u; o.curve = 0u;
    uint col = iid % u.grid.x, dr = iid / u.grid.x;
    int lrow = int(dr) - int(u.slide_rows);
    // The overlay is a screen-cell mask; index it by the screen row, not
    // the draw row, and the leaving row above the window by the row after
    // the screen's (`sized_bits`). Bit 1 (`MASK_RECORD`) is a cell a record
    // draws; bit 2 is a picture's, never on the leaving row.
    if ((sized_bits(u, overlay, col, lrow) & MASK_RECORD) != 0u) return o;
    uint2 cell = cell_at(u, cells, col, dr);
    uint content = cell.x;
    if (content > 0x10FFFFu) return o;
    uint shapes = styles[cell.y & 0xffffu].shapes;
    if (hidden(u, shapes)) return o;
    // The contrast curve row for the side the swaps leave the glyph on:
    // the slot's foreground half, or its background half when swapped
    // (`Curves::row`).
    uint curve = glyph_curve(u, styles[cell.y & 0xffffu], overlay, content, col, lrow);
    // Two cells when the next cell is this one's wide tail: the grid says
    // the width there (`CellSpan::at`), not the cell's flags.
    uint two = (col + 1u < u.grid.x && cell_at(u, cells, col + 1u, dr).x == WIDE_TAIL) ? 1u : 0u;
    uint key = content | (((shapes >> FACE_SHIFT) & 3u) << KEY_STYLE_SHIFT)
                       | (((shapes >> (FONT_SHIFT + 4u)) & 1u) << KEY_FRAKTUR_SHIFT)
                       | (two << KEY_SPAN_SHIFT)
                       | (((shapes >> FONT_SHIFT) & 15u) << KEY_FONT_SHIFT);
    uint mask = u.map_slots - 1u;
    uint i = (key * 0x9E3779B1u) >> u.map_shift;
    Entry e; bool found = false;
    for (uint n = 0u; n < u.map_slots; n++) {
        e = entries[i];
        if (e.key == 0xFFFFFFFFu) break;
        if (e.key == key) { found = true; break; }
        i = (i + 1u) & mask;
    }
    if (!found || e.kind > 2u) return o;
    uint fgw, bgw, curw;
    colors(u, styles, overlay, cell, col, lrow, fgw, bgw, curw);
    float2 c = float2(vid & 1u, vid >> 1u);
    float rowy = float(lrow) * float(u.cell.y) + float(u.pan);
    float2 origin = float2(float(col * u.cell.x) + float(e.at.x),
                           rowy + float(u.baseline) + float(e.at.y));
    float2 wh = float2(e.rect.zw);
    // The rows of the glyph this part draws, cut at the row's top: whole
    // pixels on both sides, so the two parts draw every pixel once.
    float y0 = origin.y, y1 = origin.y + wh.y;
    float a = part == PART_ABOVE ? y0 : max(y0, rowy);
    float b = part == PART_ABOVE ? min(y1, rowy) : y1;
    if (b <= a) return o;
    float2 at = float2(origin.x, a);
    float2 size = float2(wh.x, b - a);
    o.pos = clip(at + c * size, u);
    float side = e.kind == 1u ? float(u.sides.y) : float(u.sides.x);
    o.uv = (float2(e.rect.xy) + float2(0.0, a - y0) + c * size) / side;
    o.color = unpack(fgw);
    o.kind = e.kind;
    o.curve = curve;
    return o;
}

// Shapes a cell: its underline, its strike, a second underline line for a
// double underline, the cursor when it is an underline or a bar, the
// overline (or a frame's top), a double overline's second line, a frame's
// bottom and its two sides, the stress mark, an end of an encircled run
// (a mask from the glyph map, as a glyph is), and then SEGMENTS quads for a curly, dotted or
// dashed underline (kitty's 4:3/4:4/4:5) -- each a run of thickness-wide
// solids across the cell, so the styles need no texel and composite exactly
// as the straight line does. A shape not drawn is a zero-area quad.
#define SHAPE_SLOTS 27u
#define FIXED_SLOTS 11u
// The cursor's slot, which a frame with no decoration on its table draws
// alone (`CURSOR_SHAPE_SLOT` in `metal.rs`, pinned beside `SHAPE_SLOTS`).
#define CURSOR_SLOT 3u
#define SEGMENTS (SHAPE_SLOTS - FIXED_SLOTS)
vertex GOut cell_shape_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                              device const uint2* cells [[buffer(0)]],
                              device const Style* styles [[buffer(1)]],
                              constant CU& u [[buffer(2)]],
                              device const Entry* entries [[buffer(3)]],
                              device const uchar* overlay [[buffer(4)]]) {
    GOut o;
    o.pos = float4(0.0); o.uv = float2(0.0); o.color = float4(0.0); o.kind = 2u; o.curve = 0u;
    uint which = iid % SHAPE_SLOTS, ci = iid / SHAPE_SLOTS;
    uint col = ci % u.grid.x, dr = ci / u.grid.x;
    int lrow = int(dr) - int(u.slide_rows);
    // A sized line's decorations and cursor are its records'.
    if ((sized_bits(u, overlay, col, lrow) & MASK_SIZED) != 0u) return o;
    // The top of this row's cell, shifted down by the smooth-scroll pan.
    float rowy = float(lrow) * float(u.cell.y) + float(u.pan);
    uint2 cell = cell_at(u, cells, col, dr);
    Style s = styles[cell.y & 0xffffu];
    if (which != CURSOR_SLOT && hidden(u, s.shapes)) return o;
    uint fgw, bgw, curw;
    colors(u, styles, overlay, cell, col, lrow, fgw, bgw, curw);
    float2 at; float2 wh; uint colw;
    float thick = float(max(u.thickness, 2u));
    if (which == 0u) {
        if ((s.shapes & 1u) == 0u) return o;
        at = float2(float(col * u.cell.x), rowy + float(u.underline));
        wh = float2(u.cell.x, u.thickness);
        colw = (s.shapes & 4u) != 0u ? fgw : rule(u, s, bgw);
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
        colw = (s.shapes & 4u) != 0u ? fgw : rule(u, s, bgw);
    } else if (which == CURSOR_SLOT) {
        if (col != u.cursor.x || lrow != int(u.cursor.y) || u.cursor_kind < 2u
            || u.cursor_kind > 3u) return o;
        if (u.cursor_kind == 2u) {
            at = float2(float(col * u.cell.x), rowy + float(u.cell.y) - thick);
            wh = float2(u.cell.x, thick);
        } else {
            // On the right of the cell where its character runs right to
            // left (`cursor_side`), `Renderer::cursor_px`'s bar.
            float x = float(col * u.cell.x)
                    + (u.cursor_side == 1u ? max(float(u.cell.x) - thick, 0.0) : 0.0);
            at = float2(x, rowy);
            wh = float2(thick, u.cell.y);
        }
        // The bar or underline cursor draws in the cursor colour, a colour
        // or the cell's page, else the text, at the fade's opacity
        // (`Argb::scaled`, `Renderer::shape_colour`).
        colw = scaled(paint(u.cursor_color, curw, curw, bgw | 0xff000000u), u.cursor_alpha);
    } else if (which < FIXED_SLOTS) {
        // The overline, the frame, the double overline's second line, the
        // stress mark and an encircled run, in the text's colour as the
        // strike is, with the CPU compositor's arithmetic: the overline,
        // the frame's top and the top between an encircled run's ends are
        // one line, and the frame's bottom and that run's bottom are one.
        float th = float(u.thickness);
        float th1 = max(th, 1.0);
        bool framed = (s.shapes & SHAPE_FRAMED) != 0u;
        bool encircled = (s.shapes & SHAPE_ENCIRCLED) != 0u;
        bool lc = encircled && carries(u, cells, styles, col, dr, true, SHAPE_ENCIRCLED);
        bool rc = encircled && carries(u, cells, styles, col, dr, false, SHAPE_ENCIRCLED);
        bool between = lc && rc;
        float x = float(col * u.cell.x);
        if (which == 4u) {
            if (!framed && !between && (s.shapes & SHAPE_OVERLINE) == 0u) return o;
            at = float2(x, rowy);
            wh = float2(u.cell.x, th);
        } else if (which == 5u) {
            if ((s.shapes & SHAPE_DOUBLE_OVERLINE) == 0u) return o;
            float low = float(u.cell.y) - th;
            at = float2(x, rowy + min(th1 + 1.0, max(low, 0.0)));
            wh = float2(u.cell.x, th);
        } else if (which == 9u) {
            // `SGR 64`'s stress mark: a filled square two thicknesses on a
            // side, centred across the character (both cells of a wide
            // one), a side's length below the cell's top, clamped inside.
            // A cluster's answer is the overlay's (`MASK_BARE`). The leaving
            // row above the window has no overlay, and its cells hold their
            // bases (`copy_for_shader`), so the table judges them, as the
            // glyph pass draws only the base there.
            bool bare = lrow >= 0 && (overlay[uint(lrow) * u.grid.x + col] & MASK_BARE) != 0u;
            if ((s.shapes & SHAPE_STRESS) == 0u || !takes_stress_mark(cell.x, bare)) return o;
            float d = 2.0 * th1;
            bool wide = col + 1u < u.grid.x && cell_at(u, cells, col + 1u, dr).x == WIDE_TAIL;
            float span = float(wide ? 2u * u.cell.x : u.cell.x);
            at = float2(x + floor(max(span - d, 0.0) * 0.5),
                        rowy + min(d, max(float(u.cell.y) - d, 0.0)));
            wh = float2(d, d);
        } else if (which == 10u) {
            // An end of an encircled run: its part of the outline, the
            // mask the glyph map holds under `RING_KEY`, placed at the cell
            // origin as a procedural glyph is, in the text's colour and
            // bent by the glyph's curve.
            if (!encircled || between) return o;
            uint key = RING_KEY + (lc ? 2u : (rc ? 1u : 3u));
            uint mask = u.map_slots - 1u;
            uint i = (key * 0x9E3779B1u) >> u.map_shift;
            Entry e; bool found = false;
            for (uint n = 0u; n < u.map_slots; n++) {
                e = entries[i];
                if (e.key == 0xFFFFFFFFu) break;
                if (e.key == key) { found = true; break; }
                i = (i + 1u) & mask;
            }
            if (!found || e.kind > 2u) return o;
            float2 c = float2(vid & 1u, vid >> 1u);
            float2 origin = float2(x + float(e.at.x), rowy + float(u.baseline) + float(e.at.y));
            float2 rwh = float2(e.rect.zw);
            o.pos = clip(origin + c * rwh, u);
            float side = e.kind == 1u ? float(u.sides.y) : float(u.sides.x);
            o.uv = (float2(e.rect.xy) + c * rwh) / side;
            o.color = unpack(fgw);
            o.kind = e.kind;
            o.curve = glyph_curve(u, s, overlay, cell.x, col, lrow);
            return o;
        } else if (which == 6u) {
            if (!framed && !between) return o;
            at = float2(x, rowy + float(u.cell.y) - th1);
            wh = float2(u.cell.x, th);
        } else {
            if (!framed) return o;
            // A side only where the run of framed cells starts or ends:
            // the row's edge, or a neighbour that does not carry it on.
            bool left = which == 7u;
            if (carries(u, cells, styles, col, dr, left, SHAPE_FRAMED)) return o;
            at = float2(left ? x : x + float(u.cell.x) - th1, rowy);
            wh = float2(th1, u.cell.y);
        }
        colw = fgw;
    } else {
        // Segment k of a styled underline: the same arithmetic as the CPU
        // compositor's, so the two arms agree pixel for pixel.
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
        at = float2(float(col * u.cell.x) + dx, rowy + y);
        wh = float2(th, th);
        colw = (s.shapes & 4u) != 0u ? fgw : rule(u, s, bgw);
    }
    float2 c = float2(vid & 1u, vid >> 1u);
    o.pos = clip(at + c * wh, u);
    o.color = unpack(colw);
    return o;
}

// The hollow cursor, `cursor_kind` 4 (`CursorKind::Hollow`): four solids
// round the cursor's cell, instance 0 its top and 1 its bottom across the
// cell, 2 and 3 its sides between them, each the bar's line thick and at
// most half the cell's narrower side, in the bar's colour -- the four
// `Renderer::hollow` gives the CPU and the instance path. No colour swaps
// (`colors` swaps only for kind 1). A sized line's cursor is its records'.
vertex GOut cell_ring_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                             device const uint2* cells [[buffer(0)]],
                             device const Style* styles [[buffer(1)]],
                             constant CU& u [[buffer(2)]],
                             device const Entry* entries [[buffer(3)]],
                             device const uchar* overlay [[buffer(4)]]) {
    GOut o;
    o.pos = float4(0.0); o.uv = float2(0.0); o.color = float4(0.0); o.kind = 2u; o.curve = 0u;
    if (u.cursor_kind != 4u || u.cursor.x >= u.grid.x) return o;
    uint col = u.cursor.x;
    int lrow = int(u.cursor.y);
    if ((sized_bits(u, overlay, col, lrow) & MASK_SIZED) != 0u) return o;
    uint dr = uint(lrow + int(u.slide_rows));
    float rowy = float(lrow) * float(u.cell.y) + float(u.pan);
    uint2 cell = cell_at(u, cells, col, dr);
    uint fgw, bgw, curw;
    colors(u, styles, overlay, cell, col, lrow, fgw, bgw, curw);
    uint t = min(max(u.thickness, 2u), min(u.cell.x, u.cell.y) / 2u);
    float x = float(col * u.cell.x), w = float(u.cell.x), h = float(u.cell.y), ft = float(t);
    float side = float(u.cell.y - min(2u * t, u.cell.y));
    float2 at; float2 wh;
    if (iid == 0u) {
        at = float2(x, rowy); wh = float2(w, ft);
    } else if (iid == 1u) {
        at = float2(x, rowy + h - ft); wh = float2(w, ft);
    } else if (iid == 2u) {
        at = float2(x, rowy + ft); wh = float2(ft, side);
    } else {
        at = float2(x + w - ft, rowy + ft); wh = float2(ft, side);
    }
    uint colw = scaled(paint(u.cursor_color, curw, curw, bgw | 0xff000000u), u.cursor_alpha);
    float2 c = float2(vid & 1u, vid >> 1u);
    o.pos = clip(at + c * wh, u);
    o.color = unpack(colw);
    return o;
}
