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
