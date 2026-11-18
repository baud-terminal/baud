//! **The effect shaders against `baud-render`'s effects and uniform.**
//! Every effect has a fragment shader and its compiled SPIR-V, and each
//! declares `EffectU` where the instance path binds it, at the offsets
//! `Vfx::uniform` writes its floats -- read off the compiled module, not the
//! source, so what the driver is handed is what is checked.

#![cfg(target_os = "linux")]

use baud_render::effect::{Cap, Effect, Ignition, Seed, UNIFORM_FLOATS, Vfx};
use baud_render::{Corners, Pt, Rect, Shape, Smear, Strength};
use std::collections::HashMap;
use std::path::PathBuf;

/// The shaders' folder.
fn shaders() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders")
}

/// Each effect's fragment shaders by file stem: one each, and the plasma's
/// second and third for its `multiply` mix, by a second source and in two
/// passes.
fn stems() -> Vec<&'static str> {
    let mut stems: Vec<&str> = Effect::ALL
        .iter()
        .map(|e| match e {
            Effect::Fire => "fx_fire",
            Effect::Plasma => "fx_plasma",
            Effect::Lightning => "fx_lightning",
            Effect::Ice => "fx_ice",
            Effect::Ufo => "fx_ufo",
            Effect::Orb => "fx_orb",
            Effect::Liquid => "fx_liquid",
        })
        .collect();
    stems.push("fx_plasma_multiply");
    stems.push("fx_plasma_factor");
    stems
}

/// **What a module says about its `EffectU`**: the members' names and
/// offsets in order, and the set and binding of the variable of that type.
#[derive(Debug, PartialEq)]
struct Block {
    members: Vec<(String, u32)>,
    set: DescriptorSet,
    binding: Binding,
}

/// A `DescriptorSet` decoration's operand.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DescriptorSet(u32);

/// A `Binding` decoration's operand.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Binding(u32);

