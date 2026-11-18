#version 450
#extension GL_GOOGLE_include_directive : require

// ---- The orb, `baud-render/src/orb.rs` (`orb::light`): a glowing sphere
// flying from the tail onto the cursor through the quad's leading corners,
// a halo, a comet of its last flight, and sparks as it lands. Its bands
// cover the line it flies on. Its own numbers, `orb::PARAMS`: `k[0]` its
// radius and `k[1]` its halo's width in cells, `k[2]` the halo's peak,
// `k[3]` the comet's, `k[4]` the comet's seconds, `k[5]` the appear, `k[6]`
// the fade, `k[7]` the linger after arrival.

#include "fx_common.glsl"

layout(location = 0) out vec4 out_color;

float orb_flight() { return clamp(0.10 + 0.004 * fx_len() / fx_unit_len(), 0.12, 0.25); }

// How far along its flight the orb is at `a`: a smootherstep over the
// flight, from rest to rest, 1 exactly at its end.
float orb_along(float a) {
    float u = clamp(a / orb_flight(), 0.0, 1.0);
    return u * u * u * (u * (u * 6.0 - 15.0) + 10.0);
}

// Where the orb flies to: the cursor's centre, displaced by the mean of how
// far the quad's corners facing the ignition's direction -- those past the
// cursor's centre along it -- still are from the cursor's. The cursor's
// corners are in the quad's order.
vec2 orb_aim() {
    vec2 m = vec2((f.hole.x + f.hole.z) * 0.5, (f.hole.y + f.hole.w) * 0.5);
    vec2 hc[4] = vec2[4](vec2(f.hole.z, f.hole.y), vec2(f.hole.z, f.hole.w),
                         vec2(f.hole.x, f.hole.w), vec2(f.hole.x, f.hole.y));
    vec2 s = vec2(0.0);
    float n = 0.0;
    for (int i = 0; i < 4; i++) {
        if ((hc[i].x - m.x) * fx_dir().x + (hc[i].y - m.y) * fx_dir().y > 0.0) {
            s += fx_corner(uint(i)) - hc[i];
            n += 1.0;
        }
    }
    return n > 0.0 ? m + s / n : m;
}

// Where the orb is at `a`, on the line from the tail to its aim.
vec2 orb_centre(vec2 aim, float a) { return fx_tail() + (aim - fx_tail()) * orb_along(a); }

// The distance from `p` to the line the orb flies on, tail to its aim.
float orb_line(vec2 p, vec2 aim) {
    vec2 ba = aim - fx_tail();
    vec2 pa = p - fx_tail();
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

vec4 orb_light(vec2 p, vec4 color) {
    if (fx_in_hole(p)) return vec4(0.0);
    float c = fx_unit_len();
    float t = orb_flight();
    float end = t + fx_k(7u) + fx_k(6u);
    if (fx_age() >= end) return vec4(0.0);
    vec2 aim = orb_aim();
    if (orb_line(p, aim) / c > fx_reach()) return vec4(0.0);
    float env = smoothstep(0.0, fx_k(5u), fx_age()) * (1.0 - smoothstep(t + fx_k(7u), end, fx_age()));
    vec2 o = orb_centre(aim, fx_age());
    float r = fx_k(0u) * c;
    float d = length(p - o);
    float cov = clamp(0.5 - (d - r), 0.0, 1.0);
    float k = min(d / r, 1.0);
    float hs = length(p - (o - 0.35 * r)) / (0.30 * r);
    float body = (0.45 + 0.30 * (1.0 - k * k) + 0.25 * exp(-hs * hs)) * cov;
    float hd = max(d - r, 0.0) / (fx_k(1u) * c);
    float halo = fx_k(2u) * exp(-hd * hd);
    vec2 q = orb_centre(aim, max(fx_age() - fx_k(4u), 0.0));
    vec2 ba = q - o;
    vec2 pa = p - o;
    float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    float ds = length(pa - ba * h);
    float cd = ds / (r * (0.8 - 0.6 * h));
    float comet = fx_k(3u) * (1.0 - h) * (1.0 - h) * exp(-cd * cd);
    float a = color.a * clamp(max(max(body, halo), comet) * env, 0.0, 1.0);
    return vec4(color.rgb * a, a);
}

void main() {
    if (in_kind == KIND_PARTICLE) {
        out_color = fx_particle();
        return;
    }
    out_color = orb_light(fx_place(), in_color);
}
