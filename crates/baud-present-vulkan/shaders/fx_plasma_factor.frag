#version 450
#extension GL_GOOGLE_include_directive : require

// **The plasma's `multiply` factor alone**, `1 - as + cs` a channel: the
// first of the two passes a device without a second source blends the
// `multiply` mix in, which takes the backdrop's colour times this
// (`ZERO, SRC_COLOR`) and keeps its alpha; the second adds the plasma's
// colour times `1 - ab` (`fx_plasma.frag`, `ONE_MINUS_DST_ALPHA, ONE`).

#include "fx_common.glsl"
#include "fx_plasma.glsl"

layout(location = 0) out vec4 out_color;

void main() {
    vec4 c = plasma_color();
    out_color = vec4(1.0 - c.a + c.rgb, 1.0);
}
