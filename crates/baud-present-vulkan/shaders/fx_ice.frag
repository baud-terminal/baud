#version 450
#extension GL_GOOGLE_include_directive : require

// ---- The ice, `crates/baud-render/src/ice.rs` (`ice::light`): a pane over the
// quad, and over the path a frost lace of six-fold crystals, a glint and a
// rime haze, melting tail first; shards are hexagonal plates. Its own
// numbers, `ice::PARAMS`: `k[0]` the crystals' spacing, `k[1]` the longest
// arm, `k[2]` the line's half width, `k[3]` the side branches' spacing, in
// cells; `k[4]` a crystal's growth, `k[5]` the hold, `k[6]` the melt's
// spread and `k[7]` a crystal's melt, in seconds. `ICE_LEAD` and the salts
// are held to the Rust constants by the crate's tests.

#include "fx_common.glsl"

layout(location = 0) out vec4 out_color;

const float ICE_LEAD = 0.01;
const uint ICE_SALT = 0x1CE01CE0u;
const uint RIME_SALT = 0x51ED0000u;

float ice_line(float d, float w, float aa) { return 1.0 - smoothstep(w - aa, w + aa, d); }

// When the frost reaches `u` of the path: the leading corners, inverted.
float ice_dep(float u) { return -ICE_LEAD * log2(max(1.0 - 0.999 * u, 1e-3)); }

// Crystal `k` at `rel`: rotated by a hashed angle through the tangent
// half-angle form, folded into one 30-degree wedge by two mirrors, then one
// arm, its side branches and its plate.
float ice_crystal(float k, vec2 rel, vec2 D, vec2 Pp, float lu, float W, float aa, uint sbase) {
    float S = fx_k(0u);
    float ARM = fx_k(1u);
    float B = fx_k(3u);
    float GROW = fx_k(4u);
    float HOLD = fx_k(5u);
    float SPREAD = fx_k(6u);
    float MELT = fx_k(7u);
    uint h1 = fx_hash(uint(int(k)) + sbase);
    uint h2 = fx_hash(h1);
    uint h3 = fx_hash(h2);
    uint h4 = fx_hash(h3);
    uint h5 = fx_hash(h4);
    uint h6 = fx_hash(h5);
    float along = (k + (fx_unit(h1) - 0.5) * 0.5) * S;
    float across = (fx_unit(h2) - 0.5) * 0.7;
    float win = smoothstep(-0.25, 0.0, along) * (1.0 - smoothstep(lu, lu + 0.25, along));
    if (win <= 0.0) return 0.0;
    float u = clamp(along / lu, 0.0, 1.0);
    float delay = 0.03 * fx_unit(h5);
    float x = clamp((fx_age() - ice_dep(u) - delay) / GROW, 0.0, 1.0);
    float grow = 1.0 - (1.0 - x) * (1.0 - x) * (1.0 - x);
    float m = HOLD + SPREAD * u + delay;
    float fade = 1.0 - smoothstep(m, m + MELT, fx_age());
    float L = ARM * (0.45 + 0.55 * fx_unit(h4)) * grow * (0.35 + 0.65 * fade);
    float exist = clamp(L / (2.0 * W), 0.0, 1.0);
    if (exist <= 0.0 || fade <= 0.0) return 0.0;
    vec2 r = rel - (D * along + Pp * across);
    float s = 0.57735027 * fx_unit(h3);
    float den = 1.0 + s * s;
    float cs = (1.0 - s * s) / den;
    float sn = 2.0 * s / den;
    vec2 w = abs(vec2(cs * r.x + sn * r.y, -sn * r.x + cs * r.y));
    const vec2 nA = vec2(-0.8660254, 0.5);
    const vec2 nB = vec2(-0.5, 0.8660254);
    w -= 2.0 * max(dot(w, nA), 0.0) * nA;
    w -= 2.0 * max(dot(w, nB), 0.0) * nB;
    float dm = length(w - vec2(clamp(w.x, 0.0, L), 0.0));
    float wm = W * (1.4 - 0.7 * clamp(w.x / max(L, 1e-4), 0.0, 1.0));
    float mainA = 0.95 * ice_line(dm, wm, aa);
    const vec2 e = vec2(0.5, 0.8660254);
    float xr = w.x - w.y * 0.57735027;
    float xj = max(floor(xr / B + 0.5), 1.0) * B;
    float lj = min(0.6 * (L - xj), xj);
    vec2 v = w - vec2(xj, 0.0);
    float ds = length(v - clamp(dot(v, e), 0.0, max(lj, 0.0)) * e);
    float sideA = 0.60 * ice_line(ds, W, aa) * clamp(lj / (2.0 * W), 0.0, 1.0);
    float pr = 0.10 + 0.30 * fx_unit(h6) * fx_unit(h6);
    float dp = dot(w, vec2(0.8660254, 0.5)) - pr * L * 0.8660254;
    float plateA = 0.30 * (1.0 - smoothstep(-aa, aa, dp));
    return max(mainA, max(sideA, plateA)) * exist * fade * win;
}

