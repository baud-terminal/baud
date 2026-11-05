//! FNV-1a 64 against its published test vectors, and against the digests
//! committed beside the Metal library.

use xtask::fnv::{Digest, fnv1a};

#[test]
fn the_published_vectors() {
    assert_eq!(fnv1a(b""), Digest(0xcbf2_9ce4_8422_2325));
    assert_eq!(fnv1a(b"a"), Digest(0xaf63_dc4c_8601_ec8c));
    assert_eq!(fnv1a(b"foobar"), Digest(0x8594_4171_f739_67e8));
    assert_eq!(fnv1a(b"a").to_string(), "af63dc4c8601ec8c");
    assert_eq!(fnv1a(b"\x01").to_string().len(), 16);
}

#[test]
fn the_committed_digests_are_of_the_committed_files() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../baud-present-macos/src/");
    for (file, digest) in [
        ("shaders.metal", "shaders.metal.fnv"),
        ("baud.metallib", "baud.metallib.fnv"),
    ] {
        let bytes = std::fs::read(format!("{dir}{file}")).expect("the input");
        let want = std::fs::read_to_string(format!("{dir}{digest}")).expect("the digest");
        assert_eq!(format!("{}\n", fnv1a(&bytes)), want, "{file}");
    }
}
