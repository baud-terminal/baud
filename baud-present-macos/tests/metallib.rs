//! The committed `baud.metallib` must be the shaders beside it.
//!
//! `scripts/build-metallib.sh` compiles `src/shaders.metal` and records
//! two FNV-1a digests: the source's in `src/shaders.metal.fnv` and the
//! library's own in `src/baud.metallib.fnv`. Editing a shader
//! without re-running it would leave the app loading a library that
//! draws the *old* shaders -- and nothing else would notice, because the
//! library loads perfectly and the pixels are merely wrong. That is this
//! repo's "a gate that cannot fail" shape, so here is the gate.
//!
//! **A developer's machine and not CI**, like every test in this crate:
//! the workflows are `ubuntu-latest` and there is no macOS runner, so
//! these go red where the shaders are edited and nowhere else.
#![cfg(target_os = "macos")]

/// FNV-1a, the digest this repo already uses for its golden frames, and
/// the same eight bytes `scripts/build-metallib.sh` writes.
fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[test]
fn the_library_was_built_from_the_shaders_beside_it() {
    // Two digests, and the second is the one that ties the artefact to
    // the generator run. The source's alone relates the shaders to
    // themselves: a `baud.metallib` swapped for one built from different
    // text with the same entry points would pass every other gate here.
    let src = include_str!("../src/shaders.metal");
    let recorded = include_str!("../src/shaders.metal.fnv").trim();
    assert_eq!(
        format!("{:016x}", fnv(src.as_bytes())),
        recorded,
        "src/shaders.metal has changed since baud.metallib was built \
         -- run scripts/build-metallib.sh"
    );

    let lib = include_bytes!("../src/baud.metallib");
    let lib_recorded = include_str!("../src/baud.metallib.fnv").trim();
    assert_eq!(
        format!("{:016x}", fnv(lib)),
        lib_recorded,
        "src/baud.metallib is not the library scripts/build-metallib.sh \
         last wrote -- run it"
    );
}

#[test]
fn the_library_is_a_library_and_not_an_empty_file() {
    // The positive control: every way the generator can fail quietly ends
    // in a short or absent file, and a zero-byte `include_bytes!` compiles.
    let lib = include_bytes!("../src/baud.metallib");
    assert!(
        lib.len() > 4096,
        "baud.metallib is {} bytes, which is not a compiled library",
        lib.len()
    );
    assert_eq!(&lib[..4], b"MTLB", "baud.metallib does not start with MTLB");
}

/// The two libraries expose the same entry points, checked in one process
/// against one Metal runtime.
///
/// **Why this is the gate and a pixel comparison is not.** The library and
/// the digest above are both derived by `scripts/build-metallib.sh` from
/// the one `shaders.metal`, so they cannot describe different text; and a
/// `.metallib` whose AIR the runtime will not accept -- a toolchain newer
/// than the Metal it lands on -- is a *load failure*, which `Gpu::new`
/// answers by compiling the source, rather than a library that draws
/// something else. What is left between the two is that the shipped
/// library might not offer a function the source does, which would make
/// `Gpu::new` return `None` and the app fall to the CPU renderer without
/// anything saying why. That is what this checks.
#[test]
fn the_shipped_library_offers_what_the_source_does() {
    use objc2_foundation::NSString;
    use objc2_metal::{MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary};

    let Some(device) = MTLCreateSystemDefaultDevice() else {
        panic!("a Metal device; this test cannot skip itself");
    };
    let src = include_str!("../src/shaders.metal");
    let from_source = device
        .newLibraryWithSource_options_error(&NSString::from_str(src), None)
        .expect("the shaders compile");
    let data = dispatch2::DispatchData::from_static_bytes(include_bytes!("../src/baud.metallib"));
    let from_data = device
        .newLibraryWithData_error(&data)
        .expect("baud.metallib loads on the Metal that built this test");

    let names = |l: &objc2::runtime::ProtocolObject<dyn MTLLibrary>| {
        let mut v: Vec<String> = l.functionNames().iter().map(|n| n.to_string()).collect();
        v.sort();
        v
    };
    let (a, b) = (names(&from_source), names(&from_data));
    assert!(
        a.len() >= 8,
        "the source offers {} entry points, which is fewer than the shaders declare",
        a.len()
    );
    assert_eq!(
        a, b,
        "baud.metallib and src/shaders.metal do not offer the same entry points \
         -- run scripts/build-metallib.sh"
    );
}

