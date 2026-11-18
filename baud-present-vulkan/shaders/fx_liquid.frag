#version 450
#extension GL_GOOGLE_include_directive : require

// ---- The liquid, `baud-render/src/liquid.rs` (`liquid::light`): a glass
// gel along a spine from the back of the quad to the cursor -- a necked
// tube, satellite beads and a head drop in one smooth union -- with a rim
// and a glint on a squircle bevel. The state is derived here from the
// uniform, as `liquid::state` derives it in Rust. Its own numbers,
// `liquid::PARAMS`: `k[0]` the tube's radius, `k[1]` the union's width,
// `k[2]` the head's gap, `k[3]` its wobble, `k[4]` the bevel, in cells;
// `k[5]` the body's, `k[6]` the rim's and `k[7]` the glint's opacity.

#include "fx_common.glsl"

layout(location = 0) out vec4 out_color;

const float LQ_TRAIL = 0.04;
const float LQ_ZETA = 0.28;
const float LQ_OMEGA = 40.840706;
const float LQ_BACK = 0.25;
const float LQ_CROSS = 0.6;
const float LQ_LEAN = 0.08;
const float LQ_LEAN_AT = 1.2;
const float LQ_REST_X = -0.5;
const float LQ_REST_Y = -0.866;
const uint LQ_COUNT_SALT = 0x5bd1e995u;
const uint LQ_BEAD_SALT = 0x9E3779B9u;
const float LQ_BEAD_IN = 0.02;

struct LiquidS {
    vec2 b;
    vec2 h;
    float radius;
    float neck;
    float inv_len2;
    float lip;
    vec2 half_box;
    float round_;
    float k;
    vec3 bead[3];
    vec2 light;
    float envelope;
    float end;
};

float lq_tube(vec2 d, float w, float h) { return fx_k(0u) * length(vec2(d.y * w, d.x * h)); }

// The trailing corners' contact with the head, and the wobble's amplitude.
vec2 lq_contact(float len, float c, float radius) {
    float t = LQ_TRAIL * log2(max(len / (radius + fx_k(2u) * c), 1.0));
    return vec2(t, fx_k(3u) * c * smoothstep(3.0, 12.0, len / c));
}

float lq_end(float len, float c, float radius) {
    vec2 ta = lq_contact(len, c, radius);
    float tw = ta.y > 0.5 ? log(ta.y / 0.5) / (LQ_ZETA * LQ_OMEGA) : 0.0;
    return max(ta.x + tw + 0.05, 0.22);
}