/// A SPIR-V string operand: UTF-8, nul-terminated, in little-endian words.
fn string(words: &[u32]) -> String {
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// **The `EffectU` block of a SPIR-V module**, read from its instructions
/// (`OpName` 5, `OpMemberName` 6, `OpDecorate` 71, `OpMemberDecorate` 72,
/// `OpTypePointer` 32, `OpVariable` 59; decorations `Binding` 33,
/// `DescriptorSet` 34, `Offset` 35), after the five-word header.
fn effect_block(spv: &[u8]) -> Option<Block> {
    let words: Vec<u32> = spv
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    assert_eq!(words.first(), Some(&0x0723_0203), "SPIR-V's magic number");
    let mut names: HashMap<u32, String> = HashMap::new();
    let mut member_names: HashMap<(u32, u32), String> = HashMap::new();
    let mut offsets: HashMap<(u32, u32), u32> = HashMap::new();
    let mut decorations: HashMap<(u32, u32), u32> = HashMap::new();
    let mut pointers: HashMap<u32, u32> = HashMap::new();
    let mut variables: Vec<(u32, u32)> = Vec::new();
    let mut at = 5;
    while at < words.len() {
        let count = usize::try_from(words[at] >> 16).ok()?;
        let op = words[at] & 0xffff;
        let ins = words.get(at..at + count)?;
        match op {
            5 => {
                names.insert(ins[1], string(&ins[2..]));
            }
            6 => {
                member_names.insert((ins[1], ins[2]), string(&ins[3..]));
            }
            71 if ins.len() >= 4 => {
                decorations.insert((ins[1], ins[2]), ins[3]);
            }
            72 if ins.len() >= 5 && ins[3] == 35 => {
                offsets.insert((ins[1], ins[2]), ins[4]);
            }
            32 => {
                pointers.insert(ins[1], ins[3]);
            }
            59 => variables.push((ins[1], ins[2])),
            _ => {}
        }
        at += count.max(1);
    }
    let block = *names.iter().find(|(_, n)| n.as_str() == "EffectU")?.0;
    let mut members: Vec<(u32, String, u32)> = member_names
        .iter()
        .filter(|((s, _), _)| *s == block)
        .map(|((_, m), n)| {
            (
                *m,
                n.clone(),
                offsets.get(&(block, *m)).copied().unwrap_or(u32::MAX),
            )
        })
        .collect();
    members.sort();
    let (_, var) = variables
        .iter()
        .find(|(ty, _)| pointers.get(ty) == Some(&block))?;
    Some(Block {
        members: members.into_iter().map(|(_, n, o)| (n, o)).collect(),
        set: DescriptorSet(*decorations.get(&(*var, 34))?),
        binding: Binding(*decorations.get(&(*var, 33))?),
    })
}

/// An effect lit on a quad, at `age`, for reading its uniform.
fn vfx(effect: Effect, age: f32) -> Vfx {
    let hole = Rect {
        left: 160.0,
        top: 46.0,
        right: 170.0,
        bottom: 69.0,
    };
    let corners = Corners([
        Pt { x: 20.0, y: 23.0 },
        Pt { x: 30.0, y: 23.0 },
        Pt { x: 170.0, y: 69.0 },
        Pt { x: 160.0, y: 69.0 },
    ]);
    let emitter = Smear {
        corners,
        hole,
        shape: Shape::Quad,
        strength: Strength::Full,
        opacity: Smear::OPAQUE,
    };
    Vfx {
        effect,
        emitter,
        ignition: Ignition::ignite(Pt { x: 25.0, y: 34.5 }, hole, Seed(5), 23.0).at(age),
        axis: Pt { x: 1.0, y: 0.0 },
    }
}

/// **Every effect has a fragment shader and its SPIR-V, and each puts
/// `EffectU` where `Vfx::uniform` puts its floats**: set 2, binding 0,
/// nine vec4s at 16-byte steps, 144 bytes; and the uniform's floats land in
/// the members the shader reads them from -- the origin at float 12, the
/// first of `origin_tail`, and the cap's bits at float 32, the first of
/// `cap_pad` -- so a reordered uniform or block fails here, not as wrong
/// pixels. Named break: a block of `vec2 c[4]` and `float k[8]`, which
/// std140 pads to 16 bytes an element.
#[test]
fn every_effect_shader_reads_the_uniform_where_rust_writes_it() {
    let want: Vec<(String, u32)> = [
        "c01",
        "c23",
        "hole",
        "origin_tail",
        "dir_len_unit",
        "age_life_seed_reach",
        "k0",
        "k1",
        "cap_pad",
    ]
    .iter()
    .zip((0u32..).step_by(16))
    .map(|(n, o)| (String::from(*n), o))
    .collect();
    assert_eq!(UNIFORM_FLOATS * 4, 144, "the uniform's size");
    let u = vfx(Effect::Fire, 0.05).uniform([7.0, 9.0]);
    assert_eq!((u[12], u[13]), (7.0, 9.0), "the origin is origin_tail.xy");
    // The plasma's repeat pass clamps, whose bits are 1, not the padding's 0.
    let mut repeat = vfx(Effect::Plasma, 0.05);
    repeat.emitter.strength = Strength::UnderThreshold;
    assert_eq!(repeat.cap(), Cap::Clamp);
    let u = repeat.uniform([0.0, 0.0]);
    assert_eq!(u[32].to_bits(), 1, "the cap is cap_pad.x");
    assert!(u[33..].iter().all(|p| p.to_bits() == 0), "then the padding");
    let mut missing = Vec::new();
    for stem in stems() {
        let frag = shaders().join(format!("{stem}.frag"));
        let spv = shaders().join("spv").join(format!("{stem}.frag.spv"));
        let (Ok(_), Ok(bytes)) = (std::fs::read(&frag), std::fs::read(&spv)) else {
            missing.push(stem);
            continue;
        };
        let block = effect_block(&bytes).unwrap_or_else(|| panic!("{stem}: no EffectU"));
        assert_eq!(
            block,
            Block {
                members: want.clone(),
                set: DescriptorSet(2),
                binding: Binding(0),
            },
            "{stem}"
        );
    }
    assert_eq!(missing, Vec::<&str>::new(), "effects without a shader");
}

/// The value of `const <type> name = <value>;` in a shader, its `u` suffix
/// taken off.
fn constant(stem: &str, name: &str) -> String {
    let src = std::fs::read_to_string(shaders().join(format!("{stem}.frag")))
        .unwrap_or_else(|e| panic!("{stem}.frag: {e}"));
    src.lines()
        .find_map(|l| {
            let l = l.trim();
            let rest = l
                .strip_prefix("const float ")
                .or_else(|| l.strip_prefix("const uint "))?;
            let (n, v) = rest.split_once(" = ")?;
            (n == name).then(|| v.trim_end_matches(';').trim_end_matches('u').to_string())
        })
        .unwrap_or_else(|| panic!("{stem}.frag defines {name}"))
}

fn float(stem: &str, name: &str) -> f32 {
    constant(stem, name).parse().expect(name)
}

fn word(stem: &str, name: &str) -> u32 {
    let v = constant(stem, name);
    match v.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16).expect(name),
        None => v.parse().expect(name),
    }
}

