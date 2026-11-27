#version 450

// ---- The cursor trail's smear: `trail::coverage` in `baud-render`, the
// same rule, and the same function as `quad_fragment` in
// `crates/baud-present-macos/src/shaders.metal`, by the same names. The record is
// the smear's bounds, drawn by `glyph.vert`; this covers the pixels inside
// the corners by the non-zero winding rule, a pixel of anti-aliasing across
// each edge, and leaves the cursor's own rectangle bare. `mode` 0 is the
// quad, 1 the sweep, whose tail is the rectangle the corners hold and whose
// head is the hole. `origin` is where the frame sits in the target, so a
// pixel's centre in the frame is its position less it.

layout(location = 0) in vec2 in_uv;
layout(location = 1) in vec4 in_color;
layout(location = 2) flat in uint in_kind;
layout(location = 3) flat in uint in_curve;

// **`Smear::uniform`, byte for byte**: 16 floats, the four corners, the
// hole, the origin, the mode and a word of padding, spelled in vec4s as
// std140 lays them.
layout(std140, set = 2, binding = 0) uniform Quad {
    vec4 c01;
    vec4 c23;
    vec4 hole;
    vec4 origin_mode;
} q;

layout(location = 0) out vec4 out_color;

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

vec2 corner(uint i) {
    vec4 two = i < 2u ? q.c01 : q.c23;
    return (i & 1u) == 0u ? two.xy : two.zw;
}

void main() {
    vec2 p = gl_FragCoord.xy - q.origin_mode.xy;
    if (p.x >= q.hole.x && p.x < q.hole.z && p.y >= q.hole.y && p.y < q.hole.w) {
        out_color = vec4(0.0);
        return;
    }
    if (q.origin_mode.z > 0.5) {
        // The sweep: the box swept from the tail's centre to the cursor's,
        // at `128 t` of 255 -- `trail::coverage`'s sweep.
        vec2 c0 = (corner(3u) + corner(1u)) * 0.5;
        vec2 r = (q.hole.zw - q.hole.xy) * 0.5;
        vec2 ab = q.hole.xy + r - c0;
        float l2 = dot(ab, ab);
        if (l2 < 0.001) {
            out_color = vec4(0.0);
            return;
        }
        float t = clamp(dot(p - c0, ab) / l2, 0.0, 1.0);
        vec2 off = abs(p - c0 - t * ab);
        if (off.x > r.x || off.y > r.y) {
            out_color = vec4(0.0);
            return;
        }
        float a = alpha_byte(in_color.a, coverage_byte(128.0 * t / 255.0));
        out_color = vec4(in_color.rgb * a, a);
        return;
    }
    float d = 1.0 / 0.0;
    int winding = 0;
    for (uint i = 0u; i < 4u; i++) {
        vec2 a = corner(i);
        vec2 b = corner((i + 1u) & 3u);
        vec2 ba = b - a;
        vec2 pa = p - a;
        float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
        d = min(d, length(pa - ba * h));
        float side = ba.x * pa.y - ba.y * pa.x;
        if (a.y <= p.y) {
            if (b.y > p.y && side > 0.0) winding += 1;
        } else if (b.y <= p.y && side < 0.0) {
            winding -= 1;
        }
    }
    float s = winding != 0 ? -d : d;
    float a = alpha_byte(in_color.a, coverage_byte(clamp(0.5 - s, 0.0, 1.0)));
    out_color = vec4(in_color.rgb * a, a);
}