LiquidS liquid_state() {
    LiquidS s;
    float c = fx_unit_len();
    vec2 d = fx_dir();
    float hw = f.hole.z - f.hole.x;
    float hh = f.hole.w - f.hole.y;
    s.h = (f.hole.xy + f.hole.zw) * 0.5;
    vec2 m = (fx_corner(0u) + fx_corner(1u) + fx_corner(2u) + fx_corner(3u)) * 0.25;
    float lo = fx_inf();
    float hi = fx_inf();
    for (uint i = 0u; i < 4u; i++) {
        float v = dot(fx_corner(i) - m, d);
        if (v < lo) {
            hi = lo;
            lo = v;
        } else if (v < hi) {
            hi = v;
        }
    }
    s.b = m + d * (0.5 * (lo + hi));
    float l = length(s.h - s.b);
    s.inv_len2 = l > 1e-3 ? 1.0 / (l * l) : 0.0;
    // The cell's extent: the cursor's, floored at the nominal cell.
    float r = lq_tube(d, max(hw, 0.5 * c), max(hh, c));
    s.radius = r * smoothstep(0.0, 2.0 * r, l);
    float neck_max = 1.45 * smoothstep(1.5, 4.0, fx_len() / c);
    s.neck = neck_max * smoothstep(0.03, 0.14, fx_age());
    s.lip = l > 1e-3 ? 4.0 * s.neck * s.radius / l : 0.0;
    float nominal = lq_tube(d, 0.5 * c, c);
    vec2 ta = lq_contact(fx_len(), c, nominal);
    float sw = fx_age() - ta.x;
    float wob = 0.0;
    if (sw > 0.0) {
        float wd = LQ_OMEGA * sqrt(1.0 - LQ_ZETA * LQ_ZETA);
        wob = ta.y * exp(-LQ_ZETA * LQ_OMEGA * sw) * sin(wd * sw);
    }
    float ax = abs(d.x);
    float ay = abs(d.y);
    s.round_ = fx_k(2u) * c + fx_k(3u) * c + 1.0;
    s.half_box = vec2(max(0.5 * hw + fx_k(2u) * c + wob * ax - LQ_CROSS * wob * ay, s.round_),
                      max(0.5 * hh + fx_k(2u) * c + wob * ay - LQ_CROSS * wob * ax, s.round_));
    for (uint j = 0u; j < 3u; j++) s.bead[j] = vec3(0.0);
    if (neck_max > 1.0) {
        float xp = 0.5 - sin(asin(1.0 - 2.0 / neck_max) / 3.0);
        float tp = 0.03 + 0.11 * xp;
        float sg = fx_age() - tp;
        uint count = 2u + (fx_hash(fx_seed() ^ LQ_COUNT_SALT) & 1u);
        vec2 n = vec2(-d.y, d.x);
        for (uint j = 0u; j < count; j++) {
            if (sg < 0.0) break;
            uint h0 = fx_hash(fx_seed() ^ ((j + 1u) * LQ_BEAD_SALT));
            uint h1 = fx_hash(h0);
            uint h2 = fx_hash(h1);
            uint h3 = fx_hash(h2);
            float u0 = 0.5 + (fx_unit(h0) - 0.5) * 0.16;
            float tau = 0.045 + 0.04 * fx_unit(h2);
            float u = u0 + (1.0 - u0) * (1.0 - exp(-sg / tau));
            float rj = r * (0.30 - 0.07 * float(j)) * (0.85 + 0.3 * fx_unit(h1))
                     * (1.0 - smoothstep(0.85, 0.99, u)) * smoothstep(0.0, LQ_BEAD_IN, sg);
            float o = (fx_unit(h3) - 0.5) * 0.25 * r * exp(-sg / 0.06);
            s.bead[j] = vec3(s.b + (s.h - s.b) * u + n * o, rj);
        }
    }
    // The light starts leaning into the move -- the rest light plus
    // LQ_LEAN_AT of the move's direction -- and turns back to the upper left
    // the short way round, by its angle: a blend of the two vectors over time
    // passes through nothing for a move opposite the rest light.
    float rest = atan(LQ_REST_Y, LQ_REST_X);
    float turn = atan(LQ_REST_Y + LQ_LEAN_AT * d.y, LQ_REST_X + LQ_LEAN_AT * d.x) - rest;
    if (turn > PI) turn -= 2.0 * PI;
    else if (turn <= -PI) turn += 2.0 * PI;
    float th = rest + turn * exp(-fx_age() / LQ_LEAN);
    s.light = vec2(cos(th), sin(th));
    s.k = fx_k(1u) * c;
    s.end = lq_end(fx_len(), c, nominal);
    s.envelope = smoothstep(0.0, 0.035, fx_age()) * (1.0 - smoothstep(s.end - 0.10, s.end, fx_age()));
    return s;
}

// iq's quadratic smooth minimum, with its gradient.
vec3 lq_smin(vec3 a, vec3 b, float k) {
    float h = 1.0 - min(abs(a.x - b.x) / (4.0 * k), 1.0);
    float s = h * h * k;
    float w = 0.5 * h;
    return a.x < b.x ? vec3(a.x - s, a.yz + (b.yz - a.yz) * w)
                     : vec3(b.x - s, b.yz + (a.yz - b.yz) * w);
}

vec2 lq_dir(vec2 v, float l) { return l > 1e-3 ? v / l : vec2(0.0); }