// **The ice at `p`**: the pane over the quad, and near the path the three
// nearest crystals with the glint, and the rime.
vec4 ice_frost(vec2 p, vec4 color) {
    if (fx_in_hole(p)) return vec4(0.0);
    float c = fx_unit_len();
    float lu = fx_len() / c;
    float S = fx_k(0u);
    float HOLD = fx_k(5u);
    float SPREAD = fx_k(6u);
    float MELT = fx_k(7u);
    vec2 rel = (p - fx_tail()) / c;
    vec2 D = fx_dir();
    vec2 Pp = vec2(-D.y, D.x);
    float along = dot(rel, D);
    float dseg = length(rel - D * clamp(along, 0.0, lu));
    float dq = fx_distance(p);
    if (dseg > fx_reach() && dq / c > fx_reach()) return vec4(0.0);
    float pa = clamp(1.0 - fx_age() / (0.6 * HOLD), 0.0, 1.0);
    float pane = pa * max(0.14 * (1.0 - smoothstep(-0.5, 0.5, dq)),
                          0.5 * (1.0 - smoothstep(0.5, 1.5, abs(dq))));
    float a = pane;
    if (dseg <= fx_reach()) {
        float W = max(fx_k(2u), 0.75 / c);
        float aa = 0.5 / c;
        uint sbase = fx_hash(fx_seed() + ICE_SALT);
        float k0 = floor(along / S + 0.5);
        float frost = 0.0;
        for (int dk = -1; dk <= 1; dk++)
            frost = max(frost, ice_crystal(k0 + float(dk), rel, D, Pp, lu, W, aa, sbase));
        float gp = (fx_age() - 0.4 * HOLD) / (0.6 * HOLD) * (lu + 2.0 * S) - S;
        frost = clamp(frost * (1.0 + 0.3 * (1.0 - smoothstep(0.0, S, abs(along - gp)))), 0.0, 1.0);
        float u = clamp(along / lu, 0.0, 1.0);
        float front = (lu + 0.6) * (1.0 - exp2(-fx_age() / ICE_LEAD)) - 0.3;
        float dep = 1.0 - smoothstep(front - 0.3, front + 0.3, along);
        float mr = HOLD + SPREAD * u;
        float rfade = 1.0 - smoothstep(mr, mr + MELT, fx_age());
        uint s2 = fx_seed() ^ RIME_SALT;
        vec2 q2 = vec2(0.8 * rel.x - 0.6 * rel.y, 0.6 * rel.x + 0.8 * rel.y);
        float n = 0.65 * fx_noise(rel * 2.3, s2) + 0.35 * fx_noise(q2 * 6.7, s2 + 0x9E3779B9u);
        float rime = 0.24 * (1.0 - smoothstep(0.15, 0.6, dseg)) * smoothstep(0.35, 0.75, n) * dep * rfade;
        a = max(a, max(frost, rime));
    }
    a *= color.a;
    return vec4(color.rgb * a, a);
}

// iq's hexagon (MIT), for a shard: a plate with its flats at `r`.
float ice_hex(vec2 p, float r) {
    const vec3 k = vec3(-0.866025404, 0.5, 0.577350269);
    p = abs(p);
    p -= 2.0 * min(dot(k.xy, p), 0.0) * k.xy;
    p -= vec2(clamp(p.x, -k.z * r, k.z * r), r);
    return length(p) * sign(p.y);
}

void main() {
    if (in_kind == KIND_PARTICLE) {
        float d = ice_hex(in_uv, 0.70);
        float a = in_color.a * (1.0 - smoothstep(-0.08, 0.12, d))
                * (0.55 + 0.45 * smoothstep(-0.35, -0.02, d));
        out_color = vec4(in_color.rgb * a, a);
        return;
    }
    out_color = ice_frost(fx_place(), in_color);
}
