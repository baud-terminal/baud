#![deny(dead_code)]
//! `modifyOtherKeys` and `formatOtherKeys`, checked against the pinned
//! specification's own worked examples.
//!
//! **This is the negotiation `[[source: fixterms]]` does not have.** That
//! document proposes `CSI codepoint ; modifier u` and defines no handshake, so a
//! terminal cannot reach the form without one: sending it unasked breaks every
//! application that did not negotiate it. `[[source: ctlseqs]]`'s two resources
//! are that handshake, and `docs/input-coverage.md` puts them in the **base**
//! layer for exactly that reason -- the `CSI u` row above them is a format, not
//! a protocol.
//!
//! **The expectations are read out of the specification rather than written
//! here.** ctlseqs states four examples in prose -- alt-Tab in both formats,
//! shift-Tab at level 2, and space at level 3 -- and a table transcribed by hand
//! is a guessed table that looks like it worked, which is the reason
//! `scripts/extract-keys.py` exists and the reason `key_encode.rs` reads its own
//! rows the same way.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

fn ctlseqs() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("the pinned ctlseqs")
}

/// Every `<key> sends CSI <params> <final>` the specification states.
///
/// Returned as `(key, bytes)` so a caller names the key it wants rather than an
/// index -- a positional read of a prose scrape is how a document edit silently
/// re-points an assertion.
fn worked_examples(doc: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for line in doc.lines() {
        let Some(at) = line.find(" sends CSI ") else {
            continue;
        };
        // The key is the last word before "sends".
        let Some(key) = line[..at].split_whitespace().next_back() else {
            continue;
        };
        let rest = &line[at + " sends CSI ".len()..];
        // Parameters run to the final byte. Digits are spaced in this document
        // -- `3 2` is thirty-two -- so the spaces come out.
        // Bytes rather than chars, because the final byte is pushed into a byte
        // string and converting a `char` to get there is what `audit_all.sh`
        // refuses. Everything matched here is ASCII by construction.
        //
        // Do not name the cast operator in a comment in this file:
        // `impl_audit.py` matches the token in prose as well as in code, so
        // describing one spends an audit site. It cost one here, twice in one
        // session, which is why the sentence above is worded around it.
        let mut params: Vec<u8> = Vec::new();
        let mut fin = None;
        for c in rest.bytes() {
            match c {
                b'0'..=b'9' | b';' => params.push(c),
                b' ' => {}
                b'~' | b'u' => {
                    fin = Some(c);
                    break;
                }
                _ => break,
            }
        }
        let Some(fin) = fin else { continue };
        if params.is_empty() {
            continue;
        }
        let mut bytes = b"\x1b[".to_vec();
        bytes.extend_from_slice(&params);
        bytes.push(fin);
        out.push((key.to_string(), bytes));
    }
    out
}

/// Look one up, failing loudly rather than skipping.
///
/// **A missing example must fail the test**, because the whole file rests on
/// the scrape working: a regex that stops matching after a document edit would
/// otherwise leave every assertion unreached and the run green. That is the
/// probe-that-cannot-fire failure this repository records nine times.
fn example(all: &[(String, Vec<u8>)], key: &str, fin: u8) -> Vec<u8> {
    all.iter()
        .find(|(k, b)| k == key && b.last() == Some(&fin))
        .map(|(_, b)| b.clone())
        .unwrap_or_else(|| {
            panic!(
                "ctlseqs states no `{key} sends CSI ... {}`",
                char::from(fin)
            )
        })
}

/// Set the resources with real escape sequences, then press a key.
///
/// **The sequence rather than a field**, because the contract being tested is
/// that XTMODKEYS is parsed *and* acted on. A test that reached into `Cold`
/// would pass against a parser that recognised the sequence and dropped it,
/// which `docs/vt-coverage.md` rates worse than not implementing it at all.
fn press_with(setup: &[u8], code: KeyCode, mods: Modifiers) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    if !setup.is_empty() {
        t.write_at(BytePos::ZERO, setup);
        t.feed(BytePos::ZERO, ReadLen::from_index(setup.len()));
        t.clear_reply();
    }
    assert!(t.key(KeyEvent::press(code, mods)), "the key was encoded");
    t.reply().to_vec()
}

/// XTMODKEYS, `modifyOtherKeys` level 1.
const LEVEL1: &[u8] = b"\x1b[>4;1m";
/// XTMODKEYS, level 2 -- the level that replaces `CSI Z`.
const LEVEL2: &[u8] = b"\x1b[>4;2m";
/// XTFMTKEYS, `formatOtherKeys` 1 -- the `CSI u` form.
const FORMAT_CSI_U: &[u8] = b"\x1b[>4;1f";