/// **`SHAPE_SLOTS` in the shader and `SHAPES_PER_CELL` in `metal.rs` are
/// one fact spelled twice.** The vertex stage turns an instance id into a
/// cell and a slot by dividing by the first; the draw call sizes the
/// instance count by multiplying by the second. Unequal, every slot past
/// the smaller is never drawn -- which is how the colon underline styles
/// nearly shipped drawn on one GPU arm and not the other: the shader grew
/// sixteen segment slots and the Rust constant sat at four until the draw
/// call was read. The pixel oracle catches the pixels; this catches the
/// number before a `.metallib` is built.
#[test]
fn the_shape_slot_count_matches_the_shader() {
    let src = include_str!("../src/shaders.metal");
    let prefix = "#define SHAPE_SLOTS ";
    let line = src
        .lines()
        .find(|l| l.starts_with(prefix))
        .expect("shaders.metal defines SHAPE_SLOTS");
    let slots: usize = line[prefix.len()..]
        .trim()
        .trim_end_matches('u')
        .parse()
        .expect("SHAPE_SLOTS is an unsigned literal");
    assert_eq!(
        slots,
        baud_present_macos::metal::SHAPES_PER_CELL,
        "the shader divides by {line:?} and the draw call multiplies by \
         SHAPES_PER_CELL; they must be the same number"
    );
}

/// **The lightning's shape in the shader and in `baud_render::lightning` are
/// one fact spelled twice.** The bolt's jag, cap, segment floor, level count,
/// salts and fork constants are `#define`s the shader reads and constants the
/// Rust model reads -- the model the oracle's reference calls, so a shader
/// that drifted from it would be caught there too, but only in the scenes it
/// draws; this names the constant.
#[test]
fn the_bolts_shape_matches_the_shader() {
    use baud_render::lightning as l;
    let src = include_str!("../src/shaders.metal");
    let define = |name: &str| -> String {
        let prefix = format!("#define {name} ");
        let line = src
            .lines()
            .find(|x| x.starts_with(&prefix))
            .unwrap_or_else(|| panic!("shaders.metal defines {name}"));
        line[prefix.len()..]
            .trim()
            .trim_end_matches('u')
            .to_string()
    };
    let float = |name: &str| -> f32 { define(name).parse().expect(name) };
    let word = |name: &str| -> u32 {
        let v = define(name);
        u32::from_str_radix(v.trim_start_matches("0x"), 16).expect(name)
    };
    assert_eq!(float("BOLT_JAG"), l::JAG);
    assert_eq!(float("BOLT_CAP"), l::CAP);
    assert_eq!(float("BOLT_S_MIN"), l::S_MIN);
    assert_eq!(define("BOLT_LEVELS").parse::<u32>().unwrap(), l::LEVELS);
    assert_eq!(word("BOLT_SALT"), l::BOLT_SALT);
    assert_eq!(word("FORK_SALT"), l::FORK_SALT);
    assert_eq!(float("FORK_MIN"), l::FORK_MIN);
    assert_eq!(float("FORK_P"), l::FORK_P);
}

/// **The ice's lead and salts in the shader and in `baud_render::ice` are one
/// fact spelled twice**: `ICE_LEAD` is kitty's leading-corner constant the
/// frost is laid at, and the salts part its hashes from the fire's on one
/// seed. The oracle ties the shader to the Rust model, but only in the scenes
/// it draws; this names the constant.
#[test]
fn the_ices_constants_match_the_shader() {
    use baud_render::ice;
    let src = include_str!("../src/shaders.metal");
    let define = |name: &str| -> String {
        let prefix = format!("#define {name} ");
        let line = src
            .lines()
            .find(|x| x.starts_with(&prefix))
            .unwrap_or_else(|| panic!("shaders.metal defines {name}"));
        line[prefix.len()..]
            .trim()
            .trim_end_matches('u')
            .to_string()
    };
    let word = |name: &str| -> u32 {
        u32::from_str_radix(define(name).trim_start_matches("0x"), 16).expect(name)
    };
    assert_eq!(define("ICE_LEAD").parse::<f32>().unwrap(), ice::LEAD);
    assert_eq!(word("ICE_SALT"), ice::ICE_SALT);
    assert_eq!(word("RIME_SALT"), ice::RIME_SALT);
}