/// **The constants the shaders spell and `baud-render`'s models spell are
/// one fact each**: the bolt's jag, cap, segment floor, levels, salts and
/// forks; the ice's lead and salts; and the liquid's trail, wobble, light
/// and beads -- the numbers a shader carries itself rather than reading
/// from the uniform. The pixel comparison holds the shaders to the models
/// only in the scenes it draws; this names each constant. Named break: a
/// salt changed in one place.
#[test]
fn the_shaders_constants_are_the_models() {
    use baud_render::{ice, lightning as l, liquid};
    let bolt = "fx_lightning";
    assert_eq!(float(bolt, "BOLT_JAG"), l::JAG);
    assert_eq!(float(bolt, "BOLT_CAP"), l::CAP);
    assert_eq!(float(bolt, "BOLT_S_MIN"), l::S_MIN);
    assert_eq!(word(bolt, "BOLT_LEVELS"), l::LEVELS);
    assert_eq!(word(bolt, "BOLT_SALT"), l::BOLT_SALT);
    assert_eq!(word(bolt, "FORK_SALT"), l::FORK_SALT);
    assert_eq!(float(bolt, "FORK_MIN"), l::FORK_MIN);
    assert_eq!(float(bolt, "FORK_P"), l::FORK_P);
    assert_eq!(float("fx_ice", "ICE_LEAD"), ice::LEAD);
    assert_eq!(word("fx_ice", "ICE_SALT"), ice::ICE_SALT);
    assert_eq!(word("fx_ice", "RIME_SALT"), ice::RIME_SALT);
    let lq = "fx_liquid";
    assert_eq!(float(lq, "LQ_TRAIL"), liquid::TRAIL);
    assert_eq!(float(lq, "LQ_ZETA"), liquid::ZETA);
    assert_eq!(float(lq, "LQ_OMEGA"), liquid::OMEGA);
    assert_eq!(float(lq, "LQ_BACK"), liquid::BACK);
    assert_eq!(float(lq, "LQ_CROSS"), liquid::CROSS);
    assert_eq!(float(lq, "LQ_LEAN"), liquid::LEAN);
    assert_eq!(float(lq, "LQ_LEAN_AT"), liquid::LEAN_AT);
    assert_eq!(
        (float(lq, "LQ_REST_X"), float(lq, "LQ_REST_Y")),
        liquid::REST
    );
    assert_eq!(word(lq, "LQ_COUNT_SALT"), liquid::COUNT_SALT);
    assert_eq!(word(lq, "LQ_BEAD_SALT"), liquid::BEAD_SALT);
    assert_eq!(float(lq, "LQ_BEAD_IN"), liquid::BEAD_IN);
}
