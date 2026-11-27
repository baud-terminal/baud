//! **Every stage declares what its pipeline's layout binds**: each
//! resource a pipeline's vertex or fragment stage declares is in its layout
//! ([`INSTANCE_SETS`] or [`CELL_SETS`]) at the same set and binding, of the
//! same kind, visible to that stage; and a stage declares a push constant
//! only on the cell layout, which has one. A stage that declares what its
//! layout does not bind, used or not, is a pipeline a device may refuse.
//! And the cell path's four pipelines build against their layout, on
//! lavapipe and on RADV.

#![cfg(target_os = "linux")]

mod common;
mod spirv;

use ash::vk;
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::pipeline::{
    Blend, CELL_SETS, Descriptor, Fragment, INSTANCE_SETS, Layout, Pipelines, Set,
};
use spirv::{Class, declared};

fn sets(layout: Layout) -> &'static [Set] {
    match layout {
        Layout::Instance => &INSTANCE_SETS,
        Layout::Cell => &CELL_SETS,
    }
}

#[test]
fn every_stage_declares_what_its_layout_binds() {
    let mut wrong = Vec::new();
    for fragment in Fragment::every() {
        let layout = fragment.layout();
        for spv in fragment.spirv() {
            let d = declared(spv);
            let stage = match d.model {
                0 => vk::ShaderStageFlags::VERTEX,
                4 => vk::ShaderStageFlags::FRAGMENT,
                m => panic!("{fragment:?}: execution model {m}"),
            };
            for (set, binding, class) in &d.resources {
                let want = match class {
                    Class::Sampler => Descriptor::Sampler,
                    Class::Uniform => Descriptor::Uniform,
                    Class::Storage => Descriptor::Storage,
                };
                let bound = sets(layout)
                    .iter()
                    .zip(0..)
                    .find(|(_, n)| *n == set.0)
                    .and_then(|(s, _)| s.iter().zip(0..).find(|(_, n)| *n == binding.0))
                    .map(|(b, _)| b);
                match bound {
                    Some((kind, stages)) if *kind == want && stages.contains(stage) => {}
                    _ => wrong.push(format!(
                        "{fragment:?} stage {}: {}.{} {want:?}, layout has {:?}",
                        stage.as_raw(),
                        set.0,
                        binding.0,
                        bound.map(|(k, s)| (k, s.as_raw()))
                    )),
                }
            }
            if d.push && layout != Layout::Cell {
                wrong.push(format!(
                    "{fragment:?} stage {}: a push constant, none in its layout",
                    stage.as_raw()
                ));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The cell path's pipelines, each with the blend it draws with: the
/// backgrounds written in place, the rest over them.
const CELL: [(Fragment, Blend); 4] = [
    (Fragment::CellBg, Blend::Replace),
    (Fragment::CellGlyph, Blend::Normal),
    (Fragment::CellShape, Blend::Normal),
    (Fragment::CellRing, Blend::Normal),
];

fn builds_every_cell_pipeline(gpu: &Gpu) {
    let mut pipes = Pipelines::new(gpu).expect("the pipelines");
    for (fragment, blend) in CELL {
        assert_eq!(fragment.layout(), Layout::Cell);
        pipes
            .get(fragment, blend)
            .unwrap_or_else(|e| panic!("{fragment:?}: {e:?}"));
    }
    pipes.destroy();
}

#[test]
fn every_cell_pipeline_builds_on_lavapipe() {
    builds_every_cell_pipeline(&common::lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: tools/scripts/linux-vulkan-radv.sh"]
fn every_cell_pipeline_builds_on_radv() {
    builds_every_cell_pipeline(&common::radv());
}
