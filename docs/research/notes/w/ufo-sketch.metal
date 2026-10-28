// UFO trail -- MSL sketch, one member of the effect family (f202aea).
// Reads the existing EffectU unchanged: prefix c[4] hole origin tail dir len
// unit age life seed reach, then k[8] = ufo::PARAMS:
//   k[0] hover (units)   k[1] size (scale)   k[2] beam alpha   k[3] wake alpha
//   k[4] light turns/s   k[5] arc (units)    k[6] tilt (rad)   k[7] depart (s)
// Uses fx_hash, fx_unit, fx_in_hole from shaders.metal. No texture reads.
// Every trig argument is range-reduced into the fast-math domain
// (sinpi/cospi on [-1,1], cos/sin on |theta| <= 0.37): MSL Table 8.2, 2^-13 abs.

#define UFO_HULL_W   0.55
#define UFO_HULL_H   0.13
#define UFO_ROUND    0.02
#define UFO_VES_R    1.1917    // (W'^2 + H'^2) / (2 H'), W' = 0.53, H' = 0.11
#define UFO_VES_D    1.0817    // UFO_VES_R - H'
#define UFO_VES_B    0.53      // sqrt(r^2 - d^2) = W'
#define UFO_DOME_R   0.22
#define UFO_DOME_Y  -0.04
#define UFO_SMIN_K   0.03
#define UFO_LIGHT_R  0.045
#define UFO_LIGHT_X  0.40
#define UFO_LIGHT_Y  0.03
#define UFO_SAUCER_R 0.62
#define UFO_BOB      0.035
#define UFO_BOB_HZ   2.2
#define UFO_TILT_GAP 3.0
#define UFO_APPEAR   0.08
#define UFO_LEAVE    0.18
#define UFO_LEAVE_ACC 92.0
#define UFO_OPEN_AT  0.15
#define UFO_OPEN     0.10
#define UFO_CLOSE_AT 0.46
#define UFO_CLOSE    0.08
#define UFO_HW0      0.14
#define UFO_HW1      0.22
#define UFO_PAD      0.08
#define UFO_RING_P   0.22
#define UFO_RING_V   5.0
#define UFO_RING_D   0.22
#define UFO_WAKE_T   0.06
#define UFO_WAKE_RA  0.055
#define UFO_WAKE_RB  0.012
#define UFO_SIDE_ROOM 1.35

struct UfoPose {
    float2 c; float cs; float sn; float s; float vis;
    float cr; float sr; float side; float gap; float gx; float beam; float ext;
};

static inline float ufo_cov(float d) { return clamp(0.5 - d, 0.0, 1.0); }

// The flight's omega: a critically damped spring reaching within ~9% in T
// (neovide animation_utils.rs: omega = 4 / T), T from the path's length.
static inline float ufo_omega(constant EffectU& f) {
    return 4.0 / clamp(0.12 + 0.006 * f.len / f.unit, 0.16, 0.32);
}

