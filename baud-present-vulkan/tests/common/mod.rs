//! **What the Vulkan pixel tests share**: the two devices, a record in
//! `Instance::to_bytes`'s layout, and the frame comparison with the one
//! allowance a device may take, a step a channel on blended pixels where it
//! rounds term by term, and the bias bound every device but llvmpipe is held
//! to ([`bias_gated`]).

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
/// (`blend_rounding.rs`). Either way a blended pixel is within one step of
/// the CPU's, and an unblended one is its byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rounding {
    /// Each term to a byte before the sum: some blended pixel takes the
    /// step, the control that the allowance is what lets them through.
    TermByTerm,
    /// The exact sum, rounded once: a blended pixel within a step of the
    /// CPU's byte.
    OnceAtTies,
}

/// **How near a half step, in 255ths, an exact sum lies where RADV may
/// round it the other way**: 26, over the 20 observed on an RX 9070 XT
/// across every coverage of six ink and page pairs (`blend_rounding.rs`).
/// Widened only on new isolation data.
pub const TIE_WINDOW: u32 = 26;

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

/// The most any channel of `a` and `b` differ by.
fn spread(a: u32, b: u32) -> u32 {
    [0, 8, 16, 24]
        .iter()
        .map(|s| ((a >> s) & 0xff).abs_diff((b >> s) & 0xff))
        .max()
        .unwrap_or(0)
}

/// **The bar a frame is held to**: how its device rounds a blend, and which
/// device it is, by `VkDriverId` ([`bias_gated`]).
#[derive(Clone, Copy)]
pub struct Bar {
    pub rounding: Rounding,
    pub driver: ash::vk::DriverId,
}

/// **`got` against `want`, pixel for pixel**: exact where `blended` is
/// false, and within a step a channel where it is true, on every device;
/// under [`Rounding::TermByTerm`], as the control, some pixel must take the
/// step, so the allowance is what lets them through rather than a scene
/// that never blends.
pub fn assert_frame(got: &[u32], want: &[u32], blended: &[bool], bar: Bar, width: usize) {
    let width = Px(u32::try_from(width).expect("a width"));
    assert_frame_of("the frame", got, want, blended, bar, width);
}

/// **[`assert_frame`], each failure named by `what`**: the scene and the
/// path the frame is of.
pub fn assert_frame_of(
    what: &str,
    got: &[u32],
    want: &[u32],
    blended: &[bool],
    bar: Bar,
    width: Px,
) {
    let stacked = vec![false; blended.len()];
    assert_frame_stacked(what, got, want, (blended, &stacked), bar, width);
}

/// **[`assert_frame_of`], with the pixels two or more blends stack on**:
/// where each term of each blend is rounded to a byte
/// ([`Rounding::TermByTerm`]) such a pixel may take a second step, as an
/// effect's records stacked on lavapipe do (`effect_pixels.rs`); a device
/// that rounds the sum once holds it to one.
pub fn assert_frame_stacked(
    what: &str,
    got: &[u32],
    want: &[u32],
    (blended, stacked): (&[bool], &[bool]),
    bar: Bar,
    width: Px,
) {
    let width = usize::try_from(width.0).expect("a width");
    let Bar { rounding, driver } = bar;
    assert_eq!(
        (got.len(), want.len(), blended.len(), stacked.len()),
        (want.len(), blended.len(), got.len(), got.len()),
        "{what}: the frames' sizes"
    );
    let mut wrong = Vec::new();
    let mut stepped = 0;
    for (i, (((&g, &w), &b), &s)) in got.iter().zip(want).zip(blended).zip(stacked).enumerate() {
        let steps = match (b, s, rounding) {
            (false, _, _) => 0,
            (true, true, Rounding::TermByTerm) => 2,
            (true, _, _) => 1,
        };
        stepped += usize::from(b && g != w);
        if spread(g, w) > steps {
            wrong.push(format!("({}, {}, {g:08x}, {w:08x})", i % width, i / width));
        }
    }
    assert!(
        wrong.is_empty(),
        "{what}: {} pixels wrong, the first (x, y, got, want) {}",
        wrong.len(),
        wrong[..wrong.len().min(6)].join(" ")
    );
    assert!(
        rounding == Rounding::OnceAtTies || stepped > 0,
        "{what}: no blended pixel took the step: the allowance tested nothing"
    );
    let bias = mean_signed(got, want, blended);
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
