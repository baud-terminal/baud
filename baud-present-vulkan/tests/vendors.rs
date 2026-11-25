//! **The vendor matrix**: each Vulkan device this machine has, as the GPU
//! path's choice reads it, held to its driver's committed table, and each
//! fallback the choice makes forced on it by a mask.
//!
//! **The gate.** `tests/vendors/<driver>.txt` holds a driver's row
//! (`vk::Row::lines`). A live row that differs from its file fails and
//! prints both; a driver with no file fails and prints the row to commit. So
//! a new driver, or an update that moves a capability, is a reviewed diff
//! and never passes unseen. The drivers come from the development shell's
//! pinned Mesa (`VK_DRIVER_FILES` in devenv.nix), so a row moves on a
//! deliberate update of the pin, not on a host's own Mesa. The rows: RADV
//! and lavapipe, on this machine; an integrated GPU on unified memory is a
//! RADV row too, pending a run on one.
//!
//! **The masks.** On each live device alone, `choose` is asked again with
//! one capability taken away, and must fall back as the path says: implicit
//! sync without syncobj or a timeline to export, a refusal without render
//! nodes (a GPU), without an exportable modifier, or with a required
//! extension missing;
//! LINEAR when it is the only modifier.

#![cfg(target_os = "linux")]

use baud_present_vulkan::choose::{
    Candidate, Choice, Extensions, Kind, Refusal, Sync, SyncObj, TimelineExport, choose,
};
use baud_present_vulkan::vk::{Row, Vulkan};
use baud_present_wayland::dmabuf::{
    Count, Feedback, Format, Fourcc, Modifier, Tranche, TrancheFlags,
};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vendors")
}

fn rows() -> Vec<Row> {
    let v = Vulkan::open().expect("Vulkan: the development shell's loader");
    let rows = v.rows();
    assert!(
        rows.iter().any(|r| r.driver == "MESA_LLVMPIPE"),
        "the control: lavapipe is among the devices"
    );
    rows
}

/// **Where `got` differs from the committed `want`**, as a diff of lines,
/// or `None`.
fn differs(want: &str, got: &[String]) -> Option<String> {
    let want: Vec<&str> = want.lines().collect();
    if want == got.iter().map(String::as_str).collect::<Vec<_>>() {
        return None;
    }
    let mut out = String::new();
    for w in &want {
        if !got.iter().any(|g| g == w) {
            out += &format!("-{w}\n");
        }
    }
    for g in got {
        if !want.contains(&g.as_str()) {
            out += &format!("+{g}\n");
        }
    }
    Some(out)
}

