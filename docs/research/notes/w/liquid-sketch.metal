// ---- The liquid trail, design (a): a glass gel in the cursor's colour.
// Every time-dependent number is Rust's (liquid.rs), in LiquidU; the fragment
// uses only + - * / sqrt rsqrt min max clamp abs select -- no exp, sin, pow,
// hash or noise -- so a Rust port matches it to fast-math rounding.
struct LiquidU {
    // shared prefix (FireU's shape): the quad, the hole, where the frame sits,
    // the ignition's path, and the clocks. The fragment reads hole, origin, reach.
    float2 c[4]; float4 hole; float2 origin; float2 tail; float2 dir;
    float len; float unit; float age; float life; uint seed; float reach;
    // liquid, all filled by Rust for this frame
    float2 b; float2 h;              // spine: back B (quad's trailing edge on the axis), head H (hole centre)
    float radius; float neck;        // tube radius R; neck n (0 none, >1 pinched)
    float inv_len2; float lip;       // 1/|H-B|^2 (0 = no tube); 4 n R / |H-B|
    float2 half_box; float round;    // head drop: hole half-size + gap + wobble; corner radius
    float k;                         // smooth-union width, px (iq's normalised k: max bulge)
    float4 bead[3];                  // satellites: xy centre, z radius (<= 0: none), w unused
    float2 light;                    // unit vector toward the light, y down
    float envelope;                  // 0..1 fade in / out
    float inv_bezel;                 // 1 / bezel width, px
    float body; float rim; float spec; float back;
    float pad0; float pad1; float pad2; float pad3;   // 60 floats, 240 bytes
};

// iq's quadratic smooth-min (normalised k, `[[source: iq-smin]]`), carrying the
// exact gradient: for a < b, df/da = 1 - h/2 and df/db = h/2 -- continuous at
// a == b and at |a-b| == 4k, so a one-ulp branch flip moves nothing.
static inline float3 liquid_smin(float3 a, float3 b, float k) {
    float h = 1.0 - min(abs(a.x - b.x) / (4.0 * k), 1.0);
    float s = h * h * k;
    float w = 0.5 * h;
    return a.x < b.x ? float3(a.x - s, mix(a.yz, b.yz, w))
                     : float3(b.x - s, mix(b.yz, a.yz, w));
}

static inline float2 liquid_unit(float2 v, float l) {
    return l > 1e-3 ? v / l : float2(0.0);
}

// The field at p: .x the (approximate) signed distance, .yz its gradient.
static inline float3 liquid_field(float2 p, constant LiquidU& L) {
    // Head drop: a rounded box about the hole (iq's sdRoundedBox, gradient by
    // hand). `round` >= the gap on every axis, so the box's inner medial axis --
    // the one place its gradient jumps -- lies inside the hole, which is bare.
    float2 ph = p - L.h;
    float2 sg = select(float2(-1.0), float2(1.0), ph >= 0.0);   // NOT sign(): sign(0) is 0
    float2 w = abs(ph) - (L.half_box - L.round);
    float g = max(w.x, w.y);
    float3 f;
    if (g > 0.0) {
        float2 wp = max(w, 0.0);
        float lw = length(wp);
        f = float3(lw - L.round, sg * wp / lw);
    } else {
        f = float3(g - L.round, w.x > w.y ? float2(sg.x, 0.0) : float2(0.0, sg.y));
    }
    // Necked tube along the spine: radius R (1 - 4 n u (1-u)), negative in the
    // middle once pinched; the distance divided by sqrt(1 + r'(s)^2) so the
    // bezel keeps its width where the radius changes fast.
    if (L.inv_len2 > 0.0) {
        float2 e = L.h - L.b;
        float u = clamp(dot(p - L.b, e) * L.inv_len2, 0.0, 1.0);
        float2 v = p - (L.b + e * u);
        float l = length(v);
        float q = 1.0 - 4.0 * L.neck * u * (1.0 - u);
        float sl = L.lip * (2.0 * u - 1.0);
        float d = (l - L.radius * q) * rsqrt(1.0 + sl * sl);
        f = liquid_smin(f, float3(d, liquid_unit(v, l)), L.k);
    }
    for (uint i = 0u; i < 3u; i++) {
        float4 bd = L.bead[i];
        if (bd.z > 0.0) {
            float2 v = p - bd.xy;
            float l = length(v);
            f = liquid_smin(f, float3(l - bd.z, liquid_unit(v, l)), L.k);
        }
    }
    return f;
}

