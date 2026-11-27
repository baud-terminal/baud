//! Print every Vulkan device as the GPU path's choice reads it: its kind and
//! DRM nodes, a missing extension, the modifiers `B8G8R8A8_UNORM` renders to
//! and exports as one plane, and whether a timeline exports as an fd.

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("vk-probe: Linux only");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    let v = match baud_present_vulkan::vk::Vulkan::open() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("vk-probe: {e:?}");
            std::process::exit(1);
        }
    };
    for (_, c) in v.candidates() {
        println!("{:?} {:?} timeline={:?}", c.kind, c.extensions, c.timeline);
        for m in &c.modifiers {
            println!("  modifier {:#018x}", m.0);
        }
    }
}
