//! `OSC 8` hyperlinks.
//!
//! The [explicit hyperlink spec][osc8] rather than ctlseqs, which does not
//! carry it. The form is `OSC 8 ; params ; URI ST`, and a run is **closed** by
//! `OSC 8 ; ; ST` — an empty URI, not a link to nowhere.
//!
//! The link table is bounded at `NLINK` and evicts on collision, like the style
//! table. A URI is attacker-supplied content used as a key, which `CLAUDE.md`
//! names as the content-addressing failure mode: a page cycling through
//! thousands of links must cost the same as one with four.
//!
//! [osc8]: https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda

use baud_core2::*;

struct Snap {
    cells: Vec<PackedCell>,
    uris: Vec<Option<Vec<u8>>>,
}

fn run(bytes: &[u8]) -> Snap {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
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
    let cells = t.viewport().to_vec();
    let uris = cells[..40]
        .iter()
        .map(|c| t.link_uri(c.flags.link()).map(|u| u.to_vec()))
        .collect();
    Snap { cells, uris }
}

/// A linked run carries its URI; text outside it does not.
#[test]
fn a_linked_run_carries_its_uri() {
    let s = run(b"\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\plain");
    for x in 0..4 {
        assert_eq!(
            s.uris[x].as_deref(),
            Some(&b"https://example.com"[..]),
            "column {x} is inside the link"
        );
    }
    for x in 4..9 {
        assert_eq!(s.uris[x], None, "column {x} is after the close");
    }
}

/// The close is an *empty URI*, not an empty link.
///
/// Treating `OSC 8 ; ; ST` as a link to "" would make every cell after it
/// clickable, which is the failure this distinction prevents.
#[test]
fn an_empty_uri_closes_the_run() {
    let s = run(b"\x1b]8;;http://a\x1b\\x\x1b]8;;\x1b\\y");
    assert!(s.uris[0].is_some());
    assert_eq!(s.uris[1], None);
}

/// Both terminators work, as for any OSC.
#[test]
fn both_terminators_work() {
    assert!(run(b"\x1b]8;;http://a\x07x").uris[0].is_some());
    assert!(run(b"\x1b]8;;http://a\x1b\\x").uris[0].is_some());
}

/// A URI may contain semicolons: only the first two separate the fields.
#[test]
fn a_uri_may_contain_semicolons() {
    let s = run(b"\x1b]8;;http://a/b?c=1;d=2\x1b\\x");
    assert_eq!(s.uris[0].as_deref(), Some(&b"http://a/b?c=1;d=2"[..]));
}

/// The `id=` parameter distinguishes links that share a URI.
///
/// Two runs with different ids are different links even with identical URIs,
/// so they must not collapse into one table slot.
#[test]
fn the_id_parameter_distinguishes_links() {
    let a = run(b"\x1b]8;id=1;http://a\x1b\\x");
    let b = run(b"\x1b]8;id=2;http://a\x1b\\x");
    assert_eq!(a.uris[0], b.uris[0], "same URI reported either way");
    // Different ids must not be the same slot.
    let both = run(b"\x1b]8;id=1;http://a\x1b\\x\x1b]8;id=2;http://a\x1b\\y");
    assert_ne!(
        both.cells[0].flags.link(),
        both.cells[1].flags.link(),
        "distinct ids are distinct links"
    );
}

/// Links compose with the rendition rather than replacing it.
#[test]
fn links_and_styles_are_independent() {
    let s = run(b"\x1b[1;31m\x1b]8;;http://a\x1b\\x");
    assert!(s.uris[0].is_some(), "the link is there");
    let plain = run(b"\x1b[1;31mx");
    assert_eq!(
        s.cells[0].style, plain.cells[0].style,
        "and the style is unchanged by it"
    );
}

/// A link survives a wrap onto the next line.
#[test]
fn a_link_survives_a_wrap() {
    let mut v = b"\x1b]8;;http://a\x1b\\".to_vec();
    v.extend(std::iter::repeat(b'x').take(205));
    let s = run(&v);
    assert!(s.uris[0].is_some());
    // Row 1, column 0 is index 2048.
    let mut st = Storage::boxed();
    let mut t = Term::new(&mut st, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(&v));
    t.feed(base, f.len());
    t.commit_read(f.len());
    let wrapped = t.viewport()[2048];
    assert!(
        t.link_uri(wrapped.flags.link()).is_some(),
        "the wrapped cells are still in the link"
    );
}

/// An overlong URI is truncated rather than dropped or grown.
#[test]
fn an_overlong_uri_is_bounded() {
    let mut v = b"\x1b]8;;http://".to_vec();
    v.extend(std::iter::repeat(b'a').take(LINK_MAX * 2));
    v.extend_from_slice(b"\x1b\\x");
    let s = run(&v);
    let uri = s.uris[0].as_deref().expect("still a link");
    assert!(uri.len() <= LINK_MAX, "bounded at LINK_MAX");
}

/// Cycling through far more links than the table holds does not grow it and
/// does not panic.
///
/// The table evicts; an evicted link renders as an ordinary cell, which is the
/// same tolerable degradation an evicted style has.
#[test]
fn the_link_table_is_bounded_and_evicts() {
    let mut v = Vec::new();
    for i in 0..(NLINK * 8) {
        v.extend_from_slice(format!("\x1b]8;;http://example.com/{i}\x1b\\x").as_bytes());
    }
    let s = run(&v);
    // The last write is still linked: eviction costs old links, not new ones.
    assert!(
        s.cells[..40].iter().any(|c| !c.flags.link().is_none()),
        "links still work after the table has turned over"
    );
}

/// RIS closes any open link, so a crashed program cannot leave the next one's
/// output clickable.
#[test]
fn ris_closes_the_link() {
    let s = run(b"\x1b]8;;http://a\x1b\\\x1bcx");
    assert_eq!(s.uris[0], None);
}

/// Links behave identically however the read boundaries fall.
#[test]
fn hyperlinks_are_chunk_invariant() {
    let seq = b"\x1b]8;id=7;http://example.com/x\x1b\\abc\x1b]8;;\x1b\\def";
    let whole = run(seq).cells;
    for chunk in 1..seq.len() {
        let mut s = Storage::boxed();
        let mut t = Term::new(&mut s, OpMode::Computed);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            let mut slot = t.read_slot();
            let base = slot.base();
            let f = slot.write(StreamByte::slice_from(&seq[i..i + n]));
            t.feed(base, f.len());
            t.commit_read(f.len());
            i += n;
        }
        assert_eq!(t.viewport().to_vec(), whole, "differed at chunk {chunk}");
    }
}

/// The cell is still eight bytes.
///
/// The link rides in spare `CellFlags` bits precisely so this stays true —
/// `CLAUDE.md` records the 8-byte width as measured, and a ninth byte would
/// round the cell to twelve.
#[test]
fn the_cell_is_still_eight_bytes() {
    assert_eq!(core::mem::size_of::<PackedCell>(), 8);
}
