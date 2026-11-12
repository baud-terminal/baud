//! **What linking a newer macOS SDK changes, held where it reaches baud**:
//! one test for each change Apple gates on the linked SDK that baud's
//! window could feel, each citing the sentence that gates it. None makes a
//! window, a control or an application: each holds its point by pure
//! geometry over the functions the app places by, or by a pin on the
//! source, so each holds whatever SDK the host links. Where Apple says
//! what changes but not by how much, the test holds that baud reads the
//! number from AppKit rather than assuming the old one, and does not guess
//! the new one. The About window's button has its own test beside its
//! layout (`baud-present-macos/src/about.rs`), and the event rig's
//! notification row says what the rig saw.
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use baud_present_macos::full_screen::Change;
use baud_render::{Renderer, Theme};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::mark::{Lockup, Place, beside_at};
use crate::strip::grid_top;

/// The crate's own directory, and its sibling's.
fn tree() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A source file of this crate, read whole.
fn source(name: &str) -> String {
    let at = tree().join("src").join(name);
    std::fs::read_to_string(&at).unwrap_or_else(|e| panic!("{}: {e}", at.display()))
}

/// Every `.rs` file under `dir`, with its text.
fn sources(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut found = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let entries = std::fs::read_dir(&d).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                todo.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&path).expect("a source file");
                found.push((path, text));
            }
        }
    }
    found
}

/// The app's sources and the presentation crate's, where every AppKit
/// call baud makes is.
fn app_sources() -> Vec<(PathBuf, String)> {
    let mut all = sources(&tree().join("src"));
    all.extend(sources(&tree().join("../baud-present-macos/src")));
    all
}

/// The body of `fn name(` in `text`, to the first line that closes it.
fn body<'t>(text: &'t str, name: &str) -> &'t str {
    let from = text
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("no fn {name}"));
    let rest = &text[from..];
    let end = rest.find("\n}\n").map_or(rest.len(), |e| e + 3);
    &rest[..end]
}

/// **The grid starts under the title bar AppKit draws, whatever height
/// that is**: linked against an SDK before 26 the window ran in Apple's
/// compatibility mode, "the app as it looks when built against previous
/// versions of the SDKs", and its absence "is the default value for apps
/// linking against the latest SDKs"
/// ([[source: apple-uidesignrequirescompatibility]]); "windows adopt
/// rounder corners to fit controls and navigation elements"
/// ([[source: apple-adopting-liquid-glass-2026-11-09]]). The title bar
/// went from 28 points to 32 with it. So the inset the layout uses is the
/// window's own, `strip::inset_of` (its content less its content layout
/// rect), checked against the class method's prediction, and no height is
/// written into the code; and the grid's top is that inset at 28, 32 and a
/// height no SDK has drawn yet. Break: `let inset = 28.0;` where the app
/// takes the measured inset, and the pin fails.
#[test]
fn the_title_bar_inset_is_read_from_appkit_not_assumed() {
    let app = source("app.rs");
    let strip = source("strip.rs");
    assert!(
        app.contains("let measured = crate::strip::inset_of(window.ns_window());")
            && app.contains("let inset = measured;"),
        "the layout's inset is not the window's measured one"
    );
    let literal = app.lines().find(|l| {
        let l = l.trim_start();
        l.starts_with("let inset = ") && l[12..].starts_with(|c: char| c.is_ascii_digit())
    });
    assert_eq!(literal, None, "a title bar height written in");
    assert!(
        body(&app, "predicted_inset").contains("frameRectForContentRect_styleMask"),
        "the prediction is not AppKit's"
    );
    assert!(
        body(&strip, "inset_of").contains("contentLayoutRect()"),
        "the inset is not read off the content layout rect"
    );
    for inset in [28.0, 32.0, 40.0] {
        assert_eq!(grid_top(Change::Exited, 600.0, 600.0 - inset), inset);
    }
}

/// The lights' columns at `scale`, as the strip lays them out.
fn columns_at(scale: CGFloat) -> crate::lights::Columns {
    let faces = [baud_font::Face::parse(crate::embedded::BOLD.bytes).expect("the Bold")];
    let r = Renderer::new(
        baud_render::Stack::new(&faces).expect("a stack"),
        baud_render::PxSize::of(28.0).expect("a size"),
        Theme::dark(),
        baud_render::Blend::Native,
        baud_domain::Cols::of_index(6),
        baud_domain::Rows::of_index(2),
    )
    .expect("a renderer");
    crate::lights::columns(&r, scale).expect("the columns")
}

