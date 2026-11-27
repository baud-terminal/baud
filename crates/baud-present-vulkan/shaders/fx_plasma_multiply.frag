#version 450
#extension GL_GOOGLE_include_directive : require

// **The plasma by `multiply`** (`composite::Mix`): the plasma's
// premultiplied colour, and as the second source the factor its backdrop is
// taken at, `1 - as + cs` a channel, so the blend state's
// `OneMinusDstAlpha, Src1Color` forms W3C Compositing's
// `cs (1 - ab) + cb (1 - as + cs)` over the glass and an opaque page alike.

#include "fx_common.glsl"
#include "fx_plasma.glsl"

layout(location = 0, index = 0) out vec4 out_color;
layout(location = 0, index = 1) out vec4 out_factor;

void main() {
    vec4 c = plasma_color();
    out_color = c;
    out_factor = vec4(1.0 - c.a + c.rgb, 1.0);
}
