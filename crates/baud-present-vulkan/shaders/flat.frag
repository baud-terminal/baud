#version 450
// `flat_fragment`: the colour the vertex stage gave, premultiplied.

layout(location = 0) in vec4 in_color;

layout(location = 0) out vec4 out_color;

void main() {
    out_color = in_color;
}
