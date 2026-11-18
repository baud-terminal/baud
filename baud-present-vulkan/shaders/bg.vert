#version 450
// `bg_vertex`: one strip over the whole target.
void main() {
    vec2 c = vec2(float(gl_VertexIndex & 1), float(gl_VertexIndex >> 1));
    gl_Position = vec4(c * 2.0 - 1.0, 0.0, 1.0);
}