/// Whether `inner` lies within `outer`.
fn within(inner: NSRect, outer: NSRect) -> bool {
    inner.origin.x >= outer.origin.x
        && inner.origin.y >= outer.origin.y
        && inner.origin.x + inner.size.width <= outer.origin.x + outer.size.width
        && inner.origin.y + inner.size.height <= outer.origin.y + outer.size.height
}

/// **The lockup lies inside the accessory it is drawn in, at every title
/// bar height**: "When an app is linked on macOS 27.0 or later,
/// NSTitlebarAccessoryViewController is now allowed to draw outside its
/// bounds by default" ([[source: apple-macos-27-release-notes]]), so what
/// AppKit clipped before now shows. Beside the traffic lights, in a leading
/// accessory as wide as the lockup and as tall as the bar, from 16 points
/// to 44, at 1x and 2x, beside 16- and 14-point buttons; and on the plate,
/// in a 28- and a 32-point bar, at 786 and 1512 points wide, the lockup
/// inside the plate and the plate inside the bar. Break: the lockup
/// centred against a fixed 28-point bar, and in the bars under 20 points
/// its top passes the accessory's.
#[test]
fn each_lockup_draws_inside_its_accessory() {
    let art = Lockup::OnDark.art().expect("the lockup's size");
    let button = |x: CGFloat, h: CGFloat| NSRect::new(NSPoint::new(x, 6.0), NSSize::new(14.0, h));
    for scale in [1.0, 2.0] {
        for height in [16.0, 14.0] {
            for inset in (16..=44).map(CGFloat::from) {
                let place = Place::beside(
                    button(9.0, height),
                    button(29.0, height),
                    button(49.0, height),
                    inset,
                    art,
                )
                .expect("laid out");
                let at = beside_at(inset, place.size(), scale);
                let accessory =
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(at.size.width, inset));
                assert!(
                    within(at, accessory),
                    "{inset}-point bar, {height}-point buttons, {scale}x: {at:?} in {accessory:?}"
                );
            }
        }
        let columns = columns_at(scale);
        let place = Place::fallback(28.0, art);
        for inset in [28.0, 32.0] {
            for width in [786.0, 1512.0] {
                let p = crate::lights::panel(
                    NSSize::new(width, inset),
                    inset,
                    78.0,
                    CGFloat::from(baud_render::lights::PANEL_RIGHT),
                    scale,
                    &columns,
                    place.size(),
                )
                .expect("a light fits");
                let bar = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, inset));
                let at = format!("{inset}-point bar, {width} wide, {scale}x");
                assert!(
                    within(p.logo, p.plate),
                    "{at}: {:?} on {:?}",
                    p.logo,
                    p.plate
                );
                assert!(within(p.plate, bar), "{at}: {:?} in {bar:?}", p.plate);
            }
        }
    }
}

/// **The window has no toolbar, so its accessories stay in the title bar
/// baud measures**: "For applications linked on macOS 27.0 or higher,
/// leading and trailing accessories are positioned within the titlebar area
/// only (excluding the toolbar) when the window uses
/// NSWindowToolbarStyleExpanded or NSWindowToolbarStylePreference, or when
/// the window has no toolbar"
/// ([[source: macos-sdk-nstitlebaraccessoryviewcontroller-h]] line 31).
/// With no toolbar the title bar area is the whole band above the content
/// layout rect, the inset the grid starts under, so where AppKit puts them
/// does not move with the link. The two accessories are a leading and a
/// trailing one, made in one place (`strip::Accessory::put`), and nothing in
/// the app or the presentation crate names a toolbar. The window crate,
/// macos-liquid-glass, names none at the version `Cargo.lock` pins; a test
/// cannot read a registry's copy without depending on the machine, so it
/// is not read here. Break: a toolbar named anywhere in either crate's
/// source, and the pin names the file.
#[test]
fn no_toolbar_so_the_accessories_stay_in_the_title_bar() {
    let toolbar = ["NS", "Toolbar"].concat();
    let set = ["set", "Toolbar"].concat();
    let made = ["add_titlebar", "_accessory("].concat();
    let mut makers = Vec::new();
    for (path, text) in app_sources() {
        assert!(
            !text.contains(&toolbar) && !text.contains(&set),
            "{} names a toolbar",
            path.display()
        );
        let calls = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//") && l.contains(&made))
            .count();
        if calls > 0 {
            makers.push((
                path.file_name().map(|n| n.to_string_lossy().into_owned()),
                calls,
            ));
        }
    }
    assert_eq!(
        makers,
        [(Some("strip.rs".to_owned()), 1)],
        "where accessories are made"
    );
    let strip = source("strip.rs");
    let put = body(&strip, "put");
    assert!(
        put.contains("side: TitlebarSide"),
        "an accessory not leading or trailing"
    );
}

