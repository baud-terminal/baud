#version 450
#extension GL_GOOGLE_include_directive : require

// ---- The lightning, `crates/baud-render/src/lightning.rs` (`lightning::light`):
// a bolt from the cell the cursor left to the cell it reached, over a box
// along the path (the uniform's corners, `lightning::region`) that does not
// move. Its own numbers, `lightning::PARAMS`: `k[0]` the core's half width
// and `k[1]` the glow's width in cells, `k[2]` the glow's peak, `k[3]` the
// leader's crossing, `k[4]` a pixel's rise and `k[5]` when the fade starts,
// in seconds, `k[6]` the corona's peak and `k[7]` its width in cells. The
// bolt's own shape is the constants below, which the crate's tests hold to
// the Rust constants of the same names.

#include "fx_common.glsl"

layout(location = 0) out vec4 out_color;

const float BOLT_JAG = 0.8;
const float BOLT_CAP = 0.25;
const float BOLT_S_MIN = 0.30;
const uint BOLT_LEVELS = 8u;
const uint BOLT_SALT = 0x51ED270Bu;
const uint FORK_SALT = 0xA24BAED4u;
const float FORK_MIN = 5.0;
const float FORK_P = 0.75;

// The distance from `p` to the segment `a` to `b`.
float fx_segment(vec2 p, vec2 a, vec2 b) {
    vec2 ba = b - a;
    vec2 pa = p - a;
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

// As many levels as keep every segment at least BOLT_S_MIN cells long.
uint bolt_levels(float len, float c) {
    uint l = 0u;
    while (l < BOLT_LEVELS && len >= (c * float(1u << (l + 1u))) * BOLT_S_MIN) l++;
    return l;
}

// Midpoint displacement at `u` as its Schauder sum: one tent a level, zero at
// both ends.
float bolt_offset(float u, uint lv) {
    float y = 0.0;
    for (uint l = 0u; l < lv; l++) {
        float n = float(1u << l);
        float x = u * n;
        float i = floor(x);
        float t = x - i;
        float tent = 1.0 - abs(2.0 * t - 1.0);
        float xi = 2.0 * fx_lattice(ivec2(int(i), int(l)), fx_seed() ^ BOLT_SALT) - 1.0;
        float d = min(BOLT_JAG * fx_len() / (2.0 * n), BOLT_CAP * fx_unit_len());
        y += d * xi * tent;
    }
    return y;
}

vec2 bolt_vertex(int j, int m, uint lv, vec2 n) {
    float u = float(j) / float(m);
    return fx_tail() + fx_dir() * (fx_len() * u) + n * bolt_offset(u, lv);
}

// The distance to fork `k`, at a quarter of the path a fork, 25-45 degrees
// off it, two segments -- infinity where it did not grow.
float bolt_fork(vec2 p, uint k, uint lv, vec2 n) {
    uint h0 = fx_hash(k + fx_hash(fx_seed() ^ FORK_SALT));
    if (fx_unit(h0) >= FORK_P) return fx_inf();
    uint h1 = fx_hash(h0);
    uint h2 = fx_hash(h1);
    uint h3 = fx_hash(h2);
    uint h4 = fx_hash(h3);
    float q = 0.25 * float(k + 1u);
    float yr = bolt_offset(q, min(lv, 2u));
    float side = abs(yr) > 0.6 * fx_unit_len() ? -sign(yr) : (fx_unit(h1) < 0.5 ? 1.0 : -1.0);
    vec2 v = fx_dir() + n * (side * (0.466 + 0.534 * fx_unit(h2)));
    vec2 fd = v / sqrt(dot(v, v));
    float lf = min((0.18 + 0.14 * fx_unit(h3)) * fx_len(), 1.2 * fx_unit_len());
    vec2 r = fx_tail() + fx_dir() * (fx_len() * q) + n * yr;
    float bend = (fx_unit(h4) - 0.5) * 0.3 * lf;
    vec2 e = r + fd * (0.5 * lf) + vec2(-fd.y, fd.x) * bend;
    vec2 t = r + fd * lf;
    return min(fx_segment(p, r, e), fx_segment(p, e, t));
}

// **The bolt at `p`**: its core, glow and forks as the leader reaches the
// pixel and the fade takes it, and the corona round the cursor.
vec4 bolt_light(vec2 p, vec4 color) {
    if (fx_in_hole(p)) return vec4(0.0);
    if (fx_age() >= fx_life()) return vec4(0.0);
    float c = fx_unit_len();
    if (fx_distance(p) / c > fx_reach()) return vec4(0.0);
    float u = dot(p - fx_tail(), fx_dir()) / fx_len();
    float tau = fx_k(3u) * clamp(u, 0.0, 1.0);
    float rise = smoothstep(tau, tau + fx_k(4u), fx_age());
    if (rise <= 0.0) return vec4(0.0);
    float fall = 1.0 - smoothstep(fx_k(5u), fx_life(), fx_age());
    vec2 n = vec2(-fx_dir().y, fx_dir().x);
    uint lv = bolt_levels(fx_len(), c);
    int m = int(1u << lv);
    int a0 = max(clamp(int(floor(u * float(m))), 0, m - 1) - 1, 0);
    vec2 w0 = bolt_vertex(a0, m, lv, n);
    vec2 w1 = bolt_vertex(min(a0 + 1, m), m, lv, n);
    vec2 w2 = bolt_vertex(min(a0 + 2, m), m, lv, n);
    vec2 w3 = bolt_vertex(min(a0 + 3, m), m, lv, n);
    float dm = min(min(fx_segment(p, w0, w1), fx_segment(p, w1, w2)), fx_segment(p, w2, w3));
    float df = fx_inf();
    if (fx_len() >= FORK_MIN * c) {
        for (uint k = 0u; k < 3u; k++) df = min(df, bolt_fork(p, k, lv, n));
    }
    vec2 hc = (f.hole.xy + f.hole.zw) * 0.5;
    vec2 hh = (f.hole.zw - f.hole.xy) * 0.5;
    float dh = length(max(abs(p - hc) - hh, 0.0));
    float w = max(fx_k(0u) * c, 1.0) * (0.45 + 0.55 * fall);
    float gw = fx_k(1u) * c;
    float km = (dm / w) * (dm / w);
    float kf = (df / (0.7 * w)) * (df / (0.7 * w));
    float core = max(exp(-km * km), 0.7 * exp(-kf * kf));
    float gm = dm / gw;
    float gf = df / (0.75 * gw);
    float glow = fx_k(2u) * max(exp(-gm * gm), 0.6 * exp(-gf * gf));
    float gc = dh / (fx_k(7u) * c);
    // No gate of its own: every corona pixel is where the leader arrives
    // last, so the pixel's own rise gates it already.
    float corona = fx_k(6u) * exp(-gc * gc);
    float a = color.a * clamp(core * fall * fall + (glow + corona) * fall, 0.0, 1.0) * rise;
    return vec4(color.rgb * a, a);
}

void main() {
    if (in_kind == KIND_PARTICLE) {
        out_color = fx_particle();
        return;
    }
    out_color = bolt_light(fx_place(), in_color);
}
