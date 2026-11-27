//! **What the Vulkan pixel tests share**: the two devices, a record in
//! `Instance::to_bytes`'s layout, and the frame comparison with the one
//! allowance a device may take: a step a channel on blended pixels where it
//! rounds term by term, and where it rounds the sum once a step only on a
//! channel whose exact sum lies at a tie ([`Tie`]); and the bias bound every
//! device but llvmpipe is held to ([`bias_gated`]).

#![allow(dead_code)]

use baud_present_vulkan::choose::{Kind, SyncObj, choose};
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::vk::Vulkan;
use baud_present_wayland::Px;
use baud_present_wayland::dmabuf::{
    Count, Device, Feedback, Format, Fourcc, Modifier, Tranche, TrancheFlags,
};

/// **How a device rounds a blend at partial alpha.** Lavapipe rounds the
/// source and destination terms each to a byte before it adds them (133
/// for the CPU's 132 at coverage 72, 240 over 90); RADV forms the exact
/// sum and rounds it once, at a precision that can land a step either side
/// where the sum lies within [`TIE_WINDOW`] of a half
/// (`blend_rounding.rs`). Rounded term by term a blended pixel is within a
/// step a blend of the CPU's; rounded once it is the CPU's byte on every
/// channel but one whose exact sum lies at a tie ([`Tie::Near`]), which may
/// be a step off. An unblended pixel is its byte on both.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rounding {
    /// Each term to a byte before the sum: some blended pixel takes the
    /// step, the control that the allowance is what lets them through.
    TermByTerm,
    /// The exact sum, rounded once: a blended pixel within a step of the
    /// CPU's byte.
    OnceAtTies,
}

/// **Where a blended channel's exact value lies against a half step**, as
/// the CPU path forms it: the one place a device that rounds the sum once
/// may land a step from the CPU's byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tie {
    /// Further than [`TIE_WINDOW`] from a half on every blend the channel
    /// takes: the CPU's byte.
    Away,
    /// Within [`TIE_WINDOW`] of a half on some blend: a step either side.
    Near,
    /// Drawn by an effect, whose coverage each path forms from its own
    /// floats rather than from a byte both share: a step, the bar
    /// `effect_pixels.rs` holds an effect's pixels to.
    Effect,
}

impl Tie {
    /// **The tie of an exact sum `n` in 255ths of a step**, `n / 255` the
    /// value before it is rounded to a byte.
    pub fn of(n: u32) -> Tie {
        match from_half(n) <= TIE_WINDOW {
            true => Tie::Near,
            false => Tie::Away,
        }
    }

    /// **The tie of an exact value `v` in steps**, a float as an effect's
    /// stage forms it.
    pub fn of_steps(v: f32) -> Tie {
        let frac = v - v.floor();
        match (frac - 0.5).abs() * 255.0 <= TIE_WINDOW as f32 {
            true => Tie::Near,
            false => Tie::Away,
        }
    }

    /// The nearer of two: a channel at a tie on any blend it takes is at
    /// one, and one an effect drew stays the effect's.
    pub fn or(self, other: Tie) -> Tie {
        match (self, other) {
            (Tie::Effect, _) | (_, Tie::Effect) => Tie::Effect,
            (Tie::Near, _) | (_, Tie::Near) => Tie::Near,
            _ => Tie::Away,
        }
    }
}

/// **How far an exact sum `n`, in 255ths of a step, lies from a half
/// step**, in 255ths, rounded down.
pub fn from_half(n: u32) -> u32 {
    (2 * (n % 255)).abs_diff(255) / 2
}

/// **The ties of a pixel's four channels**, in the word's byte order:
/// blue, green, red, alpha.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ties(pub [Tie; 4]);

impl Ties {
    /// No channel at a tie: what an unblended pixel, or one whose sums
    /// all lie away from a half, carries.
    pub const AWAY: Ties = Ties([Tie::Away; 4]);
    /// Every channel an effect's.
    pub const EFFECT: Ties = Ties([Tie::Effect; 4]);

    /// Each channel the nearer of its two ([`Tie::or`]).
    pub fn or(self, other: Ties) -> Ties {
        Ties(core::array::from_fn(|k| self.0[k].or(other.0[k])))
    }

    /// Whether an effect drew any channel: the pixel's value is then not
    /// formed from bytes both paths share.
    pub fn effect(self) -> bool {
        self.0.contains(&Tie::Effect)
    }
}

