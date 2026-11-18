//! **What the Vulkan pixel tests share**: the two devices, a record in
//! `Instance::to_bytes`'s layout, and the frame comparison with the one
//! allowance a device may take, a step a channel on blended pixels where it
//! rounds term by term.

#![allow(dead_code)]

use baud_present_vulkan::choose::{Kind, SyncObj, choose};
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::vk::Vulkan;
use baud_present_wayland::dmabuf::{
    Count, Device, Feedback, Format, Fourcc, Modifier, Tranche, TrancheFlags,
};

/// **How a device rounds a blend at partial alpha.** Lavapipe rounds the
/// source and destination terms each to a byte before it adds them (133
/// for the CPU's 132 at coverage 72, 240 over 90); RADV forms the sum in
/// floats and rounds once.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rounding {
    /// Each term to a byte before the sum: within one step of the CPU.
    TermByTerm,
    /// The sum in floats, rounded once: the CPU's bytes.
    Exact,
}

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

/// **`got` against `want`, pixel for pixel**: exact where `blended` is
/// false, and where it is true exact under [`Rounding::Exact`] and within
/// a step a channel under [`Rounding::TermByTerm`] -- where, as the
/// control, some pixel must take the step, so the allowance is what lets
/// them through rather than a scene that never blends.
pub fn assert_frame(got: &[u32], want: &[u32], blended: &[bool], rounding: Rounding, width: usize) {
    assert_eq!(
        (got.len(), want.len(), blended.len()),
        (want.len(), blended.len(), got.len())
    );
    let mut wrong = Vec::new();
    let mut stepped = 0;
    for (i, ((&g, &w), &b)) in got.iter().zip(want).zip(blended).enumerate() {
        let ok = match (b, rounding) {
            (true, Rounding::TermByTerm) => {
                stepped += usize::from(g != w);
                spread(g, w) <= 1
            }
            _ => g == w,
        };
        if !ok {
            wrong.push((i % width, i / width, g, w));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} pixels wrong, the first (x, y, got, want) {:x?}",
        wrong.len(),
        &wrong[..wrong.len().min(6)]
    );
    assert!(
        rounding == Rounding::Exact || stepped > 0,
        "no blended pixel took the step: the allowance tested nothing"
    );
}
