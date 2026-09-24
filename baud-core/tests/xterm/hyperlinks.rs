//! `OSC 8` hyperlinks.
//!
//! The [explicit hyperlink spec][osc8] rather than ctlseqs, which does not
//! carry it. The form is `OSC 8 ; params ; URI ST`, and a run is **closed** by
//! `OSC 8 ; ; ST` — an empty URI, not a link to nowhere.
//!
//! The link table is bounded at `NLINK` -- a URI is attacker-supplied content
//! used as a key, which `CLAUDE.md` names as the content-addressing failure
//! mode: a page cycling through thousands of links must cost the same as one
//! with four -- and **it refuses a colliding link rather than evicting the
//! occupant**. It evicted like the style table until activation was designed
//! and a review showed what that lets a program do: an `OSC 8` run that writes
//! no cell lands in the slot an honest link already on screen resolves
//! through, and a click on unchanged text opens the newcomer's target. A
//! refused link is counted and renders as plain text; a clipped one is
//! refused too, because a cut URL is a different URL.
//!
//! [osc8]: https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda

use crate::support;
use baud_core::*;

struct Snap {
    cells: Vec<PackedCell>,
    uris: Vec<Option<Vec<u8>>>,
}

fn run(bytes: &[u8]) -> Snap {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let cells = support::screen(&t);
    let uris = cells[..40]
        .iter()
        .map(|c| t.link_uri(c.flags.link()).map(|u| u.to_vec()))
        .collect();
    Snap { cells, uris }
}

/// A URI that lands in the same slot as `target` under `id`, without being
/// `target`: the collision the table has to refuse.
fn colliding_with(id: &[u8], target: &[u8]) -> Vec<u8> {
    let want = LinkId::bucket(id, target, NLINK);
    (0u32..)
        .map(|i| format!("http://example.com/{i}").into_bytes())
        .find(|u| u.as_slice() != target && LinkId::bucket(id, u, NLINK) == want)
        .expect("127 slots: a collision turns up in a few hundred tries")
}

/// The stream `bytes` fed to a fresh term, for a test that reads counters.
fn term_after(bytes: &[u8]) -> (Vec<PackedCell>, usize, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    (support::screen(&t), t.links_refused, t.links_clipped)
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
    v.extend(std::iter::repeat_n(b'x', 205));
    let s = run(&v);
    assert!(s.uris[0].is_some());
    // **Row 1, column 0 -- at the live stride, not at `MAX_COLS`.** This read
    // index `2048` and found a cell no addressing path can reach: core2 strides
    // by the maximum, this core by the live column count, which is worth +14%
    // on anything that scrolls. Fifth instance of that porting bug, and the
    // reason `support` hands back the stride beside the cells.
    let ws = support::CORPUS;
    let mut st = Owned::new(&ws);
    let mut t = Term::new(st.storage(), &ws);
    t.write_at(BytePos::ZERO, &v);
    t.feed(BytePos::ZERO, ReadLen::from_index(v.len()));
    let w = t.stride().index();
    let wrapped = t.viewport()[w];
    assert!(
        t.link_uri(wrapped.flags.link()).is_some(),
        "the wrapped cells are still in the link"
    );
}

/// **A URI that may have been cut is refused, not clipped.** Past `LINK_MAX`
/// it used to be stored as its first 256 bytes in silence, and a click would
/// have opened a URL nobody sent; the same for a body that filled `OSC_MAX`
/// before the URI was split out of it, where a long `id=` leaves a URI
/// *under* the cap that still ends mid-path. Both count in `links_clipped`,
/// and a URI under both caps is the control.
#[test]
fn a_link_whose_uri_may_have_been_cut_is_refused_and_counted() {
    // Past LINK_MAX.
    let mut v = b"\x1b]8;;http://".to_vec();
    v.extend(std::iter::repeat_n(b'a', LINK_MAX));
    v.extend_from_slice(b"\x1b\\x\x1b]8;;\x1b\\");
    let (cells, refused, clipped) = term_after(&v);
    assert!(cells[0].flags.link().is_none(), "the run is plain text");
    assert_eq!((refused, clipped), (0, 1));

    // Under LINK_MAX after the body was clipped at OSC_MAX by a long id:
    // 5 + 300 + 1 + 19 + 300 bytes of body, cut at 512, leaves a URI of
    // 187 printable bytes ending mid-path.
    let mut v = b"\x1b]8;id=".to_vec();
    v.extend(std::iter::repeat_n(b'i', 300));
    v.extend_from_slice(b";http://example.com/");
    v.extend(std::iter::repeat_n(b'p', 300));
    v.extend_from_slice(b"\x1b\\y\x1b]8;;\x1b\\");
    let (cells, refused, clipped) = term_after(&v);
    assert!(cells[0].flags.link().is_none(), "the cut run is plain text");
    assert_eq!((refused, clipped), (0, 1));

    // The control: a 200-byte URI is a link and nothing is counted.
    let mut v = b"\x1b]8;;http://".to_vec();
    v.extend(std::iter::repeat_n(b'a', 193));
    v.extend_from_slice(b"\x1b\\z\x1b]8;;\x1b\\");
    let (cells, refused, clipped) = term_after(&v);
    assert!(
        !cells[0].flags.link().is_none(),
        "under both caps it is a link"
    );
    assert_eq!((refused, clipped), (0, 0));
}