/// **The general pasteboard is read in one place, `paste`, which a paste
/// command reaches**: "Prepare your app for an upcoming feature in macOS
/// that alerts a person using a device when your app programmatically
/// reads the general pasteboard. The system shows the alert only if the
/// pasteboard access wasn't a result of someone's input on a UI element
/// that the system considers paste-related"
/// ([[source: apple-updates-appkit]]). No fetched source gates it on the
/// linked SDK or names the release that turns it on. `clipboard::read_text`
/// is the one read of the general pasteboard's string, and `paste`, which
/// the `paste:` selector reaches from the Edit menu or from a Command-V the
/// menu did not take, is its one caller; the find pasteboard's read is the
/// other `stringForType`, and is not the general one. `OSC 52` reads are
/// refused, so no program makes baud read it.
///
/// **Why the alert itself is not tested here**: it is the system's, shown
/// on screen over a running application after it judges the input that
/// led to the read, and a headless test has no application, no event and
/// no screen for it to judge or show. Whether it counts the Command-V
/// fallback as paste-related is not stated. Break: a second call of
/// `clipboard::read_text`, and the count fails.
#[test]
fn the_general_pasteboard_is_read_only_from_paste() {
    let read = ["clipboard::read", "_text("].concat();
    let strings = ["stringFor", "Type("].concat();
    let mut callers = Vec::new();
    let mut reads = Vec::new();
    for (path, text) in app_sources() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut func = String::new();
        for line in text.lines() {
            let l = line.trim_start();
            if l.starts_with("//") {
                continue;
            }
            if let Some(at) = l.find("fn ") {
                let rest = &l[at + 3..];
                func = rest.split('(').next().unwrap_or_default().to_owned();
            }
            if l.contains(&read) {
                callers.push((name.clone(), func.clone()));
            }
            if l.contains(&strings) {
                reads.push((name.clone(), func.clone()));
            }
        }
    }
    assert_eq!(callers, [("app.rs".to_owned(), "paste".to_owned())]);
    reads.sort();
    assert_eq!(
        reads,
        [
            ("clipboard.rs".to_owned(), "read_find".to_owned()),
            ("clipboard.rs".to_owned(), "read_text".to_owned()),
        ]
    );
}

/// Where a source names a selector: each line outside a comment that does,
/// by file, the function it is in, and whether it is a call (`msg_send!`).
fn named(sources: &[(PathBuf, String)], selector: &str) -> Vec<(String, String, bool)> {
    let mut found = Vec::new();
    for (path, text) in sources {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut func = String::new();
        for line in text.lines() {
            let l = line.trim_start();
            if l.starts_with("//") {
                continue;
            }
            if let Some(at) = l.find("fn ") {
                let rest = &l[at + 3..];
                func = rest.split('(').next().unwrap_or_default().to_owned();
            }
            if l.contains(selector) {
                found.push((name.clone(), func.clone(), l.contains("msg_send!")));
            }
        }
    }
    found
}

/// **The one guarded call of AppKit's private `_zoomFill:`**, in `sources`:
/// the function it is called from, if the selector is called exactly once,
/// named in no other function, and asked about with `respondsToSelector`
/// on a line before the call; else what is wrong.
fn zoom_fill_site(sources: &[(PathBuf, String)]) -> Result<(String, String), String> {
    let selector = ["_zoom", "Fill:"].concat();
    let guard = ["responds", "ToSelector"].concat();
    let found = named(sources, &selector);
    let calls: Vec<_> = found.iter().filter(|f| f.2).collect();
    let [call] = calls.as_slice() else {
        return Err(format!("{} calls: {found:?}", calls.len()));
    };
    let site = (call.0.clone(), call.1.clone());
    if let Some(other) = found.iter().find(|f| (&f.0, &f.1) != (&site.0, &site.1)) {
        return Err(format!("named outside {site:?}: {other:?}"));
    }
    let (_, text) = sources
        .iter()
        .find(|(p, _)| p.file_name().is_some_and(|n| n.to_string_lossy() == site.0))
        .ok_or("the site's file")?;
    let body = body(text, &site.1);
    let call_at = body
        .lines()
        .position(|l| l.contains(&selector) && l.contains("msg_send!"));
    let guard_at = body
        .lines()
        .position(|l| !l.trim_start().starts_with("//") && l.contains(&guard));
    match (guard_at, call_at) {
        (Some(g), Some(c)) if g < c => Ok(site),
        _ => Err(format!("{site:?}: the call is not after {guard}")),
    }
}