// **The saucer at age `a`**: identical for every pixel of a frame; the Rust
// reference and `ufo::pose` (which builds the region) spell it the same way.
static inline UfoPose ufo_pose(constant EffectU& f, float a) {
    float c = f.unit;
    float2 cq = (f.c[0] + f.c[1] + f.c[2] + f.c[3]) * 0.25;   // the quad's centroid
    float w = ufo_omega(f);
    float sig = 1.0 - (1.0 + w * a) * exp(-w * a);            // spring from rest
    float2 on = f.tail + (cq - f.tail) * sig;
    float2 hc = (f.hole.xy + f.hole.zw) * 0.5;
    float2 g = hc - on;
    float gap = length(g);
    float side = f.hole.y >= UFO_SIDE_ROOM * c ? 1.0 : -1.0;  // 1: above, beam down
    float2 pth = cq - f.tail;
    float pl = length(pth);
    float2 n = pl > 1e-3 ? float2(-pth.y, pth.x) / pl : float2(0.0, -side);
    if (n.y * side > 0.0) n = -n;                                  // bulge away from the cursor
    float arc = min(f.k[5] * c, 0.12 * f.len) * 4.0 * sig * (1.0 - sig);
    float hov = 1.0 - smoothstep(0.1 * c, 0.6 * c, gap);
    float h1 = fx_unit(fx_hash(f.seed ^ 0x5bd1e995u));
    float h2 = fx_unit(fx_hash(f.seed));
    float bob = UFO_BOB * c * sinpi(2.0 * fract(UFO_BOB_HZ * a + h1) - 1.0) * hov;
    float tl = max(a - f.k[7], 0.0);
    float kk = clamp(tl / UFO_LEAVE, 0.0, 1.0);
    UfoPose P;
    P.c = on + n * arc + float2(0.0, -side * f.k[0] * c + bob - side * 0.5 * UFO_LEAVE_ACC * c * tl * tl);
    float th = f.k[6] * clamp(g.x / (UFO_TILT_GAP * c), -1.0, 1.0)
             + 0.05 * sinpi(2.0 * fract(1.3 * a + h2) - 1.0) * hov;
    P.cs = cos(th); P.sn = sin(th);
    float grow = smoothstep(0.0, UFO_APPEAR, a);
    P.vis = grow * (1.0 - smoothstep(0.0, 1.0, kk));
    P.s = f.k[1] * mix(0.5, 1.0, grow) * (1.0 - 0.6 * kk * kk);
    float rho = 2.0 * fract(f.k[4] * a + h2) - 1.0;
    P.cr = cospi(rho); P.sr = sinpi(rho);
    P.side = side; P.gap = gap; P.gx = g.x;
    P.ext = smoothstep(UFO_OPEN_AT, UFO_OPEN_AT + UFO_OPEN, a)
          * (1.0 - smoothstep(UFO_CLOSE_AT, UFO_CLOSE_AT + UFO_CLOSE, a));
    P.beam = P.ext * (1.0 - smoothstep(0.15 * c, 0.6 * c, gap));
    return P;
}

// iq's vesica (munrocket's MIT WGSL spelling), axes swapped: tips on x.
static inline float ufo_vesica(float2 q) {
    float2 p = abs(q.yx);
    if ((p.y - UFO_VES_B) * UFO_VES_D > p.x * UFO_VES_B) return length(p - float2(0.0, UFO_VES_B));
    return length(p + float2(UFO_VES_D, 0.0)) - UFO_VES_R;
}

// iq's cut disk (y up), h = 0: the upper half-disc.
static inline float ufo_dome(float2 p) {
    const float r = UFO_DOME_R;              // w = r when h = 0
    p.x = abs(p.x);
    float s = max(-r * p.x * p.x + r * r * (r - 2.0 * p.y), -r * p.y);
    return s < 0.0 ? length(p) - r : (p.x < r ? -p.y : length(p - float2(r, 0.0)));
}

// iq's quadratic smooth minimum.
static inline float ufo_smin(float a, float b, float k) {
    k *= 4.0;
    float h = max(k - abs(a - b), 0.0) / k;
    return min(a, b) - h * h * k * 0.25;
}

constant float2 UFO_LIGHTS[5] = {           // (cospi(2i/5), sinpi(2i/5)) as literals
    float2(1.0, 0.0), float2(0.309017, 0.951057), float2(-0.809017, 0.587785),
    float2(-0.809017, -0.587785), float2(0.309017, -0.951057)
};

static inline float ufo_body(float2 p, UfoPose P, float c) {
    float u = P.s * c;
    float2 d = p - P.c;
    if (dot(d, d) > (UFO_SAUCER_R * u + 1.0) * (UFO_SAUCER_R * u + 1.0)) return 0.0;
    float2 q = float2(P.cs * d.x + P.sn * d.y, -P.sn * d.x + P.cs * d.y) / u;
    float dh = ufo_vesica(q) - UFO_ROUND;
    float dd = ufo_dome(float2(q.x, UFO_DOME_Y - q.y));
    float sil = ufo_smin(dh, dd, UFO_SMIN_K);
    float light = 0.0;
    if (abs(q.y - UFO_LIGHT_Y) < 2.0 * UFO_LIGHT_R) {
        for (uint i = 0u; i < 5u; i++) {
            float cphi = P.cr * UFO_LIGHTS[i].x - P.sr * UFO_LIGHTS[i].y;
            float sphi = P.sr * UFO_LIGHTS[i].x + P.cr * UFO_LIGHTS[i].y;
            float dl = length(q - float2(UFO_LIGHT_X * sphi, UFO_LIGHT_Y)) - UFO_LIGHT_R;
            light = max(light, smoothstep(0.0, 0.35, cphi) * ufo_cov(dl * u));
        }
    }
    return (0.50 * ufo_cov(sil * u) + ufo_cov(dh * u) * (0.30 + 0.20 * light)) * P.vis;
}