/// **The liquid's constants in the shader and in `baud_render::liquid` are
/// one fact spelled twice**: the shader derives the gel's state from the
/// uniform's prefix, so kitty's trailing corners' halving time, the wobble's
/// damping, frequency and cross term, the counter-glint's opacity, the
/// glint's rest and lean, and the beads' salts and growth are `#define`s
/// there. The oracle ties the shader to the Rust model only in the
/// scenes it draws -- one seed's bead count among them -- and this names the
/// constants.
#[test]
fn the_liquids_constants_match_the_shader() {
    use baud_render::liquid;
    let src = include_str!("../src/shaders.metal");
    let define = |name: &str| -> String {
        let prefix = format!("#define {name} ");
        let line = src
            .lines()
            .find(|x| x.starts_with(&prefix))
            .unwrap_or_else(|| panic!("shaders.metal defines {name}"));
        line[prefix.len()..]
            .trim()
            .trim_end_matches('u')
            .to_string()
    };
    let float = |name: &str| -> f32 { define(name).parse().expect(name) };
    let word = |name: &str| -> u32 {
        u32::from_str_radix(define(name).trim_start_matches("0x"), 16).expect(name)
    };
    assert_eq!(float("LQ_TRAIL"), liquid::TRAIL);
    assert_eq!(float("LQ_ZETA"), liquid::ZETA);
    assert_eq!(float("LQ_OMEGA"), liquid::OMEGA);
    assert_eq!(float("LQ_BACK"), liquid::BACK);
    assert_eq!(float("LQ_CROSS"), liquid::CROSS);
    assert_eq!(float("LQ_LEAN"), liquid::LEAN);
    assert_eq!(float("LQ_LEAN_AT"), liquid::LEAN_AT);
    assert_eq!((float("LQ_REST_X"), float("LQ_REST_Y")), liquid::REST);
    assert_eq!(word("LQ_COUNT_SALT"), liquid::COUNT_SALT);
    assert_eq!(word("LQ_BEAD_SALT"), liquid::BEAD_SALT);
    assert_eq!(float("LQ_BEAD_IN"), liquid::BEAD_IN);
}

/// **`WIDE_TAIL` in the shader and `Content::WIDE_TAIL` are one fact spelled
/// twice.** The cell shader reads the grid in place and gives a wide tail
/// its lead's selection by comparing the cell's word against the literal;
/// a literal that drifted from the reserved value would half-select every
/// wide character a block's edge cuts on one GPU arm and not the other.
#[test]
fn the_wide_tail_value_matches_the_shader() {
    let src = include_str!("../src/shaders.metal");
    let prefix = "#define WIDE_TAIL ";
    let line = src
        .lines()
        .find(|l| l.starts_with(prefix))
        .expect("shaders.metal defines WIDE_TAIL");
    let hex = line[prefix.len()..]
        .trim()
        .trim_end_matches('u')
        .trim_start_matches("0x");
    let value = u32::from_str_radix(hex, 16).expect("WIDE_TAIL is a hex literal");
    assert_eq!(
        value,
        baud_domain::Content::WIDE_TAIL.bits(),
        "the shader tests {line:?} for a wide tail; it must be the grid's word"
    );
}

/// **`SHAPE_BLINK` and `SHAPE_CONCEAL` in the shader and `Shapes::BLINK` and
/// `Shapes::CONCEAL` are one fact spelled twice.** The resolved table carries
/// the bits and the shader tests them to hide a cell's text; a literal that
/// drifted from its constant would draw blinking or concealed text on the
/// cell path while the instance path hid it, or hide some other rendition.
#[test]
fn the_hidden_text_bits_match_the_shader() {
    let src = include_str!("../src/shaders.metal");
    for (name, want) in [
        ("SHAPE_BLINK", baud_render::Shapes::BLINK.0),
        ("SHAPE_CONCEAL", baud_render::Shapes::CONCEAL.0),
    ] {
        let prefix = format!("#define {name} ");
        let line = src
            .lines()
            .find(|l| l.starts_with(&prefix))
            .unwrap_or_else(|| panic!("shaders.metal defines {name}"));
        let got: u32 = line[prefix.len()..]
            .trim()
            .trim_end_matches('u')
            .parse()
            .unwrap_or_else(|_| panic!("{name} is an unsigned literal"));
        assert_eq!(got, want, "{line:?} against Shapes' {name}");
    }
}

