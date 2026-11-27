//! **`frame_gpu`'s `built` arm draws the app's frame**: its block builds
//! each frame through `baud_render::cell_frame::build_with` and draws it
//! through the presenter's `draw_cells`, and its scenes set the overlay
//! scrollbar and the find field on the renderer, so a change to the cell
//! path's build or draw is what the arm times. The bench needs a GPU and
//! a recording, so this reads its source, beside a control that the
//! reading finds the calls in a block that has them and misses one that
//! does not.

/// The `built` arm's block: from its `if let` to the `app` arm's.
fn built_block(src: &str) -> Option<&str> {
    let from = src.find("if let (Arm::Built, Some(m)) = (arm, app.as_mut())")?;
    let to = from + src[from..].find("if let (Arm::App, Some(m)) = (arm, app.as_mut())")?;
    Some(&src[from..to])
}

/// The calls the block must make, each missing one named.
fn missing(block: &str) -> Vec<&'static str> {
    [
        "build_with(",
        ".draw_cells(",
        ".set_scrollbar(",
        ".set_find(",
    ]
    .into_iter()
    .filter(|call| !block.contains(call))
    .collect()
}

#[test]
fn the_built_arm_builds_and_draws_as_the_app_does() {
    // The control: a block with every call reads whole, one without the
    // build reads it missing, and a source with no such arm reads none.
    let with = "if let (Arm::Built, Some(m)) = (arm, app.as_mut()) { build_with(); \
                p.draw_cells(); r.set_scrollbar(); r.set_find(); } \
                if let (Arm::App, Some(m)) = (arm, app.as_mut()) {}";
    assert_eq!(built_block(with).map(missing), Some(vec![]));
    let without = with.replacen("build_with(", "build(", 1);
    assert_eq!(
        built_block(&without).map(missing),
        Some(vec!["build_with("])
    );
    assert_eq!(
        built_block("if let (Arm::App, Some(m)) = (arm, app.as_mut()) {}"),
        None
    );

    let src = include_str!("../benches/frame_gpu.rs");
    let block = built_block(src).expect("frame_gpu has a built arm before the app arm");
    assert_eq!(missing(block), Vec::<&str>::new(), "the built arm's calls");
}
