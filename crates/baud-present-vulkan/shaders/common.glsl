// The instance path's shared part (`design-c2`): the record, the uniform,
// and the kinds. A port of the same declarations in
// `crates/baud-present-macos/src/shaders.metal`, the same names.

// `Instance::to_bytes`: 20 bytes, read as five words. `xy` two i16, `wh`
// and `uv` two u16 each, the colour straight `0xAARRGGBB`, the kind in the
// low half of the last word and a mask's contrast curve row in the high.
struct Inst {
    uint xy;
    uint wh;
    uint uv;
    uint color;
    uint kind;
};

// `U`, byte for byte the Metal path's: viewport, cell, grid, the mask and
// colour atlases' sizes, and the padding the grid sits inside, each a
// float pair. std140 lays a vec2 on 8 bytes, so six vec2 are 48 bytes as
// in Metal.
layout(std140, set = 0, binding = 1) uniform U {
    vec2 viewport;
    vec2 cell;
    vec2 grid;
    vec2 mask;
    vec2 color;
    vec2 pad;
} u;

// An effect trail's two record kinds, `Kind::Band` and `Kind::Particle`.
const uint KIND_BAND = 4u;
const uint KIND_PARTICLE = 5u;
