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

#[path = "../support/mod.rs"]
mod support;

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
fn dropped(preamble: &[u8], bytes: &[u8]) -> usize {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let bytes = &[preamble, bytes].concat()[..];
    support::feed(&mut t, bytes);
    t.unsupported
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

/// **Six, and one cause: `baud-core` counts unsupported CSI *finals* only.**
///
/// `spec_inventory` reads `[[source: ctlseqs]]`'s own lists and requires every
/// form to be acted on **or counted** -- the point being that a sequence the
/// parser recognises and drops is worse than one it does not implement,
/// because the application has no way to find out. `baud-core2` answers with a
/// single `unsupported` `Stat` that every dispatch arm bumps; `baud-core` has
/// `unsupported` for CSI finals, `strings_ignored` for control strings and
/// `charset_unsupported` for designations, and **nothing at all** for an `ESC`
/// form or an intermediate form it declines.
///
/// So this is one counter's worth of work rather than six features, and it is
/// the last thing between these suites and the `done` rows in
/// `docs/vt-coverage.md` that they gate. `every_spec_csi_final_is_handled_or_
/// counted` passes today, which is what says the mechanism is right and only
/// its coverage is short.
#[test]
fn the_known_conformance_gaps() {
    support::ignores_are(
        "conformance",
        &["spec_inventory.rs"],
        &[
            "a_final_with_two_arities_means_two_things",
            "every_spec_csi_form_is_handled_or_counted",
            "every_spec_dcs_form_is_handled_or_counted",
            "every_spec_esc_form_is_handled_or_counted",
            "every_spec_esc_intermediate_form_is_handled_or_counted",
            "every_spec_osc_code_is_handled_or_counted",
        ],
    );
}
