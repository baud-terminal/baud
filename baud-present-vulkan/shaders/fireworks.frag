#version 450

// ---- The fireworks' stars (`fireworks::coverage`, `fireworks::hole`): the
// star's coverage at the pixel, the hole round the cursor cut from it, and
// the coverage formed as the byte the CPU's `Target::fireworks` takes.
// `FwU` is `fireworks::uniform`: where the frame sits in the target, the
// hole's centre and its radius.

layout(location = 0) flat in vec4 in_color;
layout(location = 1) flat in vec2 in_mid;
layout(location = 2) flat in vec2 in_turn;
layout(location = 3) flat in float in_image;

layout(std140, set = 2, binding = 0) uniform FwU {
    vec4 origin_hole;
    vec4 radius_pad;
} f;

layout(location = 0) out vec4 out_color;

const float FW_TIP = 0.4935;
const float FW_NOTCH = 0.205;
const float FW_DROP = 0.04;

// `baud_domain::premultiply_chan`, `(c a + 127) / 255`, as the CPU forms it.
float alpha_byte(float color_a, uint cov) {
    if (color_a >= 1.0) {
        return float(cov) / 255.0;
    }
    uint ca = uint(color_a * 255.0 + 0.5);
    return float((ca * cov + 127u) / 255u) / 255.0;
}

// A coverage in [0, 1] as the byte the CPU takes, `(c * 255).round()`.
uint coverage_byte(float c) {
    return uint(floor(c * 255.0 + 0.5));
}

// `fireworks::star_distance`: the signed distance to the star, y up, a tip
// at (0, tip), folded across its mirror lines at 18 and 54 degrees.
float fw_star(vec2 p, float tip, float notch) {
    vec2 q = vec2(abs(p.x), p.y);
    const vec2 m18 = vec2(0.9510565, 0.309017);
    const vec2 m54 = vec2(0.58778524, 0.809017);
    float d = -m18.y * q.x + m18.x * q.y;
    if (d < 0.0) q = vec2(q.x + 2.0 * d * m18.y, q.y - 2.0 * d * m18.x);
    d = -m54.y * q.x + m54.x * q.y;
    if (d < 0.0) q = vec2(q.x + 2.0 * d * m54.y, q.y - 2.0 * d * m54.x);
    q.x = abs(q.x);
    float ex = notch * m54.x;
    float ey = notch * m54.y - tip;
    float px = q.x;
    float py = q.y - tip;
    float h = clamp((px * ex + py * ey) / (ex * ex + ey * ey), 0.0, 1.0);
    float dx = px - ex * h;
    float dy = py - ey * h;
    float dist = sqrt(dx * dx + dy * dy);
    return (ex * py - ey * px < 0.0) ? -dist : dist;
}

void main() {
    vec2 p = gl_FragCoord.xy - f.origin_hole.xy;
    vec2 q = p - in_mid;
    float c = in_turn.x;
    float s = in_turn.y;
    float x = q.x * c - q.y * s;
    float y = q.x * s + q.y * c - FW_DROP * in_image;
    float d = fw_star(vec2(x, -y), FW_TIP * in_image, FW_NOTCH * in_image);
    float star = clamp(0.5 - d, 0.0, 1.0);
    vec2 h = p - f.origin_hole.zw;
    float hole = clamp(sqrt(h.x * h.x + h.y * h.y) - f.radius_pad.x + 0.5, 0.0, 1.0);
    float a = alpha_byte(in_color.a, coverage_byte(star * hole));
    out_color = vec4(in_color.rgb * a, a);
}
