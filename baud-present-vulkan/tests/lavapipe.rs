//! **One frame on lavapipe, read back through the dmabuf.** Lavapipe is
//! Mesa's CPU Vulkan, pinned in the development shell, so this runs on any
//! box. The frame is cleared to a colour with alpha below one, so a frame
//! written straight rather than premultiplied reads wrong, and every pixel
//! of the exported plane is read back through a mapping of the fd.

#![cfg(target_os = "linux")]

use baud_present_vulkan::choose::{Kind, SyncObj, choose};
use baud_present_vulkan::frame::{Gpu, Premultiplied};
use baud_present_vulkan::vk::Vulkan;
use baud_present_wayland::Px;
use baud_present_wayland::dmabuf::{
    Count, Device, Dmabuf, Feedback, Format, Fourcc, Modifier, Tranche, TrancheFlags,
};
use baud_present_wayland::sys::{FileLen, Map};

#[test]
fn lavapipe_clears_one_frame_into_a_dmabuf() {
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
    let gpu = Gpu::new(vulkan, physical, choice).expect("a device");
    let (w, h) = (Px(64), Px(32));
    let t = gpu.target(w, h).expect("an exported image");
    let colour = baud_domain::premultiply(0x8040_80c0);
    gpu.clear(&t, Premultiplied(colour), None)
        .expect("a cleared frame");
    // The frame as the window imports it: its size, the format chosen, and
    // the plane the driver laid out.
    let planes = [t.plane()];
    let d = Dmabuf {
        width: t.width,
        height: t.height,
        format: t.format,
        planes: &planes,
    };
    assert_eq!((d.width, d.height), (w, h));
    assert_eq!(d.format.modifier, Modifier::LINEAR, "the format chosen");
    assert_eq!(
        (d.planes[0].offset, d.planes[0].stride),
        (t.offset, t.stride)
    );

    let stride = usize::try_from(t.stride.0).unwrap();
    let offset = usize::try_from(t.offset.0).unwrap();
    let rows = usize::try_from(h.0).unwrap();
    let cols = usize::try_from(w.0).unwrap();
    assert!(stride >= cols * 4, "a row holds the width");
    let mut map = Map::shared(&t.fd, FileLen(offset + stride * rows)).expect("the dmabuf maps");
    let words = map.words();
    let wrong = (0..rows)
        .flat_map(|y| (0..cols).map(move |x| (y, x)))
        .filter(|&(y, x)| words[(offset + y * stride) / 4 + x] != colour)
        .count();
    assert_eq!(colour, 0x8020_4060, "the colour, premultiplied");
    assert_eq!(wrong, 0, "every pixel is the premultiplied colour");
    gpu.drop_target(t);
}
