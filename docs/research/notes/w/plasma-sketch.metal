// ---- The plasma, `baud-render/src/plasma.rs` (proposed). Sketch against the
// uncommitted effect framework (EffectU, fx_distance, fx_in_hole, fx_hash,
// fx_unit, KIND_PARTICLE). Its own numbers, `plasma::params(&Vfx)`, computed
// per frame in Rust:
//   k[0] half   -- the cell's half-extent across the live axis, cells
//   k[1] ax, k[2] ay -- the live axis: unit vector hole-centre minus the
//                       corners' mean, or the ignition's dir when < 0.25 cell
//   k[3] tube   -- opacity inside the quad (0.08)
//   k[4] sheath -- sheath amplitude at the quad's edge (0.20)
//   k[5] decay  -- sheath e-folds a cell at ignition (5.0)
//   k[6] fil    -- filament gain, 0.95 * min(1, (80 / len_cells)^2)
//   k[7] knot   -- knot flow, Hz at a fixed pixel (2.5)

// sin(2 pi c), with the argument folded into [-pi, pi) where fast-math sinpi
// is good to 2^-13: fract is floor-based (Rust: c - c.floor()).
static inline float px_wave(float c) {
    return sinpi(2.0 * fract(c + 0.5) - 1.0);
}

// The braid's shape: cycles a cell along, cycles a second of flow, the same
// for the detuned harmonic, its share, the core's width in cells, the gain.
constant float3 PX_K   = float3(0.23, 0.31, 0.17);
constant float3 PX_F   = float3(1.10, 1.35, 0.80);
constant float3 PX_K2  = float3(0.71, 0.93, 0.53);
constant float3 PX_F2  = float3(-0.85, -1.05, 1.20);
constant float3 PX_M   = float3(0.22, 0.20, 0.25);
constant float3 PX_W   = float3(0.040, 0.034, 0.050);
constant float3 PX_G   = float3(1.00, 0.85, 0.65);
#define PX_AMP   0.62   // of `half`: the braid's reach across the tube
#define PX_PINCH 2.0    // cells behind the head over which it converges
#define PX_KNOT  0.45   // knots, cycles a cell along
#define PX_MINPX 1.1    // a core is never under this many pixels wide

static inline float4 plasma_tube(float2 p, float4 color, constant EffectU& f) {
    if (fx_in_hole(p, f)) return float4(0.0);
    float k = f.age / f.life;
    if (k >= 1.0) return float4(0.0);
    float c = f.unit;
    float d0 = fx_distance(p, f) / c;
    if (d0 > f.reach) return float4(0.0);
    float env = (1.0 - k) * (1.0 - k);
    // The tube: faint inside, a sheath that contracts as the plasma cools.
    float inside = 1.0 - smoothstep(-0.03, 0.03, d0);
    float tube = f.k[3] * inside
               + f.k[4] * exp(-f.k[5] * (1.0 + 1.5 * k) * max(d0, 0.0));
    float fil = 0.0;
    if (d0 < 0.15) {
        float2 a = float2(f.k[1], f.k[2]);
        float2 r = p - 0.5 * (f.hole.xy + f.hole.zw);
        float s = -dot(r, a) / c;                 // cells behind the head
        float w = (a.x * r.y - a.y * r.x) / c;    // cells across (perp = (-ay, ax))
        float pinch = smoothstep(0.0, PX_PINCH, s);
        float amp = f.k[0] * PX_AMP * pinch;
        float kn = fx_unit(fx_hash(f.seed + 8u));
        for (uint i = 0u; i < 3u; i++) {
            float ph = fx_unit(fx_hash(f.seed + i));
            float ph2 = fx_unit(fx_hash(f.seed + 16u + i));
            float cyc = PX_K[i] * s + ph + PX_F[i] * f.age;
            float cyc2 = PX_K2[i] * s + ph2 + PX_F2[i] * f.age;
            float along = amp * ((1.0 - PX_M[i]) * px_wave(cyc) + PX_M[i] * px_wave(cyc2));
            float wid = max(PX_W[i] * (1.0 - 0.5 * k), PX_MINPX / c);
            float e = (w - along) / wid;
            float knot = 0.72 + 0.28 * px_wave(PX_KNOT * s + kn + float(i) / 3.0 + f.k[7] * f.age);
            fil += PX_G[i] * exp(-e * e) * knot;
        }
        fil = saturate(fil) * f.k[6] * (1.0 - smoothstep(-0.02, 0.15, d0));
    }
    float alpha = color.a * saturate(env * (tube + (1.0 - tube) * fil));
    return float4(color.rgb * alpha, alpha);
}

// An ion: the same soft disc as an ember -- one spelling would be better:
// factor `fire_ember` into `fx_particle` and call it from both.
fragment float4 plasma_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    if (in.kind == KIND_PARTICLE) return fx_particle(in);
    return plasma_tube(in.pos.xy - f.origin, in.color, f);
}
