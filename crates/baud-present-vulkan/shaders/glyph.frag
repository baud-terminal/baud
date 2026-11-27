#version 450
// `glyph_fragment`: a mask's coverage bent by its contrast curve, a colour
// glyph's texel, or a flat colour; premultiplied out.

layout(location = 0) in vec2 in_uv;
layout(location = 1) in vec4 in_color;
layout(location = 2) flat in uint in_kind;
layout(location = 3) flat in uint in_curve;

layout(set = 1, binding = 0) uniform sampler2D mask_atlas;
layout(set = 1, binding = 1) uniform sampler2D color_atlas;
// The contrast curves, 256 bytes a row, read as words.
layout(std430, set = 1, binding = 2) readonly buffer Curves {
    uint curve_words[];
};

layout(location = 0) out vec4 out_color;

uint curve_at(uint k) {
    return (curve_words[k >> 2] >> ((k & 3u) * 8u)) & 0xffu;
}

// `alpha_byte`: the source alpha of coverage `cov` in a colour of alpha
// `color_a`, rounded to a byte as the CPU's `premultiply_chan` rounds it.
float alpha_byte(float color_a, uint cov) {
    if (color_a >= 1.0) {
        return float(cov) / 255.0;
    }
    uint ca = uint(color_a * 255.0 + 0.5);
    return float((ca * cov + 127u) / 255u) / 255.0;
}

void main() {
    if (in_kind == 0u) {
        uint cov = uint(texture(mask_atlas, in_uv).r * 255.0 + 0.5);
        float a = alpha_byte(in_color.a, curve_at(in_curve * 256u + cov));
        out_color = vec4(in_color.rgb * a, a);
        return;
    }
    if (in_kind == 1u) {
        vec4 c = texture(color_atlas, in_uv);
        out_color = vec4(c.rgb * c.a, c.a);
        return;
    }
    out_color = vec4(in_color.rgb * in_color.a, in_color.a);
}
