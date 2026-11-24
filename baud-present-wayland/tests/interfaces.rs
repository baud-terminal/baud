//! **The presenter names its interfaces by their generated statics**,
//! never by a string: every interface it creates an object of is one the
//! pinned protocols declare, so `protocol::<name>::INTERFACE` names it at
//! compile time, and a frame's `wl_callback` and presentation feedback cost
//! no walk of the interface table. `Interface::named` is for a name read
//! off the wire, which the presenter never resolves itself.

use std::fs;
use std::path::Path;

#[test]
fn the_presenter_looks_up_no_interface_by_name() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<_> = fs::read_dir(&src)
        .expect("src")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();
    // Positive control: the walk reads the file that creates the frame's
    // callback.
    assert!(
        files.iter().any(|p| p.ends_with("window.rs")),
        "no window.rs under {}",
        src.display()
    );
    let mut found = Vec::new();
    for path in &files {
        let text = fs::read_to_string(path).expect("read");
        for (n, line) in text.lines().enumerate() {
            if line.contains("Interface::named(") {
                found.push(format!(
                    "{}:{}: {}",
                    path.file_name().expect("name").to_string_lossy(),
                    n + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} lookups by name; use protocol::<name>::INTERFACE:\n{}",
        found.len(),
        found.join("\n")
    );
}

/// **The presenter routes an event by its interface and opcode**, a
/// generated `EventKey` matched as integers: `Event::name` and an
/// interface's `name` are strings, and a frame's `wl_callback.done`,
/// presentation feedback and `wl_buffer.release` would each pay a walk of
/// string compares to reach their arm.
#[test]
fn the_presenter_routes_no_event_by_name() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<_> = fs::read_dir(&src)
        .expect("src")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();
    files.sort();
    // Positive control: the walk reads the file whose handler routes the
    // window's events.
    assert!(
        files.iter().any(|p| p.ends_with("window.rs")),
        "no window.rs under {}",
        src.display()
    );
    let compares = |line: &str| {
        line.contains("match ")
            || line.contains("matches!(")
            || line.contains("==")
            || line.contains("!=")
    };
    let mut found = Vec::new();
    for path in &files {
        let text = fs::read_to_string(path).expect("read");
        for (n, line) in text.lines().enumerate() {
            let by_name = line.contains(".name()")
                || line.contains("interface.name")
                || (line.contains("iface.name") && compares(line));
            if by_name {
                found.push(format!(
                    "{}:{}: {}",
                    path.file_name().expect("name").to_string_lossy(),
                    n + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} routes by name; match Event::key against protocol::<name>::on::<EVENT>:\n{}",
        found.len(),
        found.join("\n")
    );
}
