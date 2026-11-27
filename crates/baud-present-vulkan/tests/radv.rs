//! **One frame on an AMD GPU (RADV), with explicit sync.** These need the
//! GPU, so they are `#[ignore]`d and run by name from
//! `tools/scripts/linux-vulkan-radv.sh`, never skipped at run time: a run without
//! the GPU fails rather than passing a path it did not take.
//!
//! The device is the one whose render node the feedback names, as the
//! compositor's would. Every pixel is read back by a GPU copy, on LINEAR
//! and on the driver's own first modifier, the second after the frame
//! signals its acquire point.

#![cfg(target_os = "linux")]

use baud_present_vulkan::choose::{Kind, Node, Sync, SyncObj, choose};
use baud_present_vulkan::frame::{Gpu, Premultiplied};
use baud_present_vulkan::sys::is_syncobj;
use baud_present_vulkan::vk::Vulkan;
use baud_present_wayland::Px;
use baud_present_wayland::dmabuf::{
    Count, Feedback, Format, Fourcc, Modifier, Point, Tranche, TrancheFlags,
};
use baud_present_wayland::sys::{FileLen, anonymous};
use std::os::fd::AsFd;

/// The GPU, its render node, and a feedback naming that node and offering
/// ARGB8888 on `modifier` alone.
fn radv(modifier: Option<Modifier>) -> (Gpu, Node) {
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
    let modifier = modifier.unwrap_or_else(|| {
        *gpu.modifiers
            .iter()
            .find(|&&m| m != Modifier::LINEAR)
            .expect("a modifier of the driver's own")
    });
    let offered = Feedback {
        main: Some(render.dev()),
        tranches: vec![Tranche {
            device: Some(render.dev()),
            flags: TrancheFlags(0),
            formats: vec![Format {
                fourcc: Fourcc::ARGB8888,
                modifier,
            }],
        }],
        unknown_indices: Count(0),
    };
    let choice = choose(core::slice::from_ref(&gpu), &offered, SyncObj::Offered).expect("the GPU");
    assert_eq!(choice.sync, Sync::Explicit, "RADV exports a timeline");
    assert_eq!(choice.format.modifier, modifier);
    (
        Gpu::new(vulkan, physical, choice).expect("a device"),
        render,
    )
}

/// **On LINEAR every pixel is the premultiplied colour**, read back by a
/// GPU copy: amdgpu refuses a CPU mapping of device-local memory.
#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn radv_clears_one_frame_into_a_linear_dmabuf() {
    let (gpu, _) = radv(Some(Modifier::LINEAR));
    let (w, h) = (Px(64), Px(32));
    let t = gpu.target(w, h).expect("an exported image");
    let colour = baud_domain::premultiply(0x8040_80c0);
    gpu.clear(&t, Premultiplied(colour), None)
        .expect("a cleared frame");
    let words = gpu.read(&t).expect("read back by the GPU");
    assert_eq!(words.len(), 64 * 32);
    let wrong = words.iter().filter(|&&p| p != colour).count();
    assert_eq!(wrong, 0, "every pixel is the premultiplied colour");
    gpu.drop_target(t);
}

/// **On the driver's own modifier the frame signals its acquire point on a
/// timeline the kernel takes as a syncobj**, and a file that is not one is
/// refused by the same check, so the compositor is never sent it.
#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn radv_signals_a_frame_on_a_syncobj_timeline() {
    let (gpu, render) = radv(None);
    let t = gpu.target(Px(256), Px(128)).expect("an exported image");
    assert!(t.stride.0 >= 256 * 4, "a row holds the width");
    let tl = gpu.timeline().expect("a timeline");
    assert!(
        is_syncobj(render, tl.fd.as_fd()),
        "the timeline's fd is a syncobj"
    );
    let not = anonymous(FileLen(4096)).expect("a plain file");
    assert!(
        !is_syncobj(render, not.as_fd()),
        "the control: a plain file is not"
    );
    assert_eq!(gpu.reached(&tl), Ok(Point(0)));
    gpu.clear(&t, Premultiplied(0x8020_4060), Some((&tl, Point(1))))
        .expect("a cleared frame");
    assert_eq!(
        gpu.reached(&tl),
        Ok(Point(1)),
        "the acquire point is signalled"
    );
    let words = gpu.read(&t).expect("read back by the GPU");
    let wrong = words.iter().filter(|&&p| p != 0x8020_4060).count();
    assert_eq!(wrong, 0, "every pixel of the tiled frame is the colour");
    gpu.drop_timeline(tl);
    gpu.drop_target(t);
}