/// **AppKit's private `_zoomFill:`, the title bar's Fill, is called at one
/// site, and only after `respondsToSelector:`**: no header names it, so it
/// is called only where the window says it has it, as Chromium
/// ([[source: chromium-native-widget-mac-nswindow-e44f559]] 778-780),
/// Firefox ([[source: firefox-nscocoautils-293c306]] 1259-1261) and iTerm2
/// ([[source: iterm2-nswindow-iterm-2a8d67e]] 40-42) call it, and
/// `container::fill` is that one site. Positive control: the site is found
/// in the sources, in `container.rs`'s `fill`. Named breaks, each built
/// from the real sources and each refused: a second call in another
/// function; the guard taken out; the guard moved after the call.
#[test]
fn the_private_fill_is_called_once_and_guarded() {
    let all = app_sources();
    assert_eq!(
        zoom_fill_site(&all),
        Ok(("container.rs".to_owned(), "fill".to_owned()))
    );
    let container = all
        .iter()
        .position(|(p, _)| p.file_name().is_some_and(|n| n == "container.rs"))
        .expect("container.rs");
    let guard = ["window.responds", "ToSelector(sel!(_zoom", "Fill:))"].concat();
    let call = ["msg_send![window, _zoom", "Fill: None::<&AnyObject>]"].concat();
    let text = &all[container].1;
    assert!(
        text.contains(&guard) && text.contains(&call),
        "the site's lines"
    );
    let with = |change: &dyn Fn(&str) -> String| {
        let mut changed = all.clone();
        changed[container].1 = change(text);
        zoom_fill_site(&changed)
    };
    let second = [
        "fn again(window: &NSWindow) {\n    let () = unsafe { ",
        &call,
        " };\n}\n",
    ]
    .concat();
    assert!(
        with(&|t| format!("{t}\n{second}")).is_err(),
        "a second call"
    );
    assert!(
        with(&|t| t.replace(&guard, "true")).is_err(),
        "the guard taken out"
    );
    let after = |t: &str| {
        let t = t.replace(&guard, "true");
        let at = t.find(&call).expect("the call");
        let line_end = at + t[at..].find('\n').expect("the call's line");
        format!(
            "{}\n        let _ = {guard};{}",
            &t[..line_end],
            &t[line_end..]
        )
    };
    assert!(with(&after).is_err(), "the guard after the call");
}

/// **A zoom is AppKit's standard frame, the screen's**: `zoom:` takes "a
/// 'best fit' frame" from `windowWillUseStandardFrame:defaultFrame:` when
/// the window's delegate or class has it, else "a default frame [that]
/// nearly fills the current screen" ([[source: apple-nswindow-zoom]],
/// [[source: apple-nswindowdelegate-windowwillusestandardframe]]), and
/// resize increments would round what it gets. The title bar's Zoom, and
/// Fill where the window has no `_zoomFill:`, are `performZoom:`
/// (`container.rs`), so neither of these is named in either crate: adding
/// one makes a zoom something else, and this fails to say so. Positive
/// control: the same scan finds `performZoom`, in `container.rs`. Named
/// break: a delegate method of that name added to a source, and it is
/// found.
#[test]
fn a_zoom_is_the_standard_frame() {
    let all = app_sources();
    let frame = ["windowWillUse", "StandardFrame"].concat();
    let increments = ["Resize", "Increments"].concat();
    let lower = ["resize", "Increments"].concat();
    let hits = |sources: &[(PathBuf, String)]| {
        let mut found = named(sources, &frame);
        found.extend(named(sources, &increments));
        found.extend(named(sources, &lower));
        found
    };
    assert_eq!(hits(&all), [], "a standard frame or resize increments");
    let zooms = named(&all, &["perform", "Zoom("].concat());
    assert!(
        zooms.iter().any(|z| z.0 == "container.rs"),
        "the scan finds the zoom: {zooms:?}"
    );
    let mut broken = all.clone();
    broken[0].1.push_str(&format!(
        "\nfn {frame}(window: &NSWindow, default: NSRect) -> NSRect {{ default }}\n"
    ));
    assert_eq!(hits(&broken).len(), 1, "a delegate's standard frame");
}
