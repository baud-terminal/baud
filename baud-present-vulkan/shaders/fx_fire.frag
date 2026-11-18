#version 450
#extension GL_GOOGLE_include_directive : require

// ---- The fire, `baud-render/src/fire.rs` (`fire::light`): a flame rising
// off the quad's collapsing corners, and the embers it sheds. Its own
// numbers, `fire::params`: `k[0]` the flame's height, `k[1]` its rise,
// `k[2]` its drift, and `k[3]` how far its path has ridden a smooth scroll,
// which the noise is anchored on so a flame that rides keeps its shape.

#include "fx_common.glsl"

layout(location = 0) out vec4 out_color;

// **The flame at `p`**: tongues where noise lifts a sample down into the
// quad, cooling with distance from it and burning out from the tail, and a
// halo on the page -- the cursor's colour at the heat's opacity,
// premultiplied.
vec4 fire_flame(vec2 p, vec4 color) {
    if (fx_in_hole(p)) return vec4(0.0);
    float height = fx_k(0u);
    float rise = fx_k(1u);
    float drift = fx_k(2u);
    float k = fx_age() / fx_life();
    if (k >= 1.0) return vec4(0.0);
    float c = fx_unit_len();
    float d0 = fx_distance(p) / c;
    if (d0 > fx_reach()) return vec4(0.0);
    vec2 q = (p - vec2(0.0, fx_k(3u))) / c;
    vec2 s = vec2(q.x * 2.6, (q.y + fx_age() * rise) * 0.7) - fx_dir() * (fx_age() * drift);
    float n = fx_fbm(s, fx_seed());
    float lift = height * c * smoothstep(0.25, 0.85, n);
    float d = fx_distance(p + vec2(0.0, lift)) / c;
    float body = max(1.0 - smoothstep(-0.10, 0.20, d),
                     0.8 * (1.0 - smoothstep(-0.25, 0.05, d0)));
    float cool = clamp(max(d0, 0.0) / height, 0.0, 1.0);
    float u = clamp(dot(p - fx_tail(), fx_dir()) / max(fx_len(), 1.0), 0.0, 1.0);
    float burn = smoothstep(k - 0.15, k + 0.35, u) * (1.0 - k * k);
    float heat = clamp(body * (1.0 - 0.85 * cool) * (0.50 + 0.60 * n) * burn, 0.0, 1.0);
    float halo = 0.12 * exp(-2.5 * max(d0, 0.0)) * burn;
    float a = color.a * clamp(smoothstep(0.05, 0.95, heat) + halo, 0.0, 1.0);
    return vec4(color.rgb * a, a);
}

void main() {
    if (in_kind == KIND_PARTICLE) {
        out_color = fx_particle();
        return;
    }
    out_color = fire_flame(fx_place(), in_color);
}