/// The specification's own four examples, and the scrape that finds them.
#[test]
fn the_worked_examples_are_what_baud_sends() {
    let doc = ctlseqs();
    let all = worked_examples(&doc);

    // Positive control: the scrape must find the shapes this file asserts on,
    // or every assertion below is unreached.
    assert!(
        all.len() >= 4,
        "the scrape found {} worked examples, expected at least 4",
        all.len()
    );

    // alt-Tab at level 1, in xterm's format. ctlseqs line 2229.
    let alt_tab_xterm = example(&all, "alt-Tab", b'~');
    assert_eq!(
        press_with(LEVEL1, KeyCode::Tab, Modifiers::ALT),
        alt_tab_xterm,
        "alt-Tab at modifyOtherKeys 1"
    );

    // The same key once formatOtherKeys picks the fixterms form. ctlseqs 2249.
    let mut setup = LEVEL1.to_vec();
    setup.extend_from_slice(FORMAT_CSI_U);
    let alt_tab_csi_u = example(&all, "alt-Tab", b'u');
    assert_eq!(
        press_with(&setup, KeyCode::Tab, Modifiers::ALT),
        alt_tab_csi_u,
        "alt-Tab at formatOtherKeys 1 -- this is the `CSI u` form"
    );

    // shift-Tab at level 2, which ctlseqs line 2237 states *replaces* `CSI Z`.
    let shift_tab = example(&all, "shift-Tab", b'~');
    assert_eq!(
        press_with(LEVEL2, KeyCode::Tab, Modifiers::SHIFT),
        shift_tab,
        "shift-Tab at modifyOtherKeys 2"
    );
}

/// **The base layer is unchanged until an application asks**, which is the
/// property that makes the resource safe to implement at all.
///
/// shift-Tab is the discriminating key: level 2 replaces its answer, so levels
/// 0 and 1 must still produce `CSI Z` or the mode has broken every application
/// that never negotiated it.
#[test]
fn the_base_layer_is_untouched_at_levels_zero_and_one() {
    let csi_z = b"\x1b[Z".to_vec();
    assert_eq!(
        press_with(b"", KeyCode::Tab, Modifiers::SHIFT),
        csi_z,
        "shift-Tab with no negotiation"
    );
    assert_eq!(
        press_with(LEVEL1, KeyCode::Tab, Modifiers::SHIFT),
        csi_z,
        "shift-Tab at level 1: shift is a modifier level 1 leaves alone"
    );

    // And an unmodified key is never taken, at either level: that is level 3
    // (`modify_other_keys_3_sends_every_ordinary_key`).
    assert_eq!(
        press_with(LEVEL2, KeyCode::Tab, Modifiers::NONE),
        b"\t".to_vec()
    );
    assert_eq!(press_with(LEVEL2, ch('a'), Modifiers::NONE), b"a".to_vec());
}

fn ch(c: char) -> KeyCode {
    KeyCode::Char(Codepoint::from_char(c))
}

/// A level past 3 is declined and counted.
///
/// **Not rounded down to a level baud can express.** An application that asks
/// for a level that does not exist and silently receives level 3 has no way to
/// find out; one whose request was refused can query and see it did not take.
/// That is the contract `DECRQM` keeps by answering `0` rather than `2`,
/// applied to a resource.
#[test]
fn an_unrepresentable_level_is_refused_and_counted() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let before = t.unsupported;

    for b in [
        &b"\x1b[>4;4m"[..],  // no level 4
        &b"\x1b[>0;16m"[..], // past modifyKeyboard's four bits
        &b"\x1b[>1;5m"[..],  // past modifyCursorKeys' 4
        &b"\x1b[>4;2f"[..],  // a formatOtherKeys value with no meaning here
        &b"\x1b[?4g"[..],    // XTQFMTKEYS as ctlseqs spells it; xterm parses `>`
    ] {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }
    assert_eq!(
        t.unsupported - before,
        5,
        "each declined resource is counted, not dropped"
    );

    // And the level did not take: shift-Tab still answers from the base layer,
    // because the refused sequence left the resource where it was.
    assert_eq!(
        press_with(b"\x1b[>4;4m", KeyCode::Tab, Modifiers::SHIFT),
        b"\x1b[Z".to_vec(),
        "a refused level must not half-apply"
    );
}

/// **The resource survives a read boundary anywhere inside its sequence.**
///
/// `CSI > 4 ; 2 m` is eight bytes and a `read(2)` can split it at any of them.
/// A mode that took only when the sequence arrived whole would give a screen --
/// here a keyboard -- that depends on where the reads fell, which is the class
/// this project's chunk-invariance properties exist to catch and the class that
/// has produced the most defects in it.
///
/// The corpus cannot check this: **no recording contains a single XTMODKEYS
/// byte**, which is the same reason the HT and SO/SI fixtures exist.
#[test]
fn the_resource_is_chunk_invariant() {
    let mut setup = LEVEL2.to_vec();
    setup.extend_from_slice(FORMAT_CSI_U);

    let whole = {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.write_at(BytePos::ZERO, &setup);
        t.feed(BytePos::ZERO, ReadLen::from_index(setup.len()));
        t.clear_reply();
        assert!(t.key(KeyEvent::press(KeyCode::Tab, Modifiers::SHIFT)));
        t.reply().to_vec()
    };

    for chunk in [1usize, 2, 3, 5, 7] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        for part in setup.chunks(chunk) {
            t.write_at(BytePos::ZERO, part);
            t.feed(BytePos::ZERO, ReadLen::from_index(part.len()));
        }
        t.clear_reply();
        assert!(t.key(KeyEvent::press(KeyCode::Tab, Modifiers::SHIFT)));
        assert_eq!(
            t.reply().to_vec(),
            whole,
            "shift-Tab differs when the setup arrives in {chunk}-byte reads"
        );
    }

    // The control: the answer being compared is the negotiated one, not the
    // base layer's. Without this the test would pass against a build where
    // neither arm ever set the resource.
    assert_ne!(
        whole,
        b"\x1b[Z".to_vec(),
        "the setup must have taken effect"
    );
}