/// **How near a half step, in 255ths, an exact sum lies where RADV may
/// round it the other way**: 32, the furthest of its misses over every
/// 8-bit over, 256^3 of them (every ink over every page at every
/// coverage), on an RX 9070 XT (1002:7550) under RADV, each miss one step.
/// `blend_rounding.rs` runs that sweep, prints the device, the driver and
/// the furthest miss it saw, and holds the furthest to this exactly.
pub const TIE_WINDOW: u32 = 32;

/// The first GPU with a render node, offered ARGB8888 on LINEAR.
pub fn radv() -> Gpu {
    let vulkan = Vulkan::open().expect("libvulkan");
    let (physical, gpu, render) = vulkan
        .candidates()
        .into_iter()
        .find_map(|(pd, c)| match c.kind {
            Kind::Gpu {
                render: Some(r), ..
            } => Some((pd, c, r)),
            _ => None,
        })
        .expect("a GPU with a render node");
    let offered = Feedback {
        main: Some(render.dev()),
        tranches: vec![Tranche {
            device: Some(render.dev()),
            flags: TrancheFlags(0),
            formats: vec![Format {
                fourcc: Fourcc::ARGB8888,
                modifier: Modifier::LINEAR,
            }],
        }],
        unknown_indices: Count(0),
    };
    let choice = choose(core::slice::from_ref(&gpu), &offered, SyncObj::Absent).expect("the GPU");
    Gpu::new(vulkan, physical, choice).expect("a device")
}

/// Lavapipe, pinned in the development shell, offered ARGB8888 on LINEAR.
pub fn lavapipe() -> Gpu {
    let vulkan = Vulkan::open().expect("libvulkan, pinned in the development shell");
    let (physical, cpu) = vulkan
        .candidates()
        .into_iter()
        .find(|(_, c)| c.kind == Kind::Cpu)
        .expect("lavapipe, pinned in the development shell");
    let offered = Feedback {
        main: Some(Device(0)),
        tranches: vec![Tranche {
            device: None,
            flags: TrancheFlags(0),
            formats: vec![Format {
                fourcc: Fourcc::ARGB8888,
                modifier: Modifier::LINEAR,
            }],
        }],
        unknown_indices: Count(0),
    };
    let choice =
        choose(core::slice::from_ref(&cpu), &offered, SyncObj::Absent).expect("lavapipe alone");
    Gpu::new(vulkan, physical, choice).expect("a device")
}

/// One record, `Instance::to_bytes`'s layout.
pub fn record(
    (x, y): (i16, i16),
    (w, h): (u16, u16),
    (u, v): (u16, u16),
    (color, kind): (u32, u32),
) -> [u8; 20] {
    let mut b = [0u8; 20];
    b[0..2].copy_from_slice(&x.to_le_bytes());
    b[2..4].copy_from_slice(&y.to_le_bytes());
    b[4..6].copy_from_slice(&w.to_le_bytes());
    b[6..8].copy_from_slice(&h.to_le_bytes());
    b[8..10].copy_from_slice(&u.to_le_bytes());
    b[10..12].copy_from_slice(&v.to_le_bytes());
    b[12..16].copy_from_slice(&color.to_le_bytes());
    b[16..20].copy_from_slice(&kind.to_le_bytes());
    b
}

/// How far each channel of `a` and `b` differ, in the word's byte order.
fn spreads(a: u32, b: u32) -> [u32; 4] {
    [0, 8, 16, 24].map(|s| ((a >> s) & 0xff).abs_diff((b >> s) & 0xff))
}

/// **The bar a frame is held to**: how its device rounds a blend, and which
/// device it is, by `VkDriverId` ([`bias_gated`]).
#[derive(Clone, Copy)]
pub struct Bar {
    pub rounding: Rounding,
    pub driver: ash::vk::DriverId,
}

/// **`got` against `want`, pixel for pixel**: exact where `blended` is
/// false; where it is true within a step a channel under
/// [`Rounding::TermByTerm`], and under [`Rounding::OnceAtTies`] exact on
/// every channel but one `ties` puts at a tie, which may be a step off.
/// Under [`Rounding::TermByTerm`], as the control, some pixel must take the
/// step, so the allowance is what lets them through rather than a scene
/// that never blends.
pub fn assert_frame(
    got: &[u32],
    want: &[u32],
    (blended, ties): (&[bool], &[Ties]),
    bar: Bar,
    width: Px,
) {
    assert_frame_of("the frame", got, want, (blended, ties), bar, width);
}

