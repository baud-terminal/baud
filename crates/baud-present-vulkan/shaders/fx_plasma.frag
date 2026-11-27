#version 450
#extension GL_GOOGLE_include_directive : require

// The plasma by `normal` and by `screen` (`composite::Mix`): its
// premultiplied colour, which the blend state meets with the backdrop --
// `One, OneMinusSourceAlpha` or `One, OneMinusSourceColor`.

#include "fx_common.glsl"
#include "fx_plasma.glsl"

layout(location = 0) out vec4 out_color;

void main() { out_color = plasma_color(); }
