//! FNV-1a 64 against its published test vectors, and against the Metal
//! library's committed digest.

use xtask::fnv::{Digest, fnv1a};

#[test]
fn the_published_vectors() {
    assert_eq!(fnv1a(b""), Digest(0xcbf2_9ce4_8422_2325));
    assert_eq!(fnv1a(b"a"), Digest(0xaf63_dc4c_8601_ec8c));
    assert_eq!(fnv1a(b"foobar"), Digest(0x8594_4171_f739_67e8));
    assert_eq!(fnv1a(b"a").to_string(), "af63dc4c8601ec8c");
    assert_eq!(fnv1a(b"\x01").to_string().len(), 16);
}

/// The library's digest only. `shaders.metal.fnv` is the digest of the
/// spliced text the build writes into baud-present-macos's `OUT_DIR`
/// (the shaders with the stress table in place of its `#include`), which
/// xtask has no route to; `baud-present-macos/tests/metallib.rs`'s
/// `the_library_was_built_from_the_shaders_beside_it` holds that one.
#[test]
fn the_committed_digest_is_of_the_committed_library() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../baud-present-macos/src/");
    let bytes = std::fs::read(format!("{dir}baud.metallib")).expect("the library");
    let want = std::fs::read_to_string(format!("{dir}baud.metallib.fnv")).expect("the digest");
    assert_eq!(format!("{}\n", fnv1a(&bytes)), want, "baud.metallib");
}
