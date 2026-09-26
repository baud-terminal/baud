//! The instance record's byte layout is a contract with the shaders in
//! `baud-present-macos`: twenty bytes, little-endian, `i16 x, y; u16 w, h,
//! u, v; u32 color; u32 kind`. A change here that the shader does not
//! follow draws every glyph somewhere else.

use baud_render::{Argb, Instance, Kind, Screen, Tex};

fn sample(kind: Kind) -> Instance {
    Instance {
        x: Screen(-3),
        y: Screen(0x1234),
        w: Tex(7),
        h: Tex(9),
        u: Tex(0x0102),
        v: Tex(0xFFFE),
        color: Argb(0xAABBCCDD),
        kind,
    }
}

#[test]
fn the_record_is_twenty_little_endian_bytes() {
    assert_eq!(core::mem::size_of::<Instance>(), Instance::BYTES);
    let b = sample(Kind::Mask).to_bytes();
    assert_eq!(&b[0..2], &(-3i16).to_le_bytes());
    assert_eq!(&b[2..4], &0x1234i16.to_le_bytes());
    assert_eq!(&b[4..6], &7u16.to_le_bytes());
    assert_eq!(&b[6..8], &9u16.to_le_bytes());
    assert_eq!(&b[8..10], &0x0102u16.to_le_bytes());
    assert_eq!(&b[10..12], &0xFFFEu16.to_le_bytes());
    assert_eq!(&b[12..16], &0xAABBCCDDu32.to_le_bytes());
    assert_eq!(&b[16..20], &0u32.to_le_bytes());
}

/// The kind codes are spelled twice -- as the enum's discriminants and in
/// `Kind::code` -- and the shader switches on these numbers.
#[test]
fn every_kind_has_the_code_the_shader_switches_on() {
    for (kind, code) in [
        (Kind::Mask, 0u32),
        (Kind::Color, 1),
        (Kind::Solid, 2),
        (Kind::Quad, 3),
        (Kind::Fire, 4),
        (Kind::Ember, 5),
    ] {
        assert_eq!(kind.code().0, code, "{kind:?}");
        assert_eq!(&sample(kind).to_bytes()[16..20], &code.to_le_bytes());
    }
}
