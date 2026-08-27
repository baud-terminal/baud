//! Conformance against the terminfo entries baud claims to be.
//!
//! `$TERM` values are **capability profiles, not protocols**. `xterm-256color`
//! is not a different protocol from `xterm`; it is the same protocol with a
//! different colour capability declared. So there is no implementation file per
//! `$TERM` — there is one implementation, and this asks, per profile, whether
//! it covers what the profile promises.
//!
//! The method: for every *output* capability in the entry, expand it to bytes,
//! feed it to a fresh `Term`, and require that nothing was recognised and
//! dropped. `Term::unsupported` counts exactly that — the `parsed` failure made
//! observable. Capabilities whose name begins with `k` are keys the terminal
//! *sends*; they are the input path, which baud does not have, and are counted
//! separately rather than silently skipped.
//!
//! Fixtures are `infocmp -1` output, captured verbatim and hash-pinned in
//! `docs/research/sources/MANIFEST.toml`, from ncurses 6.0.20150808.

mod enumerated;
mod levels;
mod spec_inventory;
mod terminfo;

use baud_core::*;
use terminfo::{Cap, is_input};

fn fixture(term: &str) -> Vec<Cap> {
    let p = format!(
        "{}/../docs/research/sources/terminfo/{term}.ti",
        env!("CARGO_MANIFEST_DIR")
    );
    let src = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"));
    terminfo::parse(&src)
}

/// Feed one capability to a fresh terminal; report how many sequences it
/// recognised and dropped.
fn dropped(preamble: &[u8], bytes: &[u8]) -> u64 {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let bytes = &[preamble, bytes].concat()[..];
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }
    t.unsupported.count()
}

/// Capabilities baud deliberately does not implement, with the reason.
///
/// Kept explicit and small. A growing list means conformance is being
/// redefined rather than achieved.
fn exempt(name: &str) -> Option<&'static str> {
    Some(match name {
        // Media copy: printing. There is no printer, and inventing one would be
        // a sequence accepted and silently discarded.
        "mc0" | "mc4" | "mc5" | "mc5p" => "printer support, out of scope",
        // xterm memory lock/unlock, a scroll-region variant no application uses.
        "meml" | "memu" => "xterm memory lock, no known consumer",
        // `u6`/`u8` are *response templates*: the shape an application should
        // expect back from `u7`/`u9`. They are what the terminal sends, not
        // what it receives, so feeding them to a parser tests nothing. baud
        // does emit both -- `tests/vt_replies.rs` checks the shapes.
        "u6" | "u8" => "response template, not a received sequence",
        _ => return None,
    })
}

/// What must be sent before a profile's capabilities mean anything.
///
/// Only VT52 needs one, and needing it is the point: `TERM=vt52` describes a
/// terminal that has been put into VT52 mode, where `ESC D` is cursor-left
/// rather than IND. Replaying its capabilities at an ANSI-mode parser would be
/// testing the wrong protocol.
fn preamble(term: &str) -> &'static [u8] {
    match term {
        "vt52" => b"\x1b[?2l",
        _ => b"",
    }
}

/// Report, rather than assert — used to see where a profile actually stands.
fn survey(term: &str) -> (Vec<String>, usize, usize) {
    let caps = fixture(term);
    let mut missing = Vec::new();
    let (mut out, mut inp) = (0, 0);
    for c in &caps {
        if is_input(&c.name) {
            inp += 1;
            continue;
        }
        out += 1;
        if exempt(&c.name).is_some() {
            continue;
        }
        let n = dropped(preamble(term), &c.bytes);
        if n > 0 {
            missing.push(format!(
                "{} = {:?}",
                c.name,
                String::from_utf8_lossy(&c.bytes)
            ));
        }
    }
    (missing, out, inp)
}

macro_rules! profile {
    ($fn:ident, $term:literal) => {
        #[test]
        fn $fn() {
            let (missing, out, inp) = survey($term);
            assert!(out > 10, "{}: only {out} output capabilities parsed", $term);
            assert!(
                missing.is_empty(),
                "{}: {} of {out} output capabilities are recognised and dropped \
                 ({inp} input capabilities not counted -- baud has no input path):\n  {}",
                $term,
                missing.len(),
                missing.join("\n  ")
            );
        }
    };
}

profile!(ansi, "ansi");
profile!(vt100, "vt100");
profile!(vt102, "vt102");
profile!(vt52, "vt52");
profile!(xterm, "xterm");
profile!(xterm_16color, "xterm-16color");
profile!(xterm_256color, "xterm-256color");
profile!(rxvt, "rxvt");
profile!(dtterm, "dtterm");
profile!(nsterm, "nsterm");
