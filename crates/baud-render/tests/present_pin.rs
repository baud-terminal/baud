//! **The frame-cost harness's drivers set the renderer as `App::present`
//! does, both ways** (FS-C0): `baud-present-macos`'s `tests/caret_frame.rs`
//! counts what a frame costs through two drivers, `cell_frame` for the
//! cell path and `cpu_frame` for the CPU path, and its own pin
//! (`the_cell_driver_builds_as_present_builds`) holds every call a driver
//! makes into the renderer to one `present` makes, in `present`'s order.
//! That is one way. This is the other: every call `present` makes into the
//! renderer before it builds the frame (`cell_frame::build`) that sets
//! something -- a method of the renderer's taking `&mut self` -- is made by
//! each driver too, or is named here with what the harness leaves out by
//! not making it. A setter added to `present` and not to the drivers is a
//! frame the harness no longer counts as the app draws it, and fails
//! here; so does a setter named here that `present` no longer makes, or
//! that a driver now makes, so the list stays the difference. A call that
//! only reads -- a method taking `&self` -- sets nothing, and needs no
//! name.
//!
//! Its controls: each driver's calls and `present`'s are read, more than
//! ten of each; and the check is run on `present` with a setter spliced in
//! before the build, which it names, and with a named setter taken out,
//! which it names too.

use std::collections::BTreeSet;

/// A method's body, closed at four spaces' indent.
const METHOD: &str = "\n    }\n";

/// What the frame is built with in `present`: the calls before it are the
/// ones a driver must make.
const BUILD: &str = "cell_frame::build_with(";

/// The code of `name`'s body in `src`, its comment lines dropped and its
/// whitespace removed: from its signature to the first `close` after it,
/// as `caret_frame.rs` reads it.
fn body_of(src: &str, name: &str, close: &str) -> String {
    let at = src
        .find(name)
        .unwrap_or_else(|| panic!("{name} is in its file"));
    let end = at + src[at..].find(close).expect("a body");
    src[at..end]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .flat_map(str::split_whitespace)
        .collect()
}

/// The methods called on `receiver` in `flat`, by name: the receiver is
/// not the tail of a longer path, as `caret_frame.rs` reads a call.
fn calls(flat: &str, receiver: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let bytes = flat.as_bytes();
    for (i, _) in flat.match_indices(receiver) {
        let before = i.checked_sub(1).map(|j| bytes[j]);
        if before.is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.') {
            continue;
        }
        let rest = &flat[i + receiver.len()..];
        let n = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        if n > 0 && rest[n..].starts_with('(') {
            out.insert(rest[..n].to_string());
        }
    }
    out
}

/// **What each driver leaves out of `present`'s frame by not setting it**:
/// a setter `present` makes before the build and a driver does not, and
/// why. Every one is left at what `Renderer::new` set.
const LEFT_OUT: [(&str, &str); 12] = [
    ("set_cursor_guide", "no cursor guide"),
    ("set_badge", "no badge"),
    ("set_find", "no find field and no find marks"),
    ("set_selection_pulse", "no selection pulse"),
    ("set_scrollbar", "no scrollbar"),
    ("set_screen_fade", "no fade between screens"),
    ("set_splash", "no splash"),
    ("set_fireworks", "no fireworks"),
    (
        "set_composition",
        "no composition, which the cell driver hands the build as none",
    ),
    ("set_margins", "no side margins"),
    ("set_flash", "no bell flash"),
    ("set_bell_tint", "no bell tint"),
];

/// **What the CPU driver alone leaves out**: `present` places the images
/// for the two GPU arms alone, and the CPU arm's `render_at` places them
/// itself, which the CPU driver calls.
const CPU_LEFT_OUT: [(&str, &str); 1] = [("set_images", "placed by render_at on the CPU arm")];

/// Whether `name` is a method of the renderer that only reads: one taking
/// `&self` in `renderer`, the renderer's source.
fn reads(renderer: &str, name: &str) -> bool {
    renderer.contains(&format!("pub fn {name}(&self"))
}

