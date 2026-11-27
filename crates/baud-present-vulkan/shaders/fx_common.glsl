// The effect trails' shared part, `crates/baud-render/src/effect.rs`: the
// records' varyings, the uniform every effect reads, and the hash, noise
// and quad distance each effect's fragment builds on. A line-for-line port
// of the same functions in `crates/baud-present-macos/src/shaders.metal`, the same
// names, so one model reads in both.

// The instance path's varyings (`design-c2`): the record's uv, its colour
// straight, its kind and its contrast curve's row.
layout(location = 0) in vec2 in_uv;
layout(location = 1) in vec4 in_color;
layout(location = 2) flat in uint in_kind;
layout(location = 3) flat in uint in_curve;

const float PI = 3.14159265358979323846;

// An effect trail's two record kinds, `Kind::Band` and `Kind::Particle`.
const uint KIND_BAND = 4u;
const uint KIND_PARTICLE = 5u;

// **`EffectU`, byte for byte `Vfx::uniform`**: 36 floats, 144 bytes. std140
// would pad a `vec2[4]` or a `float[8]` to 16 bytes an element, so the
// block is spelled in vec4s and the accessors below name the parts:
// `c01`/`c23` the emitter's corners, `hole`, `origin` and `tail`, `dir`,
// `len` and `unit`, `age`, `life`, the seed's bits and `reach`, `k` the
// effect's own numbers, and `cap` how a band's light meets its record's
// alpha, `effect::Cap` (0 scaled by it, 1 clamped at it).
layout(std140, set = 2, binding = 0) uniform EffectU {
    vec4 c01;
    vec4 c23;
    vec4 hole;
    vec4 origin_tail;
    vec4 dir_len_unit;
    vec4 age_life_seed_reach;
    vec4 k0;
    vec4 k1;
    uvec4 cap_pad;
} f;

vec2 fx_corner(uint i) {
    vec4 two = i < 2u ? f.c01 : f.c23;
    return (i & 1u) == 0u ? two.xy : two.zw;
}

float fx_k(uint i) {
    vec4 four = i < 4u ? f.k0 : f.k1;
    return four[i & 3u];
}

vec2 fx_origin() { return f.origin_tail.xy; }
vec2 fx_tail() { return f.origin_tail.zw; }
vec2 fx_dir() { return f.dir_len_unit.xy; }
float fx_len() { return f.dir_len_unit.z; }
float fx_unit_len() { return f.dir_len_unit.w; }
float fx_age() { return f.age_life_seed_reach.x; }
float fx_life() { return f.age_life_seed_reach.y; }
uint fx_seed() { return floatBitsToUint(f.age_life_seed_reach.z); }
float fx_reach() { return f.age_life_seed_reach.w; }
uint fx_cap() { return f.cap_pad.x; }

// The pixel's place in the frame: the fragment's centre, top-left origin,
// less where the frame sits in the target.
vec2 fx_place() { return gl_FragCoord.xy - fx_origin(); }

// PCG's output permutation over one step of its LCG, the GPU hash Jarzynski
// and Olano recommend; `effect::Hash` is the same function.
uint fx_hash(uint v) {
    uint s = v * 747796405u + 2891336453u;
    uint w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

// A number in [0, 1) from the hash's top 23 bits, through a float's mantissa:
// exact, with no conversion instruction.
float fx_unit(uint h) {
    return uintBitsToFloat((h >> 9u) | 0x3f800000u) - 1.0;
}

float fx_lattice(ivec2 i, uint seed) {
    return fx_unit(fx_hash(uint(i.x) + fx_hash(uint(i.y) + seed)));
}

// Value noise on the hash's lattice, with the quintic fade.
float fx_noise(vec2 p, uint seed) {
    vec2 fl = floor(p);
    ivec2 i = ivec2(fl);
    vec2 x = p - fl;
    vec2 w = x * x * x * (x * (x * 6.0 - 15.0) + 10.0);
    float a = fx_lattice(i, seed);
    float b = fx_lattice(i + ivec2(1, 0), seed);
    float c = fx_lattice(i + ivec2(0, 1), seed);
    float d = fx_lattice(i + ivec2(1, 1), seed);
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}

// Four octaves, each rotated and at a detuned double of the last's scale.
float fx_fbm(vec2 p, uint seed) {
    float t = 0.0;
    float a = 0.5;
    for (uint o = 0u; o < 4u; o++) {
        t += a * fx_noise(p, seed + o * 0x9E3779B9u);
        p = vec2(0.8 * p.x - 0.6 * p.y, 0.6 * p.x + 0.8 * p.y) * 2.02;
        a *= 0.5;
    }
    return t;
}

// Positive infinity, which a distance starts from.
float fx_inf() { return uintBitsToFloat(0x7f800000u); }

// The distance from `p` to the quad, negative inside it by the non-zero
// winding rule the quad is filled by.
float fx_distance(vec2 p) {
    float d = fx_inf();
    int winding = 0;
    for (uint i = 0u; i < 4u; i++) {
        vec2 a = fx_corner(i);
        vec2 b = fx_corner((i + 1u) & 3u);
        vec2 ba = b - a;
        vec2 pa = p - a;
        float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
        d = min(d, length(pa - ba * h));
        float side = ba.x * pa.y - ba.y * pa.x;
        if (a.y <= p.y) {
            if (b.y > p.y && side > 0.0) winding += 1;
        } else if (b.y <= p.y && side < 0.0) {
            winding -= 1;
        }
    }
    return winding != 0 ? -d : d;
}

// Whether `p` is in the cursor's own rectangle, which every effect leaves
// bare, as the quad does.
bool fx_in_hole(vec2 p) {
    return p.x >= f.hole.x && p.x < f.hole.z && p.y >= f.hole.y && p.y < f.hole.w;
}

// **A particle** -- an ember, an ion: a soft disc filling its record, the
// cursor's colour at the particle's glow, which the record's alpha carries.
vec4 fx_particle() {
    float a = 1.0 - smoothstep(0.15, 1.0, length(in_uv));
    a = in_color.a * a * a;
    return vec4(in_color.rgb * a, a);
}