// The glass at p, as an opacity of the cursor's colour.
static inline float liquid_alpha(float2 p, constant LiquidU& L) {
    if (p.x >= L.hole.x && p.x < L.hole.z && p.y >= L.hole.y && p.y < L.hole.w) return 0.0;
    // Early reject, equal to the band builder's reach: the distance to the
    // spine segment and to the hole rectangle, the liquid's two cores.
    float2 e = L.h - L.b;
    float u0 = clamp(dot(p - L.b, e) * L.inv_len2, 0.0, 1.0);
    float ds = L.inv_len2 > 0.0 ? length(p - (L.b + e * u0)) : INFINITY;
    float2 o = max(max(L.hole.xy - p, p - L.hole.zw), 0.0);
    if (min(ds, length(o)) > L.reach) return 0.0;

    float3 f = liquid_field(p, L);
    float cov = clamp(0.5 - f.x, 0.0, 1.0);
    if (cov <= 0.0) return 0.0;
    // Depth into the bezel, and the squircle bevel s^4 + z^4 = 1 (webglass's
    // profile, `Surface.squircle`): its normal's z is z^3 / |(s^3, z^3)|.
    float t = clamp(-f.x * L.inv_bezel, 0.0, 1.0);
    float s = 1.0 - t;
    float s2 = s * s;
    float z = sqrt(sqrt(max(1.0 - s2 * s2, 0.0)));
    float z3 = z * z * z;
    float s3 = s2 * s;
    float nz = z3 * rsqrt(max(s3 * s3 + z3 * z3, 1e-12));
    float fr = 1.0 - nz;
    float rim = 0.04 + 0.96 * fr * fr;          // a stylised Fresnel: Schlick's ^5 is sub-pixel at cell sizes
    float gl = dot(f.yz, f.yz);
    float2 n = gl > 1e-12 ? f.yz * rsqrt(gl) : float2(0.0);
    float bt = clamp((t - 0.04) * 2.5, 0.0, 1.0);
    float band = 4.0 * bt * (1.0 - bt);         // the highlight sits just inside the rim
    float nl = dot(n, L.light);
    float sp = max(nl, 0.0);  sp = sp * sp * sp * band;
    float bk = max(-nl, 0.0); bk = bk * bk * bk * band;
    return L.envelope * cov * saturate(L.body + L.rim * rim + L.spec * sp + L.back * bk);
}

fragment float4 liquid_fragment(GOut in [[stage_in]], constant LiquidU& L [[buffer(0)]]) {
    float a = in.color.a * liquid_alpha(in.pos.xy - L.origin, L);
    return float4(in.color.rgb * a, a);
}

// ---- Design (b)'s pass: the same glass over what is under it, bent. Drawn
// with blending off (Blend::Replace), because the lens must replace the
// pixels under it, not add to them; `under` is a copy of the target taken
// before the trail, `under_at` is where that copy's (0,0) sits in the target.
struct LensU { float2 under_at; float lens; float lens_norm; };

static inline float4 lens_read(texture2d<float, access::read> t, int2 q) {
    int2 hi = int2(t.get_width(), t.get_height()) - 1;
    return t.read(uint2(clamp(q, int2(0), hi)));
}

// Bilinear by hand from four read()s: a sampler's filter weights are the
// GPU's own fixed-point arithmetic, which a Rust reference cannot port.
static inline float4 lens_bilinear(texture2d<float, access::read> t, float2 q) {
    float2 f0 = floor(q - 0.5);
    float2 w = (q - 0.5) - f0;
    int2 i = int2(f0);
    float4 a = lens_read(t, i), b = lens_read(t, i + int2(1, 0));
    float4 c = lens_read(t, i + int2(0, 1)), d = lens_read(t, i + int2(1, 1));
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}

fragment float4 liquid_lens_fragment(GOut in [[stage_in]],
                                     constant LiquidU& L [[buffer(0)]],
                                     constant LensU& K [[buffer(1)]],
                                     texture2d<float, access::read> under [[texture(0)]]) {
    float2 q = in.pos.xy - K.under_at;          // this pixel in the copy
    float4 dst = lens_read(under, int2(floor(q)));
    float2 p = in.pos.xy - L.origin;
    if (p.x >= L.hole.x && p.x < L.hole.z && p.y >= L.hole.y && p.y < L.hole.w) return dst;
    float3 f = liquid_field(p, L);              // (early reject as above, returning dst)
    float cov = clamp(0.5 - f.x, 0.0, 1.0);
    if (cov <= 0.0) return dst;
    // Snell through the squircle bevel at IOR 1.5: the lateral shift at depth
    // t is (eta nz - sqrt(1 - eta^2 (1 - nz^2))) nr z, normalised by its
    // maximum over t (Rust, once) -- webglass's `_radialDisplacements`,
    // closed form. It is zero on the flat interior (nr = 0) and bends only
    // the bezel. The sample moves inward, which magnifies.
    // ... t, s, z, nz as in liquid_alpha; nr = s^3 * rsqrt(s^6 + z^6)
    float eta = 1.0 / 1.5;
    float k2 = 1.0 - eta * eta * (1.0 - nz * nz);
    float lat = (eta * nz - sqrt(max(k2, 0.0))) * nr * z * K.lens_norm;   // <= 0 (inward)
    float2 disp = n * (lat * K.lens * L.envelope);   // lens strength materialises with the envelope
    float4 bent = lens_bilinear(under, q + disp);
    float4 base = mix(dst, bent, cov);
    float4 glass = in.color * liquid_alpha(p, L);    // premultiplied cursor colour
    return glass + base * (1.0 - glass.a);           // written as is: blending is off
}