vec3 liquid_field(vec2 p, LiquidS s) {
    vec2 ph = p - s.h;
    vec2 sg = vec2(ph.x >= 0.0 ? 1.0 : -1.0, ph.y >= 0.0 ? 1.0 : -1.0);
    vec2 w = abs(ph) - (s.half_box - s.round_);
    float g = max(w.x, w.y);
    vec3 fl;
    if (g > 0.0) {
        vec2 wp = max(w, 0.0);
        float lw = length(wp);
        fl = vec3(lw - s.round_, sg * wp / lw);
    } else {
        fl = vec3(g - s.round_, w.x > w.y ? vec2(sg.x, 0.0) : vec2(0.0, sg.y));
    }
    if (s.inv_len2 > 0.0 && s.radius > 0.0) {
        vec2 e = s.h - s.b;
        float u = clamp(dot(p - s.b, e) * s.inv_len2, 0.0, 1.0);
        vec2 v = p - (s.b + e * u);
        float l = length(v);
        float q = 1.0 - 4.0 * s.neck * u * (1.0 - u);
        float sl = s.lip * (2.0 * u - 1.0);
        fl = lq_smin(fl, vec3((l - s.radius * q) / sqrt(1.0 + sl * sl), lq_dir(v, l)), s.k);
    }
    for (uint i = 0u; i < 3u; i++) {
        vec3 bd = s.bead[i];
        if (bd.z > 0.0) {
            vec2 v = p - bd.xy;
            float l = length(v);
            fl = lq_smin(fl, vec3(l - bd.z, lq_dir(v, l)), s.k);
        }
    }
    return fl;
}

// **The gel at `p`**: its body, rim, glint and counter-glint.
vec4 liquid_light(vec2 p, vec4 color) {
    if (fx_in_hole(p)) return vec4(0.0);
    LiquidS s = liquid_state();
    float c = fx_unit_len();
    if (fx_age() >= s.end) return vec4(0.0);
    vec2 e = s.h - s.b;
    float u0 = clamp(dot(p - s.b, e) * s.inv_len2, 0.0, 1.0);
    float ds = s.inv_len2 > 0.0 ? length(p - (s.b + e * u0)) : fx_inf();
    vec2 o = max(max(f.hole.xy - p, p - f.hole.zw), 0.0);
    if (min(ds, length(o)) > fx_reach() * c) return vec4(0.0);
    vec3 fd = liquid_field(p, s);
    float cov = clamp(0.5 - fd.x, 0.0, 1.0);
    if (cov <= 0.0) return vec4(0.0);
    float t = clamp(-fd.x / (fx_k(4u) * c), 0.0, 1.0);
    float sv = 1.0 - t;
    float s2 = sv * sv;
    float s3 = s2 * sv;
    float z = sqrt(sqrt(max(1.0 - s2 * s2, 0.0)));
    float z3 = z * z * z;
    float nz = z3 / sqrt(max(s3 * s3 + z3 * z3, 1e-12));
    float fr = 1.0 - nz;
    float rim = 0.04 + 0.96 * fr * fr;
    float gl = dot(fd.yz, fd.yz);
    vec2 n = gl > 1e-12 ? fd.yz / sqrt(gl) : vec2(0.0);
    float bt = clamp((t - 0.04) * 2.5, 0.0, 1.0);
    float band = 4.0 * bt * (1.0 - bt);
    float nl = dot(n, s.light);
    float sp = max(nl, 0.0);
    sp = sp * sp * sp * band;
    float bk = max(-nl, 0.0);
    bk = bk * bk * bk * band;
    float a = color.a * s.envelope * cov
            * clamp(fx_k(5u) + fx_k(6u) * rim + fx_k(7u) * sp + LQ_BACK * bk, 0.0, 1.0);
    return vec4(color.rgb * a, a);
}

void main() {
    if (in_kind == KIND_PARTICLE) {
        out_color = fx_particle();
        return;
    }
    out_color = liquid_light(fx_place(), in_color);
}