/// **[`assert_frame`], each failure named by `what`**: the scene and the
/// path the frame is of.
pub fn assert_frame_of(
    what: &str,
    got: &[u32],
    want: &[u32],
    (blended, ties): (&[bool], &[Ties]),
    bar: Bar,
    width: Px,
) {
    let stacked = vec![false; blended.len()];
    assert_frame_stacked(what, got, want, (blended, &stacked, ties), bar, width);
}

/// **[`assert_frame_of`], with the pixels two or more blends stack on**:
/// [`assert_frame_covered`] with those pixels covered twice and the other
/// blended ones once.
pub fn assert_frame_stacked(
    what: &str,
    got: &[u32],
    want: &[u32],
    (blended, stacked, ties): (&[bool], &[bool], &[Ties]),
    bar: Bar,
    width: Px,
) {
    assert_eq!(
        (blended.len(), stacked.len()),
        (got.len(), got.len()),
        "{what}: the masks' sizes"
    );
    let covers: Vec<Cover> = blended
        .iter()
        .zip(stacked)
        .map(|(&b, &s)| Cover(u8::from(b) + u8::from(b && s)))
        .collect();
    assert_frame_covered(what, got, want, (&covers, ties), bar, width);
}

/// How many of a scene's records blend a pixel: draw it at a coverage
/// strictly between nothing and all, counted from the scene, never from
/// the frames compared.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Cover(pub u8);

/// **The steps a channel `n` records blend may be off by**: none
/// unblended; where each term of each blend is rounded to a byte
/// ([`Rounding::TermByTerm`]), a step a record -- each blend adds up to a
/// step and the carried error shrinks by `1 - a`, so `n` records leave at
/// most `n` steps, and a third step is open where the third record's alpha
/// is at most a quarter; on a device that rounds the sum once, a step
/// where some blend's exact sum lies at a tie ([`Tie::Near`]) or an effect
/// drew it ([`Tie::Effect`]), and none where every sum lies away from one.
/// The term-by-term bound holds while both arms blend each record with the
/// same source and the same float alpha over an equal page byte under the
/// first record (`cell_effects.rs`,
/// `the_allowance_is_the_bound_term_by_term_rounding_reaches`).
pub fn allowance(n: Cover, tie: Tie, rounding: Rounding) -> u32 {
    match (n.0, rounding, tie) {
        (0, _, _) => 0,
        (n, Rounding::TermByTerm, _) => u32::from(n),
        (_, Rounding::OnceAtTies, Tie::Away) => 0,
        (_, Rounding::OnceAtTies, Tie::Near | Tie::Effect) => 1,
    }
}

/// **`got` against `want`, each channel within the [`allowance`] its
/// pixel's cover and the channel's tie give**, each failure named by
/// `what`; under [`Rounding::TermByTerm`], as the control, some blended
/// pixel must take a step. How many pixels sit at their cap, by cover, is
/// reported, so a device that pins many there shows as a number. The lean
/// over the blended pixels is held to [`BIAS`] where [`bias_gated`] says.
pub fn assert_frame_covered(
    what: &str,
    got: &[u32],
    want: &[u32],
    (covers, ties): (&[Cover], &[Ties]),
    bar: Bar,
    width: Px,
) {
    let width = usize::try_from(width.0).expect("a width");
    let Bar { rounding, driver } = bar;
    assert_eq!(
        (got.len(), want.len(), covers.len(), ties.len()),
        (want.len(), covers.len(), got.len(), got.len()),
        "{what}: the frames' sizes"
    );
    let blended: Vec<bool> = covers.iter().map(|c| c.0 > 0).collect();
    let mut wrong = Vec::new();
    let mut stepped = 0;
    let mut capped = std::collections::BTreeMap::<Cover, usize>::new();
    for (i, (((&g, &w), &n), t)) in got.iter().zip(want).zip(covers).zip(ties).enumerate() {
        let steps = t.0.map(|tie| allowance(n, tie, rounding));
        let off = spreads(g, w);
        stepped += usize::from(n.0 > 0 && g != w);
        if (0..4).any(|k| steps[k] > 0 && off[k] == steps[k]) {
            *capped.entry(n).or_default() += 1;
        }
        if (0..4).any(|k| off[k] > steps[k]) {
            wrong.push(format!(
                "({}, {}, {g:08x}, {w:08x}, {}, {:?})",
                i % width,
                i / width,
                n.0,
                t.0
            ));
        }
    }
    eprintln!("{what}: pixels at their cap, by cover: {capped:?}");
    assert!(
        wrong.is_empty(),
        "{what}: {} pixels wrong, the first (x, y, got, want, cover, ties) {}",
        wrong.len(),
        wrong[..wrong.len().min(6)].join(" ")
    );
    assert!(
        rounding == Rounding::OnceAtTies || stepped > 0,
        "{what}: no blended pixel took the step: the allowance tested nothing"
    );
    let bias = mean_signed(got, want, &blended);
    if !bias_gated(driver) {
        eprintln!(
            "{what}: bias (b, g, r, a) on VkDriverId {}, reported and not gated: {bias:?}",
            driver.as_raw()
        );
        return;
    }
    assert!(
        bias.iter().all(|b| b.abs() <= BIAS),
        "{what}: blended pixels lean {bias:?} a channel (b, g, r, a), past {BIAS}: a step every \
         pixel takes one way is an error, not rounding"
    );
}

