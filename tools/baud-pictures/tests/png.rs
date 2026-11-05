//! The PNG writer against Pillow 12.3.0's bytes: the committed pictures
//! read back and written again, and three images Pillow wrote in the
//! pinned nixpkgs (`7dd199b0`), pinned by their SHA-256, two of them past
//! one encode buffer.

use baud_pictures::png::{self, Image, Mode};
use baud_pictures::sha256;

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// The bytes the test images are made of, the generator that made
/// Pillow's copies: a 64-bit LCG, its top byte.
fn noise(n: usize, mut seed: u64) -> Vec<u8> {
    (0..n)
        .map(|_| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 56) as u8
        })
        .collect()
}

fn idats(b: &[u8]) -> Vec<usize> {
    png::chunks(b)
        .expect("a PNG")
        .iter()
        .filter(|(k, _)| k == "IDAT")
        .map(|(_, d)| d.len())
        .collect()
}

/// **Every committed PNG the brand writes, read back and written again,
/// is its own bytes**: the filter choice, deflate and the chunking, on
/// the RGBA Dock icons and the RGB link and organisation pictures.
#[test]
fn every_committed_brand_png_is_what_the_writer_writes() {
    let mut names: Vec<String> = [16, 32, 64, 128, 256, 512, 1024]
        .iter()
        .map(|n| format!("assets/logo/baud-icon-dock-{n}.png"))
        .collect();
    names.push("assets/logo/baud-github-org.png".into());
    names.push("assets/readme/social.png".into());
    for name in &names {
        let b = std::fs::read(root().join(name)).expect("a committed PNG");
        let im = png::decode(&b).expect("it reads");
        assert_eq!(png::encode(&im), b, "{name}");
    }
}

/// **Past one buffer, one IDAT per encode call.** A 300 by 300 RGB of
/// noise deflates to 270,701 bytes, five IDATs at Pillow's 65,536; a
/// 17,000 by 2 RGBA takes the `width * 4` buffer, 68,000, three IDATs.
/// Each is Pillow's file, byte for byte; the control: the wide one split
/// at 65,536 is not.
#[test]
fn a_picture_longer_than_the_buffer_is_split_as_pillow_splits_it() {
    let rgb = Image {
        w: 300,
        h: 300,
        mode: Mode::Rgb,
        px: noise(300 * 300 * 3, 1),
    };
    let b = png::encode(&rgb);
    assert_eq!(b.len(), 270_701);
    assert_eq!(
        sha256::hex(&b),
        "b32ec2fc5d8388000a35b9c35c0c9f0fea3bbea7b5ae01a209867da355cbcab7"
    );
    assert_eq!(idats(&b), [65536, 65536, 65536, 65536, 8452]);

    let wide = Image {
        w: 17_000,
        h: 2,
        mode: Mode::Rgba,
        px: noise(17_000 * 2 * 4, 2),
    };
    let b = png::encode(&wide);
    assert_eq!(
        sha256::hex(&b),
        "dccc73180d36e410f38e05135731a251da59ebffc885d4aadd9fac82ca01b3cc"
    );
    assert_eq!(idats(&b).len(), 3);
    assert_eq!(idats(&b)[..2], [68_000, 68_000]);
    let at_maxblock = png::encode_with(&wide, png::MAXBLOCK);
    assert_ne!(sha256::hex(&at_maxblock), sha256::hex(&b), "the control");
    assert_eq!(png::decode(&b).expect("it reads"), wide);
}

/// **A smooth image takes every filter**, and the bytes are Pillow's.
#[test]
fn a_smooth_image_is_filtered_as_pillow_filters_it() {
    let (w, h) = (64usize, 48usize);
    let px = (0..h)
        .flat_map(|y| {
            (0..w).flat_map(move |x| (0..3).map(move |c| ((x * 3 + y * 5 + c * 40) & 0xFF) as u8))
        })
        .collect();
    let im = Image {
        w,
        h,
        mode: Mode::Rgb,
        px,
    };
    let b = png::encode(&im);
    assert_eq!(
        sha256::hex(&b),
        "3e92563d3286cd9aa80d63fc57cab4a069b876ea43b68a39197bd9d8ca1f5fac"
    );
    assert_eq!(png::decode(&b).expect("it reads"), im);
}