/// **A cell never resolves to a URI other than the one written under its
/// index.** A later link that hashes to an occupied slot is refused and
/// counted, never written over the occupant. The table evicted until
/// activation was designed, and the review that changed it named the
/// consequence: an `OSC 8` run that writes no cell could retarget every cell
/// already on screen whose index it took, with nothing on screen changing.
#[test]
fn a_cell_never_resolves_to_a_uri_other_than_the_one_written_under_its_index() {
    let first = b"http://a".to_vec();
    let second = colliding_with(b"", &first);
    let mut v = Vec::new();
    for (uri, text) in [(&first, &b"x"[..]), (&second, &b"y"[..])] {
        v.extend_from_slice(b"\x1b]8;;");
        v.extend_from_slice(uri);
        v.extend_from_slice(b"\x1b\\");
        v.extend_from_slice(text);
        v.extend_from_slice(b"\x1b]8;;\x1b\\");
    }
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, &v);
    let cells = support::screen(&t);
    assert_eq!(
        t.link_uri(cells[0].flags.link()),
        Some(first.as_slice()),
        "the first link keeps its URI"
    );
    assert_eq!(
        t.link_uri(cells[1].flags.link()),
        None,
        "the colliding link is plain text, not the other target"
    );
    assert_eq!((t.links_refused, t.links_clipped), (1, 0));
}

/// The one collision that is not a collision: the same URI under an `id` that
/// hashes to the same slot shares it, because one slot for one target is
/// exactly right, and it is not counted as a refusal.
#[test]
fn a_colliding_link_with_the_same_uri_shares_the_slot() {
    let uri = b"http://example.com/";
    let want = LinkId::bucket(b"1", uri, NLINK);
    let other = (2u32..)
        .map(|k| k.to_string().into_bytes())
        .find(|k| LinkId::bucket(k, uri, NLINK) == want)
        .expect("some id shares slot 1's bucket");
    let mut v = b"\x1b]8;id=1;http://example.com/\x1b\\x\x1b]8;;\x1b\\\x1b]8;id=".to_vec();
    v.extend_from_slice(&other);
    v.extend_from_slice(b";http://example.com/\x1b\\y\x1b]8;;\x1b\\");
    let (cells, refused, clipped) = term_after(&v);
    assert_eq!(cells[0].flags.link(), cells[1].flags.link(), "one slot");
    assert!(!cells[1].flags.link().is_none(), "and it is a link");
    assert_eq!((refused, clipped), (0, 0));
}

/// Cycling through far more links than the table holds does not grow it and
/// does not panic: past 127 distinct targets every new link is refused, the
/// refusals are counted, and the links that took a slot keep it.
#[test]
fn the_link_table_is_bounded_and_refuses_rather_than_grows() {
    let mut v = Vec::new();
    for i in 0..(NLINK * 8) {
        v.extend_from_slice(format!("\x1b]8;;http://example.com/{i}\x1b\\x").as_bytes());
    }
    let (cells, refused, clipped) = term_after(&v);
    assert!(
        !cells[0].flags.link().is_none(),
        "the first link took a slot and keeps it"
    );
    assert!(
        refused >= NLINK * 8 - (NLINK - 1),
        "at most 127 slots taken, the rest refused: {refused}"
    );
    assert_eq!(clipped, 0);
}

/// A wide character's tail carries its lead's link, so a click on either
/// half of it resolves -- which activation depends on and nothing asserted.
#[test]
fn a_wide_tail_carries_its_leads_link() {
    let s = run(b"\x1b]8;;http://a\x1b\\\xe4\xb8\xad\x1b]8;;\x1b\\");
    assert_eq!(s.cells[1].content, Content::WIDE_TAIL, "the tail of U+4E2D");
    assert_eq!(s.cells[1].flags.link(), s.cells[0].flags.link());
    assert_eq!(s.uris[1].as_deref(), Some(&b"http://a"[..]));
}

/// RIS closes any open link, so a crashed program cannot leave the next one's
/// output clickable.
#[test]
fn ris_closes_the_link() {
    let s = run(b"\x1b]8;;http://a\x1b\\\x1bcx");
    assert_eq!(s.uris[0], None);
}

/// **A full reset frees the link table.** It is bounded at 127 targets and
/// refuses past them rather than evicting, because a slot a cell still names
/// must not be taken by another target. A full reset erases every cell a
/// reader can reach -- the screen and the history -- so nothing names a slot
/// after one, and a table filled before it must take new links after it.
#[test]
fn a_full_reset_frees_the_link_table() {
    let mut v = Vec::new();
    for i in 0..(NLINK * 4) {
        v.extend_from_slice(
            format!("\x1b]8;;http://example.com/{i}\x1b\\x\x1b]8;;\x1b\\").as_bytes(),
        );
    }
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, &v);
    let refused = t.links_refused;
    assert!(refused > 0, "the control: the table filled");
    support::feed(
        &mut t,
        b"\x1bc\x1b]8;;http://after.example/\x1b\\y\x1b]8;;\x1b\\",
    );
    let cells = support::screen(&t);
    assert_eq!(
        t.links_refused, refused,
        "the link after the reset was taken"
    );
    assert_eq!(
        t.link_uri(cells[0].flags.link()),
        Some(&b"http://after.example/"[..]),
        "and resolves to its own target"
    );
}

/// Links behave identically however the read boundaries fall.
#[test]
fn hyperlinks_are_chunk_invariant() {
    let seq = b"\x1b]8;id=7;http://example.com/x\x1b\\abc\x1b]8;;\x1b\\def";
    let whole = run(seq).cells;
    for chunk in 1..seq.len() {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            t.write_at(BytePos::ZERO, &seq[i..i + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            i += n;
        }
        assert_eq!(support::screen(&t), whole, "differed at chunk {chunk}");
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