static inline float ufo_beam(float2 p, UfoPose P, constant EffectU& f) {
    if (P.beam <= 0.0) return 0.0;
    float c = f.unit;
    float2 top = P.c + float2(0.0, P.side * UFO_HULL_H * c * P.s * 0.9);
    float2 end = float2((f.hole.x + f.hole.z) * 0.5, P.side > 0.0 ? f.hole.y : f.hole.w);
    float2 ax = end - top;
    float ln = length(ax);
    if (ln < 1.0) return 0.0;
    float2 nn = ax / ln;
    float2 d = p - top;
    float along = dot(d, nn);
    float across = abs(d.x * nn.y - d.y * nn.x);
    float ff = clamp(along / ln, 0.0, 1.0);
    float hw = mix(UFO_HW0 * c * P.s, max(0.5 * (f.hole.z - f.hole.x) + UFO_PAD * c, UFO_HW1 * c), ff);
    float edge = clamp(hw - across + 0.5, 0.0, 1.0) * clamp(ln * P.ext - along + 0.5, 0.0, 1.0)
               * clamp(along + 0.5, 0.0, 1.0);
    if (edge <= 0.0) return 0.0;
    float core = 1.0 - smoothstep(0.0, 0.6, across / max(hw, 1.0));
    float fr = fract(along / (UFO_RING_P * c) - fract(UFO_RING_V * f.age));
    float ring = 1.0 - UFO_RING_D * 4.0 * fr * (1.0 - fr);
    return f.k[2] * P.vis * P.beam * edge * (0.55 + 0.45 * core) * (0.6 + 0.4 * ff) * ring;
}

static inline float ufo_wake(float2 p, UfoPose P, constant EffectU& f) {
    float c = f.unit;
    if (f.age <= 0.0) return 0.0;
    UfoPose Q = ufo_pose(f, max(f.age - UFO_WAKE_T, 0.0));
    float2 v = P.c - Q.c;
    float l = length(v);
    if (l < 0.1 * c) return 0.0;
    float2 a = P.c - v / l * (0.45 * UFO_HULL_W * c * P.s);
    float2 ba = Q.c - a;
    float2 pa = p - a;
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    float dd = length(pa - ba * h) - mix(UFO_WAKE_RA * c, UFO_WAKE_RB * c, h);
    return f.k[3] * P.vis * ufo_cov(dd) * (1.0 - h) * smoothstep(0.1 * c, 0.6 * c, l);
}

// **A band**: the three parts composited here, saucer over beam over wake --
// a band is one row slice no other band shares, so nothing is drawn twice.
static inline float4 ufo_band(float2 p, float4 color, constant EffectU& f) {
    if (fx_in_hole(p, f)) return float4(0.0);
    if (f.age >= f.life) return float4(0.0);
    UfoPose P = ufo_pose(f, f.age);
    float a = ufo_wake(p, P, f);
    float b = ufo_beam(p, P, f);  a = b + a * (1.0 - b);
    float s = ufo_body(p, P, f.unit); a = s + a * (1.0 - s);
    a *= color.a;
    return float4(color.rgb * a, a);
}

// **An exhaust ring** (neovide's torpedo stroke): the record's alpha carries
// the ring's glow; `glyph_vertex` hands -1..1 over the record.
static inline float4 ufo_puff(GOut in) {
    float a = in.color.a * (1.0 - smoothstep(0.0, 0.22, abs(length(in.uv) - 0.72)));
    return float4(in.color.rgb * a, a);
}

fragment float4 ufo_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    if (in.kind == KIND_PARTICLE) return ufo_puff(in);
    return ufo_band(in.pos.xy - f.origin, in.color, f);
}
