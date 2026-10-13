// ---- baud's record path, reproduced for measurement (read from baud-present-macos/src/shaders.metal:4-58)
struct U { float2 viewport; float2 cell; float2 grid; float2 mask; float2 color; float2 pad; };
struct Inst { short2 xy; ushort2 wh; ushort2 uv; uint color; uint kind; };
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
#if EFFECTS
    if (i.kind == 4u) o.uv = c * 2.0 - 1.0;   // ember: local disc coordinates
#endif
    uint col = i.color;
    o.color = float4(float((col >> 16) & 0xffu), float((col >> 8) & 0xffu),
                     float(col & 0xffu), float(col >> 24)) / 255.0;
    o.kind = i.kind;
    return o;
}

// ---- the flame (this harness's own design; math from Planck/CIE, value noise + fBM per IQ articles)
struct FireU {
    float2 c0; float2 c1; float2 c2; float2 c3;  // emitter quad, window px
    float4 hole;                                 // x0 y0 x1 y1 of the landed cursor
    float2 dir; float2 tail;                     // unit tail->head, tail centre px
    float len; float cell; float age; float life;
    float gain; uint seed; float pad0; float pad1;
};

static inline uint pcg(uint v) {                 // Jarzynski & Olano 2020, "pcg" (O'Neill 2014)
    uint s = v * 747796405u + 2891336453u;
    uint w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}
static inline float u01(uint h) { return as_type<float>((h >> 9) | 0x3f800000u) - 1.0f; } // exact, no int->float convert
static inline float lattice(int2 i, uint seed) { return u01(pcg(uint(i.x) + pcg(uint(i.y) + seed))); }
static inline float vnoise(float2 p, uint seed) {
    float2 f = floor(p); int2 i = int2(f); float2 x = p - f;
    float2 w = x * x * x * (x * (x * 6.0f - 15.0f) + 10.0f);          // quintic
    float a = lattice(i, seed), b = lattice(i + int2(1, 0), seed);
    float c = lattice(i + int2(0, 1), seed), d = lattice(i + int2(1, 1), seed);
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}
static inline float fbm(float2 p, uint seed) {
    float t = 0.0f, a = 0.5f;
    const float2x2 R = float2x2(float2(0.8f, 0.6f), float2(-0.6f, 0.8f));
    for (int o = 0; o < OCT; o++) {
        t += a * vnoise(p, seed + uint(o) * 0x9E3779B9u);
        p = (R * p) * 2.02f; a *= 0.5f;
    }
    return t;
}
static inline float seg(float2 p, float2 a, float2 b) {
    float2 pa = p - a, ba = b - a;
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6f), 0.0f, 1.0f);
    return length(pa - ba * h);
}
static inline bool xing(float2 p, float2 a, float2 b) {
    float dy = b.y - a.y; dy = abs(dy) < 1e-6f ? 1e-6f : dy;           // fast math: never divide by 0
    return ((a.y > p.y) != (b.y > p.y)) && (p.x < (b.x - a.x) * (p.y - a.y) / dy + a.x);
}
static inline float sd_quad(float2 p, constant FireU& f) {
    float d = min(min(seg(p, f.c0, f.c1), seg(p, f.c1, f.c2)), min(seg(p, f.c2, f.c3), seg(p, f.c3, f.c0)));
    bool in = xing(p, f.c0, f.c1) ^ xing(p, f.c1, f.c2) ^ xing(p, f.c2, f.c3) ^ xing(p, f.c3, f.c0);
    return in ? -d : d;                                                 // even-odd: concave/bowtie safe
}

#define RISE 2.2f       // cells/s the pattern climbs
#define DRIFT 0.6f      // cells/s it trails behind the motion
#define FLAME_H 1.6f    // cells a tongue can climb above the emitter

static inline float4 fire(float2 p, constant FireU& f) {
    float c = f.cell;
    if (p.x >= f.hole.x && p.x < f.hole.z && p.y >= f.hole.y && p.y < f.hole.w) return float4(0.0f);
    float k = f.age / f.life;
    if (k >= 1.0f) return float4(0.0f);
    float d0 = sd_quad(p, f) / c;
    if (d0 > FLAME_H + 0.4f || d0 < -(FLAME_H + 0.4f) * 8.0f) {
        // cheap reject: a tongue lifts a sample at most FLAME_H cells
        if (d0 > FLAME_H + 0.4f) return float4(0.0f);
    }
    float2 q = p / c;
    float2 s = float2(q.x * 1.6f, q.y * 1.1f + f.age * RISE) - f.dir * (f.age * DRIFT);
#if WARP
    s.x += 0.8f * vnoise(s * 0.5f + 7.3f, f.seed ^ 0xA5A5A5A5u);
#endif
    float n = fbm(s, f.seed);
    float lift = FLAME_H * c * n * n;
    float d = sd_quad(p + float2(0.0f, lift), f) / c;
    float body = 1.0f - smoothstep(-0.15f, 0.35f, d);
    float u = clamp(dot(p - f.tail, f.dir) / max(f.len, 1.0f), 0.0f, 1.0f);
    float burn = smoothstep(k - 0.15f, k + 0.35f, u) * (1.0f - k * k);
    float heat = body * burn * (0.55f + 0.75f * n);
    if (heat <= 0.02f) return float4(0.0f);
    float h = saturate(heat);
    float x = h * 15.0f; int i = min(int(x), 14); float fr = x - float(i);
    float3 col = mix(BB[i], BB[i + 1], fr);
    float a = smoothstep(0.05f, 0.6f, h) * f.gain;
    float glow = smoothstep(0.0f, 0.9f, h) * f.gain;
    return float4(col * glow, a * 0.75f);                                 // premultiplied, partly additive
}

static inline float4 ember(GOut in) {
    float r = length(in.uv);
    float a = 1.0f - smoothstep(0.15f, 1.0f, r);
    a *= a;
    return float4(in.color.rgb * a, in.color.a * a * 0.5f);
}

fragment float4 glyph_fragment(GOut in [[stage_in]],
                               texture2d<float> mask [[texture(0)]],
                               texture2d<float> color [[texture(1)]],
                               sampler s [[sampler(0)]]
#if EFFECTS
                               , constant FireU& f [[buffer(0)]]
#endif
                               ) {
    if (in.kind == 0u) {
        float a = in.color.a * mask.sample(s, in.uv).r;
        return float4(in.color.rgb * a, a);
    }
    if (in.kind == 1u) {
        float4 c = color.sample(s, in.uv);
        return float4(c.rgb * c.a, c.a);
    }
#if EFFECTS
    if (in.kind == 3u) return fire(in.pos.xy, f);
    if (in.kind == 4u) return ember(in);
#endif
    return float4(in.color.rgb * in.color.a, in.color.a);
}

// a separate pipeline's fragment: the same flame, nothing else
fragment float4 fire_fragment(GOut in [[stage_in]], constant FireU& f [[buffer(0)]]) {
    return fire(in.pos.xy, f);
}