/// **The cell shader composes the glyph-map key the CPU builds.** The face
/// style's bits in a resolved slot, and where they sit in the key, are one
/// fact spelled in Rust (`Shapes::FACE_SHIFT`, `CellKey::STYLE_SHIFT`) and
/// in the shader; a drift would have the shader probe for keys the CPU
/// never inserted, and every bold cell would draw nothing on the cell path.
#[test]
fn the_face_style_bits_match_the_shader() {
    let src = include_str!("../src/shaders.metal");
    for (name, want) in [
        ("FACE_SHIFT", baud_render::Shapes::FACE_SHIFT),
        ("KEY_STYLE_SHIFT", baud_render::CellKey::STYLE_SHIFT),
        // The smoothing looks (note J): a drift probes a key the walk never
        // inserted, and every smoothed cell draws nothing.
        ("FG_LOOK_SHIFT", baud_render::Shapes::FG_LOOK_SHIFT),
        ("BG_LOOK_SHIFT", baud_render::Shapes::BG_LOOK_SHIFT),
        ("KEY_LOOK_SHIFT", baud_render::CellKey::LOOK_SHIFT),
        // The two-cell span: a drift probes a key the walk never inserted,
        // and every wide character draws nothing on the cell path.
        ("KEY_SPAN_SHIFT", baud_render::CellKey::SPAN_SHIFT),
    ] {
        let prefix = format!("#define {name} ");
        let line = src
            .lines()
            .find(|l| l.starts_with(&prefix))
            .unwrap_or_else(|| panic!("shaders.metal defines {name}"));
        let got: u32 = line[prefix.len()..]
            .trim()
            .trim_end_matches('u')
            .parse()
            .unwrap_or_else(|_| panic!("{name} is an unsigned literal"));
        assert_eq!(got, want, "{line:?} against {name}");
    }
}

/// **`KIND_BAND` and `KIND_PARTICLE` in the shader and `Kind::Band` and
/// `Kind::Particle` are one fact spelled twice.** `glyph_vertex` gives a
/// particle's record its shape coordinates by the literal, and every effect's
/// fragment function tells a particle from a band of its reach by it; a
/// literal that drifted would draw every particle as a band, or a band as a
/// particle, on both GPU arms alike -- which the pixel oracle's reference
/// would catch only as pixels.
#[test]
fn the_effect_kinds_match_the_shader() {
    let src = include_str!("../src/shaders.metal");
    for (name, kind) in [
        ("KIND_BAND", baud_render::Kind::Band),
        ("KIND_PARTICLE", baud_render::Kind::Particle),
    ] {
        let prefix = format!("#define {name} ");
        let line = src
            .lines()
            .find(|l| l.starts_with(&prefix))
            .unwrap_or_else(|| panic!("shaders.metal defines {name}"));
        let got: u32 = line[prefix.len()..]
            .trim()
            .trim_end_matches('u')
            .parse()
            .unwrap_or_else(|_| panic!("{name} is an unsigned literal"));
        assert_eq!(got, kind.code().0, "{line:?} against Kind::{kind:?}");
    }
}

/// **Every effect's fragment function is in the shipped library.** A GPU
/// path builds an effect's pipeline the first frame it draws one, by the
/// name `Effect::fragment` gives; a name the library does not offer fails
/// that build, and the trail is left out without anything else going red --
/// on the machine of whoever first chooses the effect.
#[test]
fn every_effect_has_its_fragment_in_the_library() {
    use objc2_metal::{MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary};

    let Some(device) = MTLCreateSystemDefaultDevice() else {
        panic!("a Metal device; this test cannot skip itself");
    };
    let data = dispatch2::DispatchData::from_static_bytes(include_bytes!("../src/baud.metallib"));
    let lib = device
        .newLibraryWithData_error(&data)
        .expect("baud.metallib loads on the Metal that built this test");
    let names: Vec<String> = lib.functionNames().iter().map(|n| n.to_string()).collect();
    for e in baud_render::effect::Effect::ALL {
        assert!(
            names.iter().any(|n| n == e.fragment()),
            "{e:?}'s {} is not in baud.metallib -- run scripts/build-metallib.sh",
            e.fragment()
        );
    }
}
