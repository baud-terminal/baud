#version 450
#extension GL_GOOGLE_include_directive : require

// ---- The ufo, `crates/baud-render/src/ufo.rs` (`ufo::light`): a saucer springing
// along the path onto the quad's centroid, a wake, a tractor beam onto the
// cursor, and exhaust rings. Its bands cover only the box `ufo::region`
// names. Its own numbers, `ufo::PARAMS`: `k[0]` the hover in cells, `k[1]`
// the size, `k[2]` the beam's peak, `k[3]` the wake's, `k[4]` the rim
// lights' turns a second, `k[5]` the arc in cells, `k[6]` the bank in
// radians, `k[7]` the earliest age it leaves at.

#include "fx_common.glsl"

layout(location = 0) out vec4 out_color;

struct UfoPose {
    vec2 c;
    float cs, sn, s, vis, cr, sr, side, gap, beam, ext;
};

float ufo_cov(float d) { return clamp(0.5 - d, 0.0, 1.0); }

float ufo_flight() { return clamp(0.12 + 0.006 * fx_len() / fx_unit_len(), 0.16, 0.32); }

float ufo_omega() { return 4.0 / ufo_flight(); }

// When the saucer leaves: after its beam, 1.6 flights and a quarter second
// in, never before `k[7]`.
float ufo_depart() { return max(fx_k(7u), 1.6 * ufo_flight() + 0.25); }

// **The saucer at `a` seconds**: a critically damped spring from the tail to
// the quad's centroid, an arc away from the cursor, a hover and a bob, a bank
// into the travel, and its departure.
UfoPose ufo_pose(float a) {
    float c = fx_unit_len();
    vec2 cq = (fx_corner(0u) + fx_corner(1u) + fx_corner(2u) + fx_corner(3u)) * 0.25;
    float w = ufo_omega();
    float sig = 1.0 - (1.0 + w * a) * exp(-w * a);
    vec2 on = fx_tail() + (cq - fx_tail()) * sig;
    vec2 g = (f.hole.xy + f.hole.zw) * 0.5 - on;
    float gap = length(g);
    float side = f.hole.y >= 1.35 * c ? 1.0 : -1.0;
    vec2 pth = cq - fx_tail();
    float pl = length(pth);
    vec2 n = pl > 1e-3 ? vec2(-pth.y, pth.x) / pl : vec2(0.0, -side);
    if (n.y * side > 0.0) n = -n;
    float arc = min(fx_k(5u) * c, 0.12 * fx_len()) * 4.0 * sig * (1.0 - sig);
    float hov = 1.0 - smoothstep(0.1 * c, 0.6 * c, gap);
    float h1 = fx_unit(fx_hash(fx_seed() ^ 0x5bd1e995u));
    float h2 = fx_unit(fx_hash(fx_seed()));
    float bob = 0.035 * c * sin(PI * (2.0 * fract(2.2 * a + h1) - 1.0)) * hov;
    float t = ufo_flight();
    float dep = ufo_depart();
    float tl = max(a - dep, 0.0);
    float kk = clamp(tl / 0.18, 0.0, 1.0);
    UfoPose P;
    P.c = on + n * arc + vec2(0.0, -side * fx_k(0u) * c + bob - side * 46.0 * c * tl * tl);
    float th = fx_k(6u) * clamp(g.x / (3.0 * c), -1.0, 1.0)
             + 0.05 * sin(PI * (2.0 * fract(1.3 * a + h2) - 1.0)) * hov;
    P.cs = cos(th);
    P.sn = sin(th);
    float grow = smoothstep(0.0, 0.08, a);
    P.vis = grow * (1.0 - smoothstep(0.0, 1.0, kk));
    P.s = fx_k(1u) * (0.5 + 0.5 * grow) * (1.0 - 0.6 * kk * kk);
    float rho = 2.0 * fract(fx_k(4u) * a + h2) - 1.0;
    P.cr = cos(PI * rho);
    P.sr = sin(PI * rho);
    P.side = side;
    P.gap = gap;
    P.ext = smoothstep(1.3 * t, 1.3 * t + 0.1, a) * (1.0 - smoothstep(dep - 0.08, dep, a));
    P.beam = P.ext * (1.0 - smoothstep(0.15 * c, 0.6 * c, gap));
    return P;
}

// The hull: a vesica, pointed tips along x.
float ufo_vesica(vec2 q) {
    vec2 p = abs(q.yx);
    if ((p.y - 0.53) * 1.221818 > p.x * 0.53) return length(p - vec2(0.0, 0.53));
    return length(p + vec2(1.221818, 0.0)) - 1.331818;
}

// The dome: iq's cut disk, radius 0.22 cut at its centre, y up.
float ufo_dome(vec2 p) {
    const float r = 0.22;
    p.x = abs(p.x);
    float s = max(-r * p.x * p.x + r * r * (r - 2.0 * p.y), -r * p.y);
    return s < 0.0 ? length(p) - r : (p.x < r ? -p.y : length(p - vec2(r, 0.0)));
}

