//! **How a device rounds the premultiplied over of a mask's coverage**:
//! every coverage 0..=255 of an opaque ink over an opaque page, for six
//! pairs, drawn by the instance path, each pixel set beside the CPU's rule
//! (the exact sum, rounded once) and each term rounded before the sum.
//!
//! Lavapipe follows the terms rule on every pixel. RADV rounds the exact
//! sum once, but where the exact sum lies within `common::TIE_WINDOW` of
//! a half it may land a step either side: every pixel it does not round as
//! the CPU does is one step off, at such a tie, and some pixel is (the
//! control that the pairs reach a tie).

#![cfg(target_os = "linux")]

mod common;

use baud_present_vulkan::draw::{Pass, Records, Scene, Texel, Texels};
use baud_present_vulkan::frame::{Gpu, Premultiplied};
use baud_present_vulkan::pipeline::Pipelines;
use baud_present_wayland::Px;
use common::{TIE_WINDOW, record};

const W: u32 = 256;

/// The inks and pages, each a grey level.
const PAIRS: [(u32, u32); 6] = [
    (0xff, 0x10),
    (0xf4, 0x10),
    (0xe0, 0x20),
    (0x80, 0x40),
    (0x10, 0xf0),
    (0xa3, 0x2c),
];

/// One channel's over at coverage `a` of 255: the exact sum rounded once,
/// and each term rounded before the sum.
fn rules(c: u32, d: u32, a: u32) -> (u32, u32) {
    let exact = (c * a + d * (255 - a) + 127) / 255;
    let terms = (c * a + 127) / 255 + (d * (255 - a) + 127) / 255;
    (exact, terms)
}

/// A count of pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Pixels(u64);

/// An amount in 255ths of a step, or in steps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Of255(u32);

/// What a device's frames showed against the rules.
#[derive(Debug)]
struct Measured {
    seen: Pixels,
    /// Pixels the exact rule gives, and the terms rule.
    exact: Pixels,
    terms: Pixels,
    /// Pixels off the exact rule, below it and above it.
    low: Pixels,
    high: Pixels,
    /// The most any pixel off the exact rule is off by, and the furthest
    /// its exact sum lay from a half step, in 255ths of a step.
    worst: Of255,
    from_half: Of255,
}

fn measure(gpu: &Gpu) -> Measured {
    let mut pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    let curves: Vec<u8> = (0..=255u8).collect();
    let mask: Vec<u8> = (0..=255u8).collect();
    let sheets = common::sheets(
        gpu,
        Texels {
            width: Px(W),
            height: Px(1),
            texel: Texel::Coverage,
            bytes: &mask,
        },
        Texels {
            width: Px(1),
            height: Px(1),
            texel: Texel::Argb,
            bytes: &[0; 4],
        },
        &curves,
    );
    let mut m = Measured {
        seen: Pixels(0),
        exact: Pixels(0),
        terms: Pixels(0),
        low: Pixels(0),
        high: Pixels(0),
        worst: Of255(0),
        from_half: Of255(0),
    };
    for (ink, page) in PAIRS {
        let t = gpu.target(Px(W), Px(1)).expect("a target");
        gpu.clear(&t, Premultiplied(0), None).expect("cleared");
        let grey = |v: u32| 0xff00_0000 | v << 16 | v << 8 | v;
        let bg = grey(page).to_le_bytes();
        let rec = record((0, 0), (256, 1), (0, 0), (grey(ink), 0));
        let scene = Scene {
            u: [
                W as f32, 1.0, W as f32, 1.0, 1.0, 1.0, W as f32, 1.0, 1.0, 1.0, 0.0, 0.0,
            ],
            backgrounds: Texels {
                width: Px(1),
                height: Px(1),
                texel: Texel::Argb,
                bytes: &bg,
            },
            sheets: &sheets,
            records: &rec,
            count: Records(1),
            framing: None,
        };
        gpu.draw(&mut pipes, &t, &scene, Pass::Whole)
            .expect("drawn");
        let got = gpu.read(&t).expect("read back");
        gpu.drop_target(t);
        for (a, px) in (0u32..).zip(&got) {
            let g = px & 0xff;
            let (exact, terms) = rules(ink, page, a);
            m.seen.0 += 1;
            m.exact.0 += u64::from(g == exact);
            m.terms.0 += u64::from(g == terms);
            if g != exact {
                match g < exact {
                    true => m.low.0 += 1,
                    false => m.high.0 += 1,
                }
                let frac = (ink * a + page * (255 - a)) % 255;
                m.worst = m.worst.max(Of255(g.abs_diff(exact)));
                m.from_half = m.from_half.max(Of255((2 * frac).abs_diff(255) / 2));
            }
        }
    }
    sheets.destroy(gpu);
    pipes.destroy();
    eprintln!("{m:?}");
    m
}

#[test]
fn lavapipe_rounds_each_term_of_an_over() {
    let m = measure(&common::lavapipe());
    assert_eq!(m.terms, m.seen, "every pixel is the terms rule's");
    assert!(
        m.exact < m.seen,
        "and the pairs reach where it is not the CPU's"
    );
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn radv_misses_the_cpu_only_a_step_at_a_tie() {
    let m = measure(&common::radv());
    assert!(
        m.exact < m.seen,
        "the pairs reach a tie RADV rounds the other way"
    );
    assert_eq!(m.worst, Of255(1), "a miss is one step");
    assert!(
        m.from_half <= Of255(TIE_WINDOW),
        "a miss lies within {TIE_WINDOW}/255 of a half: {}/255",
        m.from_half.0
    );
}
