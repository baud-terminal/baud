#version 450
#extension GL_GOOGLE_include_directive : require

// ---- The fireworks, `crates/baud-render/src/fireworks.rs`: a five-pointed star a
// record, in its colour at its opacity, turned by its `u` (1/65536 of a
// turn a step) and its image `v` sixteenths of a pixel a side. The vertex
// stage carries the record's middle pixel, its turn and its image size.

#include "common.glsl"

layout(std430, set = 0, binding = 0) readonly buffer Insts {
    Inst insts[];
};

layout(location = 0) flat out vec4 out_color;
layout(location = 1) flat out vec2 out_mid;
layout(location = 2) flat out vec2 out_turn;
layout(location = 3) flat out float out_image;

void main() {
    Inst i = insts[gl_InstanceIndex];
    vec2 c = vec2(float(gl_VertexIndex & 1), float(gl_VertexIndex >> 1));
    vec2 xy = vec2(float(int(i.xy << 16) >> 16), float(int(i.xy) >> 16));
    uint w = i.wh & 0xffffu;
    vec2 wh = vec2(float(w), float(i.wh >> 16));
    vec2 n = (xy + c * wh) / u.viewport;
    // Vulkan's clip space has y down: no flip.
    gl_Position = vec4(n.x * 2.0 - 1.0, n.y * 2.0 - 1.0, 0.0, 1.0);
    uint col = i.color;
    out_color = vec4(float((col >> 16) & 0xffu), float((col >> 8) & 0xffu),
                     float(col & 0xffu), float(col >> 24)) / 255.0;
    // The centre of the record's middle pixel, `fireworks::centre`.
    float half_side = float(w / 2u);
    out_mid = xy + half_side + 0.5;
    precise float th = float(i.uv & 0xffffu) * (6.28318530717958647692 / 65536.0);
    precise float cs = cos(th);
    precise float sn = sin(th);
    out_turn = vec2(cs, sn);
    out_image = float(i.uv >> 16) / 16.0;
}
