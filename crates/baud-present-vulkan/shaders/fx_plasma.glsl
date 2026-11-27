// ---- The plasma, `crates/baud-render/src/plasma.rs` (`plasma::parts`): a tube
// filling the quad, a sheath around it, and three filaments braided along
// the axis into the cursor. Its own numbers, `plasma::params`: `k[0]` the
// braid's half width in cells, `k[1..3]` the axis, `k[3]` the tube, `k[4]`
// the sheath, `k[5]` its decay, `k[6]` the filaments' gain, `k[7]` the
// knots' rate. Shared by `fx_plasma.frag` and `fx_plasma_multiply.frag`.


// sin(2 pi x), its argument folded into [-1, 1) first.
float px_wave(float x) { return sin(PI * (2.0 * fract(x + 0.5) - 1.0)); }

// Per filament: cycles a cell and a second of its wave, of its harmonic, the
// harmonic's share, its width in cells and its gain.
const vec3 PX_K = vec3(0.23, 0.31, 0.17);
const vec3 PX_F = vec3(1.10, 1.35, 0.80);
const vec3 PX_K2 = vec3(0.71, 0.93, 0.53);
const vec3 PX_F2 = vec3(-0.85, -1.05, -1.20);
const vec3 PX_M = vec3(0.22, 0.20, 0.25);
const vec3 PX_W = vec3(0.040, 0.034, 0.050);
const vec3 PX_G = vec3(1.00, 0.85, 0.65);

// **The plasma at `p`**: the tube and its sheath dimming together, and the
// braid behind the head -- nothing ahead of it -- converging into the cursor.
vec4 plasma_tube(vec2 p, vec4 color) {
    if (fx_in_hole(p)) return vec4(0.0);
    float k = fx_age() / fx_life();
    if (k >= 1.0) return vec4(0.0);
    float c = fx_unit_len();
    float d0 = fx_distance(p) / c;
    if (d0 > fx_reach()) return vec4(0.0);
    float env = (1.0 - k) * (1.0 - k);
    float inside = 1.0 - smoothstep(-0.03, 0.03, d0);
    float tube = fx_k(3u) * inside + fx_k(4u) * exp(-fx_k(5u) * (1.0 + 1.5 * k) * max(d0, 0.0));
    float fil = 0.0;
    if (d0 < 0.15) {
        vec2 a = vec2(fx_k(1u), fx_k(2u));
        vec2 r = p - 0.5 * (f.hole.xy + f.hole.zw);
        float s = -dot(r, a) / c;
        float w = (a.x * r.y - a.y * r.x) / c;
        // The ignition's hand, `plasma::Hand`: a leftward move's braid is
        // the rightward one's mirror image, its offset across negated.
        if (fx_dir().x < 0.0) w = -w;
        float amp = fx_k(0u) * 0.62 * smoothstep(0.0, 2.0, s);
        uint salt = fx_seed() ^ 0xA511E9B3u;
        float kn = fx_unit(fx_hash(salt + 8u));
        for (uint i = 0u; i < 3u; i++) {
            float ph = fx_unit(fx_hash(salt + i));
            float ph2 = fx_unit(fx_hash(salt + 16u + i));
            float along = amp * ((1.0 - PX_M[i]) * px_wave(PX_K[i] * s + ph + PX_F[i] * fx_age())
                               + PX_M[i] * px_wave(PX_K2[i] * s + ph2 + PX_F2[i] * fx_age()));
            float wid = max(PX_W[i] * (1.0 - 0.5 * k), 1.1 / c);
            float e = (w - along) / wid;
            float knot = 0.72 + 0.28 * px_wave(0.45 * s + kn + float(i) / 3.0 + fx_k(7u) * fx_age());
            fil += PX_G[i] * exp(-e * e) * knot;
        }
        fil = clamp(fil, 0.0, 1.0) * fx_k(6u) * (1.0 - smoothstep(-0.02, 0.15, d0))
            * smoothstep(0.0, 0.3, s);
    }
    float light = clamp(env * (tube + (1.0 - tube) * fil), 0.0, 1.0);
    float alpha = fx_cap() == 1u ? min(light, color.a) : color.a * light;
    return vec4(color.rgb * alpha, alpha);
}

vec4 plasma_color() {
    return in_kind == KIND_PARTICLE ? fx_particle() : plasma_tube(fx_place(), in_color);
}
