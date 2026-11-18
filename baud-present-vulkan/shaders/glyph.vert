#version 450
#extension GL_GOOGLE_include_directive : require
// `glyph_vertex`: each record a four-vertex strip over its rectangle, its
// atlas uv, its colour straight, its kind and its curve row.
#include "common.glsl"

layout(std430, set = 0, binding = 0) readonly buffer Insts {
    Inst insts[];
};

layout(location = 0) out vec2 out_uv;
layout(location = 1) out vec4 out_color;
layout(location = 2) flat out uint out_kind;
layout(location = 3) flat out uint out_curve;

void main() {
    Inst i = insts[gl_InstanceIndex];
    uint kind = i.kind & 0xffffu;
    vec2 c = vec2(float(gl_VertexIndex & 1), float(gl_VertexIndex >> 1));
    // `xy` is two signed halves: sign-extended from the low and high 16.
    vec2 xy = vec2(float(int(i.xy << 16) >> 16), float(int(i.xy) >> 16));
    vec2 wh = vec2(float(i.wh & 0xffffu), float(i.wh >> 16));
    vec2 uv = vec2(float(i.uv & 0xffffu), float(i.uv >> 16));
    vec2 n = (xy + c * wh) / u.viewport;
    // Vulkan's clip space has y down: no flip, where Metal's is `1 - 2y`.
    gl_Position = vec4(n.x * 2.0 - 1.0, n.y * 2.0 - 1.0, 0.0, 1.0);
    vec2 side = kind == 1u ? u.color : u.mask;
    out_uv = (uv + c * wh) / side;
    // A particle is a shape over its record: the effect's fragment stage
    // reads where in it a pixel is, -1 to 1 on each axis.
    if (kind == KIND_PARTICLE) {
        out_uv = c * 2.0 - 1.0;
    }
    uint col = i.color;
    out_color = vec4(float((col >> 16) & 0xffu), float((col >> 8) & 0xffu),
                     float(col & 0xffu), float(col >> 24)) / 255.0;
    out_kind = kind;
    out_curve = i.kind >> 16;
}
