//! **What the GPU path runs on, chosen from capabilities alone.** The
//! device, the format and modifier, and the sync are each read from what
//! the compositor's feedback offers and what the Vulkan devices report,
//! never from a vendor's name; anything missing is a named reason, and the
//! window stays on shared memory.
//!
//! The device is the one the compositor reads buffers on: "Clients need to
//! create buffers that the main device can import and read from"
//! (`[[source: wp-linux-dmabuf-v1]]`, `main_device`), and "Clients cannot
//! check two devices for equality by comparing the dev_t value", so a
//! device matches by the node's major and minor against either of its own
//! nodes, as `VK_EXT_physical_device_drm` reports them. A device that
//! cannot say its nodes is used only when it is a CPU device and the only
//! device there is, which is how a headless box without a GPU runs.

use baud_present_wayland::dmabuf::{Device, Feedback, Format, Fourcc, Modifier};

/// A device node's major number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Major(pub u32);

/// A device node's minor number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Minor(pub u32);

/// A DRM node, as major and minor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub major: Major,
    pub minor: Minor,
}

impl Node {
    /// The node a `dev_t` names, in Linux's encoding (`gnu_dev_major` and
    /// `gnu_dev_minor`): twelve bits of major and twenty of minor, split
    /// across the word.
    pub fn of(d: Device) -> Node {
        let v = d.0;
        let major = ((v >> 32) & 0xffff_f000) | ((v >> 8) & 0x0fff);
        let minor = ((v >> 12) & 0xffff_ff00) | (v & 0x00ff);
        Node {
            major: Major(u32::try_from(major).unwrap_or(u32::MAX)),
            minor: Minor(u32::try_from(minor).unwrap_or(u32::MAX)),
        }
    }

    /// The `dev_t` of this node, `gnu_dev_makedev`'s encoding.
    pub fn dev(self) -> Device {
        let (major, minor) = (u64::from(self.major.0), u64::from(self.minor.0));
        Device(
            ((major & 0xffff_f000) << 32)
                | ((major & 0x0fff) << 8)
                | ((minor & 0xffff_ff00) << 12)
                | (minor & 0x00ff),
        )
    }
}

/// What kind of device Vulkan reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A GPU, with its primary and render nodes where it says them.
    Gpu {
        primary: Option<Node>,
        render: Option<Node>,
    },
    /// A device that runs on the CPU, such as lavapipe.
    Cpu,
}

/// Whether a device has every extension the path needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extensions {
    All,
    /// The first one missing, by name.
    Missing(&'static str),
}

/// Whether a device can export a timeline semaphore as an fd.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineExport {
    Yes,
    No,
}

/// One Vulkan device, as the choice reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub kind: Kind,
    pub extensions: Extensions,
    /// The modifiers it can render `B8G8R8A8_UNORM` to as a single-plane
    /// colour attachment, and export.
    pub modifiers: Vec<Modifier>,
    pub timeline: TimelineExport,
}

/// Whether the compositor offers wp_linux_drm_syncobj_manager_v1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncObj {
    Offered,
    Absent,
}

/// How a frame is ordered against the compositor's reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sync {
    /// Acquire and release points on a timeline.
    Explicit,
    /// No points: the frame's end is put on its target's dmabuf as the
    /// write fence the compositor's reads wait on, or where the targets
    /// take no sync file, the frame is finished on the CPU before it is
    /// shown.
    Implicit,
}

/// An index into the candidates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceIx(pub usize);

/// What the GPU path runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub device: DeviceIx,
    pub format: Format,
    pub sync: Sync,
}

/// Why the window stays on shared memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The compositor has said no main device, and no tranche names one.
    NoMainDevice,
    /// No device is the compositor's, and no CPU device stands alone.
    NoDevice,
    /// The device matched lacks an extension the path needs.
    Missing(&'static str),
    /// No format the tranches offer is one the device renders to.
    NoModifier,
}

/// The formats a frame is drawn in, in order: with alpha, for the glass,
/// then without.
const FOURCCS: [Fourcc; 2] = [Fourcc::ARGB8888, Fourcc::XRGB8888];

