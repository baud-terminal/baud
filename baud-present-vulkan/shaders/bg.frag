#version 450
#extension GL_GOOGLE_include_directive : require
// `bg_fragment`: each cell's background, one texel a cell, read at the
// cell the pixel is in; clear outside the grid. Straight in the texture,
// premultiplied out.
#include "common.glsl"

layout(set = 1, binding = 3) uniform sampler2D bg;

layout(location = 0) out vec4 out_color;

void main() {
    vec2 cf = (gl_FragCoord.xy - u.pad) / u.cell;
    if (cf.x >= u.grid.x || cf.y >= u.grid.y) {
        out_color = vec4(0.0);
        return;
    }
    vec4 c = texelFetch(bg, ivec2(cf), 0);
    out_color = vec4(c.rgb * c.a, c.a);
}