/// **Each live device's row is its driver's committed table**: one that
/// differs is printed as a diff, one with no file as the row to commit.
#[test]
fn each_device_is_its_drivers_committed_row() {
    let mut problems = Vec::new();
    for row in rows() {
        let lines = row.lines();
        let path = dir().join(format!("{}.txt", row.driver));
        let count = row.candidate.modifiers.len();
        match std::fs::read_to_string(&path) {
            Ok(want) => {
                if let Some(diff) = differs(&want, &lines) {
                    problems.push(format!(
                        "{} differs from {} ({count} modifiers):\n{diff}",
                        row.driver,
                        path.display()
                    ));
                }
            }
            Err(_) => problems.push(format!(
                "{} has no table: commit {} ({count} modifiers) as\n{}\n",
                row.driver,
                path.display(),
                lines.join("\n")
            )),
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// **A change the table does not compare passes the gate**: the count of
/// modifiers, which moves with a driver's version, is printed and not
/// compared, so a row with one more modifier, LINEAR still among them, is
/// the same row. The control: one with none differs.
#[test]
fn the_modifier_count_is_printed_not_compared() {
    let row = rows().into_iter().next().expect("a device");
    let want = row.lines().join("\n");
    let mut more = row.clone();
    more.candidate
        .modifiers
        .push(Modifier(0x00ff_0000_0000_0001));
    assert_eq!(differs(&want, &more.lines()), None, "one more modifier");
    let mut none = row;
    none.candidate.modifiers.clear();
    assert!(
        differs(&want, &none.lines()).is_some(),
        "no modifier at all"
    );
}

/// What the compositor offers `c`: every modifier it exports, and LINEAR,
/// in ARGB8888, on its render node when it is a GPU.
fn offer(c: &Candidate) -> Feedback {
    let main = match c.kind {
        Kind::Gpu { render, primary } => render.or(primary).map(|n| n.dev()),
        Kind::Cpu => None,
    };
    let mut formats: Vec<Format> = c
        .modifiers
        .iter()
        .map(|&modifier| Format {
            fourcc: Fourcc::ARGB8888,
            modifier,
        })
        .collect();
    formats.push(Format {
        fourcc: Fourcc::ARGB8888,
        modifier: Modifier::LINEAR,
    });
    Feedback {
        main,
        tranches: vec![Tranche {
            device: main,
            flags: TrancheFlags(0),
            formats,
        }],
        unknown_indices: Count(0),
    }
}

/// `c` chosen alone, from its own offer.
fn alone(c: &Candidate, offered: &Feedback, syncobj: SyncObj) -> Result<Choice, Refusal> {
    choose(std::slice::from_ref(c), offered, syncobj)
}

/// **Each fallback row, forced by a mask on each live device**: the device
/// as it is is chosen, and with one capability taken away the choice falls
/// back, or refuses, as the path says.
#[test]
fn each_fallback_is_forced_by_a_mask_on_each_device() {
    for row in rows() {
        let c = row.candidate.clone();
        let offered = offer(&c);
        let at = |what: &str| format!("{}: {what}", row.driver);
        let base = alone(&c, &offered, SyncObj::Offered)
            .unwrap_or_else(|e| panic!("{}", at(&format!("chosen as it is, not {e:?}"))));
        let explicit = match c.timeline {
            TimelineExport::Yes => Sync::Explicit,
            TimelineExport::No => Sync::Implicit,
        };
        assert_eq!(base.sync, explicit, "{}", at("syncobj and its timeline"));
        let sync = |c: &Candidate, s: SyncObj| alone(c, &offer(c), s).map(|ch| ch.sync);
        assert_eq!(
            sync(&c, SyncObj::Absent),
            Ok(Sync::Implicit),
            "{}",
            at("no syncobj")
        );
        let no_timeline = Candidate {
            timeline: TimelineExport::No,
            ..c.clone()
        };
        assert_eq!(
            sync(&no_timeline, SyncObj::Offered),
            Ok(Sync::Implicit),
            "{}",
            at("no timeline export")
        );
        if let Kind::Gpu { .. } = c.kind {
            let silent = Candidate {
                kind: Kind::Gpu {
                    primary: None,
                    render: None,
                },
                ..c.clone()
            };
            assert_eq!(
                alone(&silent, &offered, SyncObj::Offered),
                Err(Refusal::NoDevice),
                "{}",
                at("no render node")
            );
        }
        let none = Candidate {
            modifiers: Vec::new(),
            ..c.clone()
        };
        assert_eq!(
            alone(&none, &offered, SyncObj::Offered),
            Err(Refusal::NoModifier),
            "{}",
            at("no exportable modifier")
        );
        let linear = Candidate {
            modifiers: vec![Modifier::LINEAR],
            ..c.clone()
        };
        assert_eq!(
            alone(&linear, &offered, SyncObj::Offered).map(|ch| ch.format.modifier),
            Ok(Modifier::LINEAR),
            "{}",
            at("LINEAR only")
        );
        let missing = Candidate {
            extensions: Extensions::Missing("VK_EXT_queue_family_foreign"),
            ..c
        };
        assert_eq!(
            alone(&missing, &offered, SyncObj::Offered),
            Err(Refusal::Missing("VK_EXT_queue_family_foreign")),
            "{}",
            at("a required extension missing")
        );
    }
}
