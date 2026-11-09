//! **No file of the core writes the reply buffer but `outgoing.rs`.** Every
//! reply goes through `Outgoing`'s doors, which frame it in the C1 form
//! S7C1T/S8C1T chose; a raw write is how the OSC answers and XTVERSION
//! sent `ESC ]` and `ESC \` after S8C1T. The fields are private to that
//! module, which the compiler holds; this holds that no other file keeps a
//! buffer, length or count of its own beside it, and that no door takes a
//! C1 form of its caller's choosing.

use std::path::Path;

/// What a file that writes the reply buffer itself names.
const RAW: &[&str] = &[
    "reply_len =",
    "reply_len:",
    "reply_overflows +=",
    "reply_overflows:",
    ".reply[",
    "reply: &'a mut [u8]",
    "reply: &'b mut [u8]",
];

/// The names of `RAW` that `source` uses.
fn raw_in(source: &str) -> Vec<&'static str> {
    RAW.iter().copied().filter(|r| source.contains(r)).collect()
}

#[test]
fn no_file_but_outgoing_writes_the_reply_buffer() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut read = 0;
    let mut wrong = Vec::new();
    let mut dirs = vec![src];
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(&dir).expect("src reads") {
            let p = e.expect("entry").path();
            if p.is_dir() {
                dirs.push(p);
                continue;
            }
            if p.extension().is_none_or(|x| x != "rs") || p.ends_with("outgoing.rs") {
                continue;
            }
            read += 1;
            let found = raw_in(&std::fs::read_to_string(&p).expect("source reads"));
            if !found.is_empty() {
                wrong.push(format!("{}: {found:?}", p.display()));
            }
        }
    }
    assert!(
        read > 50,
        "the premise: the core's files were read ({read})"
    );
    assert!(
        wrong.is_empty(),
        "files that write the reply buffer:\n{}",
        wrong.join("\n")
    );
}

/// The positive control: the check finds each name in a line that uses it.
#[test]
fn the_check_finds_a_raw_write() {
    for (line, name) in [
        ("self.reply_len = ByteLen::from_index(at);", "reply_len ="),
        ("    reply_len: ByteLen,", "reply_len:"),
        ("self.reply_overflows += 1;", "reply_overflows +="),
        ("    pub reply_overflows: usize,", "reply_overflows:"),
        ("self.store.reply[at] = b'\\r';", ".reply["),
        ("    reply: &'a mut [u8],", "reply: &'a mut [u8]"),
        ("        reply: &'b mut [u8],", "reply: &'b mut [u8]"),
    ] {
        assert_eq!(raw_in(line), [name], "{line}");
    }
}

/// The signatures of `source`'s `pub(crate) fn`s, each to its body's `{`,
/// that name a C1 form.
fn doors_naming_c1(source: &str) -> Vec<&str> {
    source
        .split("pub(crate) fn ")
        .skip(1)
        .map(|s| &s[..s.find('{').unwrap_or(s.len())])
        .filter(|sig| sig.contains("C1Transmit"))
        .collect()
}

/// **No door takes a C1 form**: the doors frame in the live one, which
/// `Outgoing::lend` takes from what S7C1T and S8C1T last set, so no caller
/// can pass seven-bit by hand.
#[test]
fn no_door_takes_a_c1_form() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/outgoing.rs");
    let source = std::fs::read_to_string(path).expect("outgoing.rs reads");
    let doors = source.matches("pub(crate) fn ").count();
    assert!(doors > 8, "the premise: the doors were read ({doors})");
    assert!(
        doors_naming_c1(&source).is_empty(),
        "doors that take a C1 form: {:?}",
        doors_naming_c1(&source)
    );
}

/// The positive control: a door with a C1 parameter, on one line or
/// wrapped, is found.
#[test]
fn the_check_finds_a_door_taking_a_c1_form() {
    for sig in [
        "    pub(crate) fn csi(&mut self, c1: C1Transmit, rest: &[u8]) -> bool {",
        "    pub(crate) fn report(\n        &mut self,\n        c1: C1Transmit,\n    ) -> bool {",
    ] {
        assert_eq!(doors_naming_c1(sig).len(), 1, "{sig}");
    }
}

/// The arguments of `source`'s `.lend(` calls that are not the terminal's
/// own state, `self.cold` or `&self.cold`.
fn lends_not_of_the_terminal(source: &str) -> Vec<&str> {
    source
        .split(".lend(")
        .skip(1)
        .map(|s| &s[..s.find(')').unwrap_or(s.len())])
        .filter(|arg| !matches!(*arg, "self.cold" | "&self.cold"))
        .collect()
}

/// **The doors read the terminal's own C1 form**: `Outgoing::lend`
/// borrows a `Cold`, so a hand-built `Replies` cannot reach it, and every
/// call lends the terminal's own, never one built for the call.
#[test]
fn every_lend_is_of_the_terminals_own_state() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let (mut lends, mut wrong) = (0, Vec::new());
    for e in std::fs::read_dir(&src).expect("src reads") {
        let p = e.expect("entry").path();
        if p.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let source = std::fs::read_to_string(&p).expect("source reads");
        lends += source.matches(".lend(").count();
        for arg in lends_not_of_the_terminal(&source) {
            wrong.push(format!("{}: lend({arg})", p.display()));
        }
    }
    assert!(lends > 20, "the premise: the lends were read ({lends})");
    assert!(
        wrong.is_empty(),
        "lends of another state:\n{}",
        wrong.join("\n")
    );
}

/// The positive control: a lend of a state built for the call is found,
/// and the terminal's own is not.
#[test]
fn the_check_finds_a_lend_of_another_state() {
    let line = "self.reply.lend(&Cold::new(rows, cols)).report(r);";
    assert_eq!(lends_not_of_the_terminal(line), ["&Cold::new(rows, cols"]);
    assert!(lends_not_of_the_terminal("self.reply.lend(self.cold).csi(b);").is_empty());
}