/// **The device, the format and the sync**, or why there is none.
pub fn choose(
    candidates: &[Candidate],
    offered: &Feedback,
    syncobj: SyncObj,
) -> Result<Choice, Refusal> {
    let main = offered
        .main
        .or_else(|| offered.tranches.iter().find_map(|t| t.device))
        .map(Node::of);
    let gpus = candidates
        .iter()
        .any(|c| matches!(c.kind, Kind::Gpu { .. }));
    let lone_cpu = (candidates.len() == 1 && candidates[0].kind == Kind::Cpu).then_some(0);
    let matched = main.and_then(|m| {
        candidates.iter().position(|c| match c.kind {
            Kind::Gpu { primary, render } => primary == Some(m) || render == Some(m),
            Kind::Cpu => false,
        })
    });
    let at = match (matched, lone_cpu, main) {
        (Some(i), _, _) => i,
        (None, Some(i), _) if !gpus => i,
        (None, _, None) => return Err(Refusal::NoMainDevice),
        (None, _, Some(_)) => return Err(Refusal::NoDevice),
    };
    let c = &candidates[at];
    if let Extensions::Missing(name) = c.extensions {
        return Err(Refusal::Missing(name));
    }
    let format = offered
        .tranches
        .iter()
        .find_map(|t| {
            FOURCCS.iter().find_map(|&fourcc| {
                t.formats
                    .iter()
                    .find(|f| f.fourcc == fourcc && c.modifiers.contains(&f.modifier))
            })
        })
        .copied()
        .ok_or(Refusal::NoModifier)?;
    let sync = match (syncobj, c.timeline) {
        (SyncObj::Offered, TimelineExport::Yes) => Sync::Explicit,
        _ => Sync::Implicit,
    };
    Ok(Choice {
        device: DeviceIx(at),
        format,
        sync,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_present_wayland::dmabuf::{Count, Tranche, TrancheFlags};

    fn node(major: Major, minor: Minor) -> Node {
        Node { major, minor }
    }

    /// The `dev_t` of a node: `dev(Major(226), Minor(128))` is the first render node.
    fn dev(major: Major, minor: Minor) -> Device {
        node(major, minor).dev()
    }

    const DCC: Modifier = Modifier(0x0200_0000_0001_ab04);
    const TILED: Modifier = Modifier(0x0200_0000_0000_0b04);

    fn fmt(fourcc: Fourcc, modifier: Modifier) -> Format {
        Format { fourcc, modifier }
    }

    fn feedback(main: Option<Device>, tranches: Vec<Vec<Format>>) -> Feedback {
        Feedback {
            main,
            tranches: tranches
                .into_iter()
                .map(|formats| Tranche {
                    device: main,
                    flags: TrancheFlags(0),
                    formats,
                })
                .collect(),
            unknown_indices: Count(0),
        }
    }

    fn gpu(primary: Node, render: Node, modifiers: Vec<Modifier>) -> Candidate {
        Candidate {
            kind: Kind::Gpu {
                primary: Some(primary),
                render: Some(render),
            },
            extensions: Extensions::All,
            modifiers,
            timeline: TimelineExport::Yes,
        }
    }

    fn cpu() -> Candidate {
        Candidate {
            kind: Kind::Cpu,
            extensions: Extensions::All,
            modifiers: vec![Modifier::LINEAR],
            timeline: TimelineExport::No,
        }
    }

    /// A `dev_t` reads back as the major and minor it was made of, with
    /// both past their low bits.
    #[test]
    fn a_dev_t_reads_back_its_major_and_minor() {
        assert_eq!(
            dev(Major(226), Minor(128)),
            Device(0xe280),
            "makedev(226, 128)"
        );
        assert_eq!(
            Node::of(dev(Major(226), Minor(128))),
            node(Major(226), Minor(128))
        );
        assert_eq!(
            Node::of(dev(Major(4097), Minor(300_000))),
            node(Major(4097), Minor(300_000))
        );
    }

    /// **The compositor's device is taken, by either of its nodes**: a
    /// main device naming the render node, or the primary, picks the GPU
    /// with it among two and the lavapipe beside them; the first offered
    /// format whose modifier it renders, tranche by tranche, ARGB before
    /// XRGB; explicit sync with the manager and a timeline to export.
    #[test]
    fn the_compositors_device_is_taken_by_either_node() {
        let other = gpu(
            node(Major(226), Minor(1)),
            node(Major(226), Minor(129)),
            vec![TILED],
        );
        let ours = gpu(
            node(Major(226), Minor(0)),
            node(Major(226), Minor(128)),
            vec![TILED, Modifier::LINEAR],
        );
        let cands = [other, ours, cpu()];
        let offered = feedback(
            Some(dev(Major(226), Minor(128))),
            vec![
                vec![fmt(Fourcc::XRGB8888, DCC), fmt(Fourcc::ARGB8888, DCC)],
                vec![fmt(Fourcc::XRGB8888, TILED), fmt(Fourcc::ARGB8888, TILED)],
            ],
        );
        assert_eq!(
            choose(&cands, &offered, SyncObj::Offered),
            Ok(Choice {
                device: DeviceIx(1),
                format: fmt(Fourcc::ARGB8888, TILED),
                sync: Sync::Explicit,
            })
        );
        let by_primary = feedback(
            Some(dev(Major(226), Minor(0))),
            offered.tranches.iter().map(|t| t.formats.clone()).collect(),
        );
        assert_eq!(
            choose(&cands, &by_primary, SyncObj::Offered).map(|c| c.device),
            Ok(DeviceIx(1))
        );
    }

    /// **Without the manager, or without a timeline to export, sync is
    /// implicit**; and a device the timeline check refused is told as one
    /// without (`TimelineExport::No`), the branch a fd that is not a
    /// syncobj takes.
    #[test]
    fn sync_is_implicit_without_the_manager_or_a_timeline() {
        let ours = gpu(
            node(Major(226), Minor(0)),
            node(Major(226), Minor(128)),
            vec![Modifier::LINEAR],
        );
        let offered = feedback(
            Some(dev(Major(226), Minor(128))),
            vec![vec![fmt(Fourcc::ARGB8888, Modifier::LINEAR)]],
        );
        let sync = |c: &Candidate, s| choose(core::slice::from_ref(c), &offered, s).map(|c| c.sync);
        assert_eq!(sync(&ours, SyncObj::Absent), Ok(Sync::Implicit));
        let no_timeline = Candidate {
            timeline: TimelineExport::No,
            ..ours.clone()
        };
        assert_eq!(sync(&no_timeline, SyncObj::Offered), Ok(Sync::Implicit));
        assert_eq!(sync(&ours, SyncObj::Offered), Ok(Sync::Explicit));
    }

    /// **Each way the GPU path is refused, by name**: no main device and
    /// no tranche's; a GPU that is not the compositor's beside a lavapipe
    /// that does not stand alone; a GPU that cannot say its nodes; the
    /// compositor's GPU without an extension; and no shared modifier.
    #[test]
    fn each_refusal_is_named() {
        let ours = gpu(
            node(Major(226), Minor(0)),
            node(Major(226), Minor(128)),
            vec![TILED],
        );
        let tiled = vec![vec![fmt(Fourcc::ARGB8888, TILED)]];
        let at = |main| feedback(main, tiled.clone());
        let mut none = at(None);
        for t in &mut none.tranches {
            t.device = None;
        }
        assert_eq!(
            choose(core::slice::from_ref(&ours), &none, SyncObj::Offered),
            Err(Refusal::NoMainDevice)
        );
        assert_eq!(
            choose(
                &[ours.clone(), cpu()],
                &at(Some(dev(Major(226), Minor(129)))),
                SyncObj::Offered
            ),
            Err(Refusal::NoDevice)
        );
        let silent = Candidate {
            kind: Kind::Gpu {
                primary: None,
                render: None,
            },
            ..ours.clone()
        };
        assert_eq!(
            choose(
                &[silent],
                &at(Some(dev(Major(226), Minor(128)))),
                SyncObj::Offered
            ),
            Err(Refusal::NoDevice)
        );
        let missing = Candidate {
            extensions: Extensions::Missing("VK_EXT_queue_family_foreign"),
            ..ours.clone()
        };
        assert_eq!(
            choose(
                &[missing],
                &at(Some(dev(Major(226), Minor(128)))),
                SyncObj::Offered
            ),
            Err(Refusal::Missing("VK_EXT_queue_family_foreign"))
        );
        let dcc_only = feedback(
            Some(dev(Major(226), Minor(128))),
            vec![vec![fmt(Fourcc::ARGB8888, DCC)]],
        );
        assert_eq!(
            choose(&[ours], &dcc_only, SyncObj::Offered),
            Err(Refusal::NoModifier)
        );
    }

    /// **A lavapipe alone is taken, on LINEAR, with implicit sync**, and the
    /// main device it cannot match does not refuse it; a tranche main
    /// device is read where the main one is missing, as from version 6.
    #[test]
    fn a_lone_cpu_device_is_taken_on_linear() {
        let offered = feedback(
            Some(dev(Major(226), Minor(128))),
            vec![vec![
                fmt(Fourcc::ARGB8888, TILED),
                fmt(Fourcc::ARGB8888, Modifier::LINEAR),
            ]],
        );
        assert_eq!(
            choose(&[cpu()], &offered, SyncObj::Offered),
            Ok(Choice {
                device: DeviceIx(0),
                format: fmt(Fourcc::ARGB8888, Modifier::LINEAR),
                sync: Sync::Implicit,
            })
        );
        let mut tranche_only = feedback(None, vec![vec![fmt(Fourcc::ARGB8888, TILED)]]);
        tranche_only.tranches[0].device = Some(dev(Major(226), Minor(128)));
        let ours = gpu(
            node(Major(226), Minor(0)),
            node(Major(226), Minor(128)),
            vec![TILED],
        );
        assert_eq!(
            choose(&[ours], &tranche_only, SyncObj::Absent).map(|c| c.device),
            Ok(DeviceIx(0))
        );
    }
}