/// **What is wrong with `driver` against `present`'s calls before the
/// build**, `left_out` the setters it is allowed not to make: a setter
/// made there and not by the driver, nor named; a name for a setter
/// `present` does not make there, or the driver makes, or that only
/// reads. Empty when the two agree.
fn disagree(
    present: &BTreeSet<String>,
    driver: &BTreeSet<String>,
    left_out: &[(&str, &str)],
    renderer: &str,
) -> Vec<String> {
    let named: BTreeSet<&str> = left_out.iter().map(|(n, _)| *n).collect();
    let mut wrong = Vec::new();
    for call in present {
        if !driver.contains(call) && !named.contains(call.as_str()) && !reads(renderer, call) {
            wrong.push(format!(
                "present sets {call} before the build and the driver does not"
            ));
        }
    }
    for (name, why) in left_out {
        if !present.contains(*name) {
            wrong.push(format!(
                "{name} ({why}) is not set by present before the build"
            ));
        }
        if driver.contains(*name) {
            wrong.push(format!("{name} ({why}) is set by the driver"));
        }
        if reads(renderer, name) {
            wrong.push(format!("{name} ({why}) only reads"));
        }
    }
    wrong
}

/// `p` under the workspace.
fn read(p: &str) -> String {
    std::fs::read_to_string(format!("{}/../../{p}", env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// `present`'s body, flat.
fn present_body(app: &str) -> String {
    body_of(
        app,
        "fn present(&mut self, screen: &DrawableView, show: Show)",
        METHOD,
    )
}

/// The renderer calls in `flat` before the frame is built.
fn before_build(flat: &str) -> BTreeSet<String> {
    let at = flat
        .find(BUILD)
        .unwrap_or_else(|| panic!("present builds the frame: {flat}"));
    calls(&flat[..at], "self.renderer.")
}

/// **Every setter `present` makes before the build, each driver makes or
/// leaves out by name**, and the other way round (`caret_frame.rs`'s
/// own pin): the cell driver against [`LEFT_OUT`], the CPU driver against
/// it and [`CPU_LEFT_OUT`], whose reason is read in `present` too -- it
/// places the images only on the GPU arms. Named break: a setter added to
/// `present` before the build and to no driver, here spliced into the
/// read body.
#[test]
fn every_setter_present_makes_before_the_build_each_driver_makes() {
    let app = read("crates/baud-app/src/app.rs");
    let harness = read("crates/baud-present-macos/tests/caret_frame.rs");
    let renderer = read("crates/baud-render/src/renderer.rs");
    let flat = present_body(&app);
    let present = before_build(&flat);
    assert!(
        present.len() > 10,
        "present's calls before the build are read: {present:?}"
    );
    assert!(
        flat.contains("==Some(Mode::Gpu){self.renderer.set_images("),
        "present places the images on the GPU arms alone: {flat}"
    );
    let cpu_left: Vec<(&str, &str)> = LEFT_OUT.iter().chain(&CPU_LEFT_OUT).copied().collect();
    for (path, name, left_out) in [
        ("cell", "fn cell_frame(", &LEFT_OUT[..]),
        ("cpu", "fn cpu_frame(", &cpu_left[..]),
    ] {
        let driver = calls(&body_of(&harness, name, METHOD), "r.");
        assert!(
            driver.len() > 10,
            "the {path} driver's calls are read: {driver:?}"
        );
        let wrong = disagree(&present, &driver, left_out, &renderer);
        assert!(
            wrong.is_empty(),
            "the {path} driver against present: {wrong:#?}"
        );
        // The control, red first: a setter spliced into present before
        // the build, and one it names taken out of present.
        let at = flat.find(BUILD).expect("the build");
        let spliced = format!("{}self.renderer.set_probe(x);{}", &flat[..at], &flat[at..]);
        let wrong = disagree(&before_build(&spliced), &driver, left_out, &renderer);
        assert!(
            wrong.iter().any(|w| w.contains("present sets set_probe")),
            "the control: a setter added to present alone is named: {wrong:?}"
        );
        let taken = flat.replace("self.renderer.set_badge(", "self.other.set_badge(");
        let wrong = disagree(&before_build(&taken), &driver, left_out, &renderer);
        assert!(
            wrong.iter().any(|w| w.starts_with("set_badge")),
            "the control: a setter named and no longer made is named: {wrong:?}"
        );
    }
}