float ufo_smin(float a, float b, float k) {
    k *= 4.0;
    float h = max(k - abs(a - b), 0.0) / k;
    return min(a, b) - h * h * k * 0.25;
}

const vec2 UFO_LIGHTS[5] = vec2[5](
    vec2(1.0, 0.0), vec2(0.309017, 0.951057), vec2(-0.809017, 0.587785),
    vec2(-0.809017, -0.587785), vec2(0.309017, -0.951057)
);

// **The saucer's body**: the hull, the dome and the rim lights.
float ufo_body(vec2 p, UfoPose P, float c) {
    float u = P.s * c;
    vec2 d = p - P.c;
    float lim = 0.62 * u + 1.0;
    if (dot(d, d) > lim * lim) return 0.0;
    vec2 q = vec2(P.cs * d.x + P.sn * d.y, -P.sn * d.x + P.cs * d.y) / u;
    float dh = ufo_vesica(q) - 0.02;
    float sil = ufo_smin(dh, ufo_dome(vec2(q.x, -0.04 - q.y)), 0.03);
    float light = 0.0;
    if (abs(q.y - 0.03) < 0.09) {
        for (uint i = 0u; i < 5u; i++) {
            float cphi = P.cr * UFO_LIGHTS[i].x - P.sr * UFO_LIGHTS[i].y;
            float sphi = P.sr * UFO_LIGHTS[i].x + P.cr * UFO_LIGHTS[i].y;
            float dl = length(q - vec2(0.40 * sphi, 0.03)) - 0.045;
            light = max(light, smoothstep(0.0, 0.35, cphi) * ufo_cov(dl * u));
        }
    }
    return (0.50 * ufo_cov(sil * u) + ufo_cov(dh * u) * (0.30 + 0.20 * light)) * P.vis;
}

// **The tractor beam**: a trapezoid from under the saucer to the cursor's
// near edge, a brighter core, rings travelling down it.
float ufo_beam(vec2 p, UfoPose P) {
    if (P.beam <= 0.0) return 0.0;
    float c = fx_unit_len();
    vec2 top = P.c + vec2(0.0, P.side * 0.117 * c * P.s);
    vec2 end = vec2((f.hole.x + f.hole.z) * 0.5, P.side > 0.0 ? f.hole.y : f.hole.w);
    vec2 ax = end - top;
    float ln = length(ax);
    if (ln < 1.0) return 0.0;
    vec2 nn = ax / ln;
    vec2 d = p - top;
    float along = dot(d, nn);
    float across = abs(d.x * nn.y - d.y * nn.x);
    float ff = clamp(along / ln, 0.0, 1.0);
    float wide = max(0.5 * (f.hole.z - f.hole.x) + 0.08 * c, 0.22 * c);
    float hw = 0.14 * c * P.s + (wide - 0.14 * c * P.s) * ff;
    float edge = clamp(hw - across + 0.5, 0.0, 1.0) * clamp(ln * P.ext - along + 0.5, 0.0, 1.0)
               * clamp(along + 0.5, 0.0, 1.0);
    if (edge <= 0.0) return 0.0;
    float core = 1.0 - smoothstep(0.0, 0.6, across / max(hw, 1.0));
    float fr = fract(along / (0.22 * c) - fract(5.0 * fx_age()));
    return fx_k(2u) * P.vis * P.beam * edge * (0.55 + 0.45 * core) * (0.6 + 0.4 * ff)
         * (1.0 - 0.88 * fr * (1.0 - fr));
}

// **The wake**: a tapered streak along the saucer's last 60 ms.
float ufo_wake(vec2 p, UfoPose P) {
    float c = fx_unit_len();
    if (fx_age() <= 0.0) return 0.0;
    UfoPose Q = ufo_pose(max(fx_age() - 0.06, 0.0));
    vec2 v = P.c - Q.c;
    float l = length(v);
    if (l < 0.1 * c) return 0.0;
    vec2 a = P.c - v / l * (0.2475 * c * P.s);
    vec2 ba = Q.c - a;
    vec2 pa = p - a;
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    float dd = length(pa - ba * h) - (0.055 * c + (0.012 * c - 0.055 * c) * h);
    return fx_k(3u) * P.vis * ufo_cov(dd) * (1.0 - h) * smoothstep(0.1 * c, 0.6 * c, l);
}

void main() {
    if (in_kind == KIND_PARTICLE) {
        float a = in_color.a * (1.0 - smoothstep(0.0, 0.22, abs(length(in_uv) - 0.72)));
        out_color = vec4(in_color.rgb * a, a);
        return;
    }
    vec2 p = fx_place();
    if (fx_in_hole(p) || fx_age() >= ufo_depart() + 0.18) {
        out_color = vec4(0.0);
        return;
    }
    UfoPose P = ufo_pose(fx_age());
    float a = ufo_wake(p, P);
    float b = ufo_beam(p, P);
    a = b + a * (1.0 - b);
    float s = ufo_body(p, P, fx_unit_len());
    a = s + a * (1.0 - s);
    a *= in_color.a;
    out_color = vec4(in_color.rgb * a, a);
}