/// **How far a step may lean**: the mean signed difference a channel over
/// the blended pixels, in steps. Rounding a term at a time goes either way;
/// a slip in premultiplying goes one way on every pixel and stays within a
/// step, which the per-pixel bound alone lets through.
pub const BIAS: f64 = 0.25;

/// **Whether a device's frames are held to [`BIAS`]**: every device but
/// llvmpipe, by its `VkDriverId`. llvmpipe rounds each blend term to a byte
/// before the sum ([`Rounding::TermByTerm`]) and the rounding leans: on the
/// instance path's scene red goes up 16 times and never down, +0.33 step.
/// RADV is exact on the same reference, so a slip in premultiplying shows
/// there at full strength, and a change to a shader, a blend or this helper
/// is not tested until it has run on RADV.
pub fn bias_gated(driver: ash::vk::DriverId) -> bool {
    driver != ash::vk::DriverId::MESA_LLVMPIPE
}

/// The mean of `got - want` a channel (b, g, r, a) over the blended pixels.
pub fn mean_signed(got: &[u32], want: &[u32], blended: &[bool]) -> [f64; 4] {
    let mut sum = [0i64; 4];
    let mut n = 0i64;
    for ((&g, &w), _) in got.iter().zip(want).zip(blended).filter(|(_, b)| **b) {
        for (c, s) in [0, 8, 16, 24].iter().enumerate() {
            sum[c] += i64::from((g >> s) & 0xff) - i64::from((w >> s) & 0xff);
        }
        n += 1;
    }
    sum.map(|s| if n == 0 { 0.0 } else { s as f64 / n as f64 })
}

/// **Sheets holding `mask` and `color` whole**, and `curves`, for a scene
/// drawn from texels a test makes.
pub fn sheets(
    gpu: &Gpu,
    mask: baud_present_vulkan::draw::Texels<'_>,
    color: baud_present_vulkan::draw::Texels<'_>,
    curves: &[u8],
) -> baud_present_vulkan::sheets::Sheets {
    use baud_present_vulkan::sheets::Which;
    let mut sheets = gpu.sheets(curves).expect("the sheets");
    sheets
        .replace(gpu, Which::Mask, &mask)
        .expect("the mask sheet");
    sheets
        .replace(gpu, Which::Color, &color)
        .expect("the colour sheet");
    sheets
}

/// The AMD GPU, its frames synchronised explicitly.
pub fn radv_explicit() -> Gpu {
    use baud_present_vulkan::choose::{Kind, SyncObj, choose};
    use baud_present_vulkan::vk::Vulkan;
    use baud_present_wayland::dmabuf::{
        Count, Feedback, Format, Fourcc, Modifier, Tranche, TrancheFlags,
    };
    let vulkan = Vulkan::open().expect("libvulkan");
    let (physical, gpu, render) = vulkan
        .candidates()
        .into_iter()
        .find_map(|(pd, c)| match c.kind {
            Kind::Gpu {
                render: Some(r), ..
            } => Some((pd, c, r)),
            _ => None,
        })
        .expect("a GPU with a render node");
    let offered = Feedback {
        main: Some(render.dev()),
        tranches: vec![Tranche {
            device: Some(render.dev()),
            flags: TrancheFlags(0),
            formats: vec![Format {
                fourcc: Fourcc::ARGB8888,
                modifier: Modifier::LINEAR,
            }],
        }],
        unknown_indices: Count(0),
    };
    let choice = choose(core::slice::from_ref(&gpu), &offered, SyncObj::Offered).expect("the GPU");
    assert_eq!(
        choice.sync,
        baud_present_vulkan::choose::Sync::Explicit,
        "the premise: explicit sync"
    );
    Gpu::new(vulkan, physical, choice).expect("a device")
}
