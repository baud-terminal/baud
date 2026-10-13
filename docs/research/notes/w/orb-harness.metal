// ---- The orb (research sketch, not baud source): a glowing ball riding
// kitty's quad on the fast constant, a ribbon of glow inside the quad
// behind it, a Plummer halo, and a ring where it lands. Everything a
// function of age alone is evaluated once in Rust and rides in `k`:
//   k[0..1] lead   the orb's centre this frame (a fifth point of kitty's
//                  chase, on TAU_FAST, retargeted from where it is)
//   k[2]    R      the orb's radius, px
//   k[3]    glow   its brightness now, 0..1
//   k[4..5] ring   where it landed
//   k[6]    ring_r the ring's radius now, px
//   k[7]    ring_a the ring's density gain now (0 while in flight)
// The shape's own numbers are literals, mirrored by the Rust reference.
#define ORB_K_CORE 2.0
#define ORB_K_BODY 0.9
#define ORB_K_RIM 1.3
#define ORB_HALO 0.8        // Plummer radius, fraction of R
#define ORB_K_HALO 0.9
#define ORB_RIBBON 0.5      // ribbon depth scale, fraction of the cell height
#define ORB_GLOW_OUT 0.25   // ribbon's outer glow, fraction of the cell height
#define ORB_WAKE_LEN 2.5    // ribbon's e-fold behind the orb, cells
#define ORB_K_WAKE 1.0
#define ORB_RING_W 0.07     // ring's Gaussian width, fraction of the cell height

static inline float orb_sq(float x) { return x * x; }

// The density at `p`: an optical depth, turned into opacity by 1 - e^-D.
static inline float orb_density(float2 p, constant EffectU& f) {
    float c = f.unit;
    float d = fx_distance(p, f);
    if (d > f.reach * c) return 0.0;
    float2 lead = float2(f.k[0], f.k[1]);
    float R = f.k[2];
    float glow = f.k[3];
    float r = length(p - lead);
    // A glowing sphere seen in one colour: the chord through an emitting
    // ball (limb darkening of an optically thin emitter, mu), a hot core
    // (mu^3), and Schlick's Fresnel shell at the limb, (1-mu)^5, cut at the
    // disc's edge by a pixel of coverage.
    float mu = sqrt(max(1.0 - orb_sq(r / R), 0.0));
    float m = 1.0 - mu;
    float edge = saturate(R - r + 0.5);
    float body = ORB_K_CORE * mu * mu * mu + ORB_K_BODY * mu + ORB_K_RIM * (m * m * m * m * m) * edge;
    // Plummer's projected surface density, (1 + r^2/a^2)^-2: the halo of a
    // softened glowing ball, continuous across the limb.
    float halo = ORB_K_HALO / orb_sq(1.0 + orb_sq(r / (ORB_HALO * R)));
    // The wake: a ribbon of glow inside kitty's quad, brightest at its
    // spine, a quarter at its edge, then a Gaussian skirt outside it, all
    // falling off with distance from the orb.
    float depth = saturate(-d / (ORB_RIBBON * c));
    float wake = d <= 0.0 ? 0.25 + 0.75 * depth : 0.25 * exp(-orb_sq(d / (ORB_GLOW_OUT * c)));
    float along = exp(-r / (ORB_WAKE_LEN * c));
    float D = glow * (body + halo + ORB_K_WAKE * wake * along);
    float ring_a = f.k[7];
    if (ring_a > 0.0) {
        float e = length(p - float2(f.k[4], f.k[5])) - f.k[6];
        D += ring_a * exp(-orb_sq(e / (ORB_RING_W * c)));
    }
    return D;
}

fragment float4 orb_fragment(GOut in [[stage_in]], constant EffectU& f [[buffer(0)]]) {
    float2 p = in.pos.xy - f.origin;
    // The hole first, for motes as well as bands: nothing covers the cursor.
    if (fx_in_hole(p, f)) return float4(0.0);
    if (in.kind == KIND_PARTICLE) return fire_ember(in);
    float a = in.color.a * (1.0 - exp(-orb_density(p, f)));
    return float4(in.color.rgb * a, a);
}
