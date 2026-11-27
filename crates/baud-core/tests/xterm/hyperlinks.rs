//! `OSC 8` hyperlinks.
//!
//! The [explicit hyperlink spec][osc8] rather than ctlseqs, which does not
//! carry it. The form is `OSC 8 ; params ; URI ST`, and a run is **closed** by
//! `OSC 8 ; ; ST` — an empty URI, not a link to nowhere.
//!
//! The link table is bounded at `NLINK` -- a URI is attacker-supplied content
//! used as a key, so a page cycling through thousands of links must cost the
//! same as one with four -- and **it never gives a slot something names to another
//! target**. It evicted like the style table until activation was designed
//! and a review showed what that lets a program do: an `OSC 8` run that writes
//! no cell lands in the slot an honest link already on screen resolves
//! through, and a click on unchanged text opens the newcomer's target. A
//! target probes sixteen slots, the table is swept of the slots nothing names
//! when they are all held, and a link still without one is refused, counted
//! and renders as plain text; a clipped one is refused too, because a cut URL
//! is a different URL.
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

/// `OSC 8` to `uri`, `text`, and the close.
fn link(uri: &str, text: &str) -> Vec<u8> {
    format!("\x1b]8;;{uri}\x1b\\{text}\x1b]8;;\x1b\\").into_bytes()
}

/// Erase the screen and the history (`ED 2`, xterm's `ED 3`) and write a
/// screen's worth of plain text: what lets a sweep run and find nothing
/// naming the links that were there.
fn wipe() -> Vec<u8> {
    let mut v = b"\x1b[H\x1b[2J\x1b[3J".to_vec();
    v.extend(std::iter::repeat_n(b'.', 200 * 50));
    v.extend_from_slice(b"\x1b[H\x1b[2J");
    v
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
    for (x, uri) in s.uris.iter().enumerate().take(4) {
        assert_eq!(
            uri.as_deref(),
            Some(&b"https://example.com"[..]),
            "column {x} is inside the link"
        );
    }
    for (x, uri) in s.uris.iter().enumerate().take(9).skip(4) {
        assert_eq!(*uri, None, "column {x} is after the close");
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
    // **Row 1, column 0 -- at `Term::stride`, not at `MAX_COLS`.** This read
    // index `2048` and found a cell no addressing path can reach: core2 strode
    // by the maximum, this core by a pitch fixed from the provisioned width,
    // and leaving the maximum is worth +14% on anything that scrolls. Fifth
    // instance of that porting bug, and the reason `support` hands back the
    // stride beside the cells.
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
/// index.** A later link that hashes to an occupied slot takes another slot
/// of its window, never the occupant's. The table evicted until activation
/// was designed, and a later check of that design found the consequence: an
/// `OSC 8` run that writes no cell could retarget every cell already on
/// screen whose index it took, with nothing on screen changing -- so the
/// empty run comes first here, and the cell is read after it.
#[test]
fn a_cell_never_resolves_to_a_uri_other_than_the_one_written_under_its_index() {
    let first = b"http://a".to_vec();
    let second = colliding_with(b"", &first);
    let mut v = Vec::new();
    for (uri, text) in [
        (&first, &b"x"[..]),
        (&second, &b""[..]),
        (&second, &b"y"[..]),
    ] {
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
        "the first link keeps its URI through a colliding run that wrote no cell"
    );
    assert_eq!(
        t.link_uri(cells[1].flags.link()),
        Some(second.as_slice()),
        "the colliding link is its own target, in a slot of its own"
    );
    assert_ne!(cells[0].flags.link(), cells[1].flags.link());
    assert_eq!((t.links_refused, t.links_clipped), (0, 0));
}

/// **Two ids with one URI are two links, even from one home slot.**
/// `[[source: osc8-spec]]`: cells "that have the same target URI and the
/// same nonempty `id` are always underlined together"; a second `id` is
/// another link. The table shared the slot when the two hashed alike, since
/// it kept the URI alone; a slot keeps the key of its `id` and URI now.
/// Perturbation: match a slot on its URI alone -- one slot.
#[test]
fn two_ids_with_one_uri_are_two_links_even_from_one_home_slot() {
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
    assert_ne!(cells[0].flags.link(), cells[1].flags.link(), "two slots");
    assert!(!cells[1].flags.link().is_none(), "and both are links");
    assert_eq!((refused, clipped), (0, 0));
}

/// **Sixty distinct targets on one screen are all linked.** Each target
/// hashed to one slot and was refused whenever another held it, so a
/// listing lost links long before 127: `ls --hyperlink` of a 37-entry
/// directory linked 34 of them. A target probes sixteen slots now.
/// Perturbation: probe one slot -- several of the sixty are plain text.
#[test]
fn sixty_distinct_targets_on_one_screen_are_all_linked() {
    let mut v = Vec::new();
    for i in 0..60 {
        v.extend(link(&format!("file:///usr/bin/tool-{i}"), "f\r\n"));
    }
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, &v);
    assert_eq!(t.links_refused, 0, "every target took a slot");
    let w = t.stride().index();
    for i in 0..60 {
        let want = format!("file:///usr/bin/tool-{i}");
        assert_eq!(
            t.link_uri(t.cells()[i * w].flags.link()),
            Some(want.as_bytes()),
            "row {i}"
        );
    }
}

/// **A target past the table is linked once nothing names the old ones.**
/// A slot was freed only by `RIS`, so after 127 targets every later link
/// of the session was plain text: a listing, `clear`, and another listing
/// showed no link at all. When every slot a target may take is held, the
/// table is swept of the slots no cell, open link or parked display names,
/// and the epoch a held index is checked against moves. Perturbation:
/// never sweep -- the second listing is plain text.
#[test]
fn a_target_past_the_table_is_linked_once_nothing_names_the_old_ones() {
    let mut v = Vec::new();
    for i in 0..(NLINK * 2) {
        v.extend(link(&format!("http://first.example/{i}"), "a"));
    }
    v.extend(wipe());
    for i in 0..100 {
        v.extend(link(&format!("http://second.example/{i}"), "b"));
    }
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let before = t.link_epoch();
    support::feed(&mut t, &v);
    let cells = support::screen(&t);
    for (i, c) in cells[..100].iter().enumerate() {
        let want = format!("http://second.example/{i}");
        assert_eq!(
            t.link_uri(c.flags.link()),
            Some(want.as_bytes()),
            "column {i}"
        );
    }
    let swept = t.link_sweeps();
    assert!(swept.sweeps >= 1 && swept.freed >= NLINK - 1, "{swept:?}");
    assert_ne!(
        t.link_epoch(),
        before,
        "a slot was freed, so the epoch moved"
    );
}

/// **No cell ever resolves to another link's target**, through a session
/// that fills the table, erases, overwrites linked cells with others,
/// scrolls them into the history and out of it, and sweeps again and
/// again, and sweeps on the alternate screen while the primary's links wait
/// behind it. Each target's text is one wide character that names it, so
/// every linked cell the ring holds is checked against the target it was
/// written under, after every read. Perturbations: a sweep that frees
/// every slot, named or not, or one that leaves the history or the other
/// screen unmarked -- a cell resolves to the target that took its slot.
#[test]
fn no_cell_ever_resolves_to_another_links_target() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let mut n = 0u32;
    let mut checked = 0usize;
    // Every linked cell of the ring against its own target: how many.
    fn check(t: &Term<'_>, round: u32) -> usize {
        let mut checked = 0;
        for c in t.cells() {
            let Some(uri) = t.link_uri(c.flags.link()) else {
                continue;
            };
            let Some(k) = c.content.as_codepoint().map(|cp| cp.value()) else {
                continue;
            };
            let want = format!("http://x.example/{}", k - 0x4E00);
            assert_eq!(uri, want.as_bytes(), "round {round}");
            checked += 1;
        }
        checked
    }
    for round in 0..40u32 {
        let mut v = Vec::new();
        for _ in 0..150 {
            let ch = char::from_u32(0x4E00 + n).expect("a CJK ideograph");
            v.extend(link(&format!("http://x.example/{n}"), &ch.to_string()));
            n += 1;
        }
        match round % 4 {
            // Overwrite the first rows in place.
            0 => v.extend_from_slice(b"\x1b[H"),
            // Scroll the screen into the history.
            1 => v.extend(std::iter::repeat_n(b'\n', 60)),
            // Erase the screen and the history and write a screen.
            2 => v.extend(wipe()),
            // Write a screen of text past the links.
            _ => v.extend(std::iter::repeat_n(b'-', 200 * 50)),
        }
        support::feed(&mut t, &v);
        checked += check(&t, round);
    }
    // Last, the alternate screen: links on the primary, then on the
    // alternate screen links fill the table, an erase and a screen of
    // text let a sweep run, and more links take what it frees, while the
    // primary's wait behind it. First a ring's worth of text, the bytes
    // that pay for about four sweeps.
    let mut v = vec![b'-'; t.cells().len()];
    v.extend(wipe());
    for _ in 0..100 {
        let ch = char::from_u32(0x4E00 + n).expect("a CJK ideograph");
        v.extend(link(&format!("http://x.example/{n}"), &ch.to_string()));
        n += 1;
    }
    v.extend_from_slice(b"\x1b[?1049h");
    for i in 0..(NLINK * 2) {
        v.extend(link(&format!("http://alt.example/a{i}"), "a"));
    }
    v.extend(wipe());
    for i in 0..(NLINK * 2) {
        v.extend(link(&format!("http://alt.example/b{i}"), "b"));
    }
    v.extend_from_slice(b"\x1b[?1049l");
    let swept = t.link_sweeps().sweeps;
    support::feed(&mut t, &v);
    assert!(
        t.link_sweeps().sweeps > swept + 1,
        "the control: the alternate screen swept"
    );
    let back = check(&t, 40);
    assert!(
        back >= 100,
        "the control: the primary's links came back, {back}"
    );
    assert!(t.link_sweeps().freed > 0, "the session swept");
    assert!(
        checked > 1000,
        "the control: linked cells were checked, {checked}"
    );
}

/// **A link the parked display holds open outlives a sweep.** A
/// host-writable status line keeps its own pen while the main display is
/// written (`[[source: dec-std-070]]` 070-14), open link and all, and no
/// cell names that link until the line writes again -- so a sweep that
/// freed the slot would hand the line's next character another target.
/// The line's own cells name theirs while the main display is written.
/// Perturbations: leave the parked display unmarked -- the line's write
/// resolves to whatever took the slot; leave the line's cells unmarked --
/// its first character does.
#[test]
fn a_link_the_parked_display_holds_open_outlives_a_sweep() {
    let mut v = b"\x1b[2$~\x1b[1$}\x1b]8;;http://cell.example/\x1b\\T".to_vec();
    v.extend_from_slice(b"\x1b]8;;http://status.example/\x1b\\\x1b[0$}");
    for i in 0..(NLINK * 2) {
        v.extend(link(&format!("http://fill.example/{i}"), "a"));
    }
    v.extend(wipe());
    for i in 0..(NLINK * 2) {
        v.extend(link(&format!("http://after.example/{i}"), "b"));
    }
    v.extend_from_slice(b"\x1b[1$}S");
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, &v);
    assert!(
        t.link_sweeps().freed > 0,
        "the control: the table was swept"
    );
    let line = t.status_view().expect("a status line");
    let first = line.row(Row::from_index(0))[0];
    assert_eq!(
        t.link_uri(first.flags.link()),
        Some(&b"http://cell.example/"[..]),
        "the line's cell keeps its target"
    );
    let cell = line.row(Row::from_index(0))[1];
    assert_eq!(
        cell.content.as_codepoint().map(|c| c.value()),
        Some(u32::from(b'S'))
    );
    assert_eq!(
        t.link_uri(cell.flags.link()),
        Some(&b"http://status.example/"[..]),
        "the line writes with its own target"
    );
}

/// **A sweep reads at most four cells for each byte read**, whatever the
/// bytes write: a screen of cells is no measure, because `DECALN` writes
/// one in three bytes, and neither is `ED 3`. 3,000 lines, each a link to
/// one of 127 targets written across the whole width, leave every row of
/// the screen and the history holding one; then two thousand rounds of
/// `DECALN`, or of `ED 3` and a cursor home, and eight new targets.
/// Counted, the sweeps read no more cells than four a byte, the 64 KiB the
/// table starts with, and the one sweep a debt may finish with.
/// Perturbations: a sweep paid for in cells written -- one a round, each
/// reading the whole buffer; an `ED 3` that pays off the debt whatever it
/// erased -- a sweep of the marked screen every round.
#[test]
fn a_sweep_reads_at_most_four_cells_a_byte() {
    for round in [&b"\x1b#8"[..], b"\x1b[3J\x1b[H"] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let mut v = Vec::new();
        let wide = "-".repeat(199);
        for i in 0..3000 {
            v.extend(link(&format!("http://held.example/{}", i % NLINK), &wide));
            v.extend_from_slice(b"!\r\n");
        }
        support::feed(&mut t, &v);
        let before = t.link_sweeps();
        let mut v = Vec::new();
        let mut n = 0;
        for _ in 0..2000 {
            v.extend_from_slice(round);
            for _ in 0..8 {
                v.extend(link(&format!("http://new.example/{n}"), "b"));
                n += 1;
            }
        }
        support::feed(&mut t, &v);
        let after = t.link_sweeps();
        let read = after.read - before.read;
        let one = t.cells().len() + 2 * 2050;
        assert!(
            read <= 4 * (v.len() + 64 * 1024) + one,
            "{round:?}: {read} cells read by {} sweeps for {} bytes",
            after.sweeps - before.sweeps,
            v.len()
        );
        assert!(
            after.sweeps - before.sweeps >= 2,
            "the control: sweeps ran, {after:?}"
        );
    }
}

/// **An `ED 3` pays off no more than a cell for each byte read**, however
/// often it erases a row that held a link. The alternate screen holds the
/// 127 targets on every cell, where `ED 3` cannot reach them; back on the
/// primary each round writes one linked character and a newline, which
/// scrolls a linked row into the history, then `ED 3` and one new target.
/// An erase that paid off the whole debt let every round sweep the
/// alternate screen, freeing nothing: 178 cells read a byte with the
/// default history, 150 with a history of one line. Counted, the sweeps
/// read no more than five cells a byte -- four the bytes pay, one the
/// erases -- the 64 KiB the table starts with, the 64 KiB an erase may
/// hold, and the one sweep a debt may finish with. Perturbation: an
/// `ED 3` that pays off the whole debt -- a sweep every round.
#[test]
fn an_erase_of_the_history_pays_off_at_most_a_cell_a_byte() {
    let ws = support::CORPUS;
    for (depth, rounds) in [(None, 1000), (Some(1), 20_000), (Some(10), 20_000)] {
        let mut s = match depth {
            None => Owned::new(&ws),
            Some(d) => Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::of(d)),
        };
        let mut t = Term::new(s.storage(), &ws);
        let (rows, cols) = (usize::from(ws.ws_row), usize::from(ws.ws_col));
        let mut v = b"\x1b[?47h\x1b[H".to_vec();
        for i in 0..rows * cols {
            v.extend_from_slice(
                format!("\x1b]8;;http://held.example/{}\x1b\\L", i % (NLINK - 1)).as_bytes(),
            );
        }
        v.extend_from_slice(b"\x1b]8;;\x1b\\\x1b[?47l\x1b[H");
        for _ in 0..rows + 5 {
            v.extend(link("http://held.example/0", "x"));
            v.extend_from_slice(b"\r\n");
        }
        support::feed(&mut t, &v);
        let before = t.link_sweeps();
        let mut v = Vec::new();
        for n in 0..rounds {
            v.extend(link("http://held.example/0", "x"));
            v.extend_from_slice(b"\r\n\x1b[3J");
            v.extend(link(&format!("http://new.example/{n}"), "b"));
        }
        support::feed(&mut t, &v);
        let after = t.link_sweeps();
        let read = after.read - before.read;
        let one = t.cells().len() + 2 * 2050;
        assert!(
            read <= 5 * v.len() + 8 * 64 * 1024 + one,
            "depth {depth:?}: {read} cells read by {} sweeps for {} bytes",
            after.sweeps - before.sweeps,
            v.len()
        );
        assert!(
            after.sweeps - before.sweeps >= 2,
            "the control: sweeps ran, {after:?}"
        );
    }
}

/// **A reset pays off no more than an erase of the history does.** Each
/// cycle `RIS`, the 127 targets on the alternate screen, and back on the
/// primary a `DECALN` under one of them, a screen at a time, through the
/// whole history, then one new target: the sweep it asks for reads every
/// row of the buffer. A reset that paid off the whole debt let every
/// cycle sweep the buffer: 57 cells read a byte at 200x50. Counted, the
/// sweeps read no more than five cells a byte beside the allowance of
/// `an_erase_of_the_history_pays_off_at_most_a_cell_a_byte`.
/// Perturbation: a reset that pays off the whole debt -- a sweep of the
/// buffer every cycle.
#[test]
fn a_reset_pays_off_at_most_a_cell_a_byte() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let rows = usize::from(ws.ws_row);
    let mut v = Vec::new();
    for n in 0..100 {
        v.extend_from_slice(b"\x1bc\x1b[?47h");
        for i in 0..NLINK - 1 {
            v.extend_from_slice(format!("\x1b]8;;http://held.example/{i}\x1b\\L").as_bytes());
        }
        v.extend_from_slice(b"\x1b]8;;\x1b\\\x1b[?47l\x1b]8;;http://held.example/0\x1b\\");
        for _ in 0..(2000 + rows) / rows + 1 {
            v.extend_from_slice(format!("\x1b#8\x1b[{rows}H").as_bytes());
            v.extend(std::iter::repeat_n(b'\n', rows));
        }
        v.extend(link(&format!("http://new.example/{n}"), "b"));
    }
    support::feed(&mut t, &v);
    let swept = t.link_sweeps();
    let one = t.cells().len() + 2 * 2050;
    assert!(
        swept.read <= 5 * v.len() + 8 * 64 * 1024 + one,
        "{} cells read by {} sweeps for {} bytes",
        swept.read,
        swept.sweeps,
        v.len()
    );
    assert!(swept.sweeps >= 1, "the control: a sweep ran, {swept:?}");
}

/// A GNU `ls --hyperlink=always -1` of `n` entries of `dir`, one a line,
/// each name linked to its file's URL as the listing names it.
fn listing(dir: &str, n: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for i in 0..n {
        let name = format!("entry-{i:04}.1");
        v.extend_from_slice(
            format!("\x1b]8;;file://host.example/{dir}/{name}\x1b\\{name}\x1b]8;;\x1b\\\r\n")
                .as_bytes(),
        );
    }
    v
}

/// The app's storage, provisioned for 400x150 with its history, under an
/// 80x24 window: what `baud-app` builds its terminal on.
fn in_the_apps_storage(f: impl FnOnce(&mut Term<'_>)) {
    let ws = WinSize {
        ws_row: 24,
        ws_col: 80,
    };
    let max = WinSize {
        ws_row: 150,
        ws_col: 400,
    };
    let mut s = Owned::with_geometry(&max, &CacheGeometry::UNKNOWN);
    let mut t = Term::new(s.storage(), &ws);
    f(&mut t);
}

/// **A listing after `clear` links, however many listings came before**,
/// in the app's storage. Three listings of 1,179 targets and one of 1,000
/// left a sweep paid for in the ring's cells -- 940,000 of them, where
/// each listing's bytes paid for a few -- so after `clear` (`ED 3`, `ED 2`)
/// the next listing linked nothing until 235 KB more had been written. A
/// sweep now reads only the rows that hold links. Perturbation: a sweep
/// that reads every row -- the listing after the clear is plain.
#[test]
fn a_listing_after_clear_links_in_the_apps_storage() {
    in_the_apps_storage(|t| {
        for _ in 0..3 {
            support::feed(t, &listing("usr/share/man/man1", 1179));
        }
        support::feed(t, &listing("usr/bin", 1000));
        let refused = t.links_refused;
        let mut v = b"\x1b[3J\x1b[H\x1b[2J".to_vec();
        v.extend(listing("usr/sbin", 20));
        support::feed(t, &v);
        assert_eq!(t.links_refused, refused, "nothing after the clear refused");
        let vp = t.viewport();
        for y in 0..20 {
            let want = format!("file://host.example/usr/sbin/entry-{y:04}.1");
            assert_eq!(
                t.link_uri(vp.row(Row::from_index(y))[0].flags.link()),
                Some(want.as_bytes()),
                "row {y}"
            );
        }
    });
}

/// **A listing past the table sweeps a few times, not every eight
/// links.** While every slot is named by rows still in the buffer a sweep
/// frees nothing, and the next waits for twice as many links, up to 127,
/// or for a screen of cells: 1,179 entries in the app's storage swept 136
/// times when each eight links asked. Perturbation: no wait after a sweep
/// that freed nothing -- a sweep every eight links.
#[test]
fn a_listing_past_the_table_sweeps_a_few_times() {
    in_the_apps_storage(|t| {
        support::feed(t, &listing("usr/share/man/man1", 1179));
        let swept = t.link_sweeps();
        assert_eq!(swept.freed, 0, "the control: every slot stayed named");
        assert!(
            (1..=40).contains(&swept.sweeps),
            "{} sweeps for {} refused targets",
            swept.sweeps,
            t.links_refused
        );
    });
}

/// **A clear pays for the sweep after it.** A screen of 80 columns and
/// 3,000 lines each linking one of 127 targets across the row hold the
/// table full on every row of the history, so each sweep the next targets
/// ask for reads all of them, frees nothing and spends more than the bytes
/// between pay for. After `clear` (`ED 3`, `ED 2`) the next listing is
/// linked from its first entry. Perturbation: an `ED 3` that pays nothing
/// -- the listing waits out the debt, plain.
#[test]
fn a_clear_pays_for_the_sweep_after_it() {
    in_the_apps_storage(|t| {
        let wide = "-".repeat(79);
        let mut v = Vec::new();
        for i in 0..3000 {
            v.extend(link(&format!("http://held.example/{}", i % NLINK), &wide));
            v.extend_from_slice(b"\r\n");
        }
        for r in 0..60 {
            v.extend(std::iter::repeat_n(b'.', 80 * 24));
            for k in 0..8 {
                v.extend(link(&format!("http://more.example/{r}/{k}"), "x"));
            }
        }
        support::feed(t, &v);
        let swept = t.link_sweeps();
        assert!(
            swept.sweeps >= 2 && swept.freed == 0,
            "the control: sweeps ran and found every slot named, {swept:?}"
        );
        let refused = t.links_refused;
        let mut v = b"\x1b[3J\x1b[H\x1b[2J".to_vec();
        v.extend(listing("usr/sbin", 20));
        support::feed(t, &v);
        assert_eq!(t.links_refused, refused, "nothing after the clear refused");
        let vp = t.viewport();
        for y in 0..20 {
            let want = format!("file://host.example/usr/sbin/entry-{y:04}.1");
            assert_eq!(
                t.link_uri(vp.row(Row::from_index(y))[0].flags.link()),
                Some(want.as_bytes()),
                "row {y}"
            );
        }
    });
}

/// **A sweep reads the rows that hold links, not the buffer**: a program
/// on the alternate screen that draws a screen of sixty new targets a
/// frame, over 3,000 lines of plain history, has its links reclaimed every
/// frame. A sweep of every row read the app's 940,000 cells, a quarter of a
/// megabyte of credit, where a frame is 6 KB; it reads the screen's rows
/// now.
/// Perturbation: a sweep that reads every row -- frames past the first
/// are plain text.
#[test]
fn a_program_on_the_alternate_screen_keeps_its_links_over_a_full_history() {
    in_the_apps_storage(|t| {
        let mut v = Vec::new();
        for i in 0..3000 {
            v.extend_from_slice(format!("{i:>79}\r\n").as_bytes());
        }
        v.extend_from_slice(b"\x1b[?1049h");
        support::feed(t, &v);
        let refused = t.links_refused;
        for frame in 0..40 {
            let mut v = b"\x1b[H\x1b[2J".to_vec();
            for k in 0..60 {
                let r = 1 + k % 24;
                let c = 1 + (k / 24) * 16;
                v.extend_from_slice(format!("\x1b[{r};{c}H").as_bytes());
                v.extend(link(
                    &format!("http://frame.example/{frame}/{k}"),
                    &format!("{k:<16}"),
                ));
            }
            support::feed(t, &v);
        }
        assert_eq!(t.links_refused, refused, "{:?}", t.link_sweeps());
    });
}

/// Cycling through far more links than the table holds does not grow it and
/// does not panic: past 127 distinct targets named at once every new link is
/// refused, the refusals are counted, and the links that took a slot keep
/// it.
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
/// the 8-byte width is a measured number, and a ninth byte would round
/// the cell to twelve.
#[test]
fn the_cell_is_still_eight_bytes() {
    assert_eq!(core::mem::size_of::<PackedCell>(), 8);
}

/// **The pen's open link is the one left open, until it is closed**: an
/// `OSC 8` with no close leaves its slot open after the read, the text
/// after it written under it; the close, and `RIS`, leave none. Named
/// break: answer the last link interned -- the closed link stays open.
#[test]
fn the_open_link_is_the_one_left_open_until_closed() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    assert_eq!(t.open_link(), None, "none at power-on");
    support::feed(&mut t, b"\x1b]8;;file:///etc/passwd\x1b\\x");
    let open = t.open_link().expect("left open");
    assert_eq!(t.link_uri(open), Some(&b"file:///etc/passwd"[..]));
    support::feed(&mut t, b"y");
    assert_eq!(
        support::screen(&t)[1].flags.link(),
        open,
        "written under it"
    );
    support::feed(&mut t, b"\x1b]8;;\x1b\\");
    assert_eq!(t.open_link(), None, "closed");
    support::feed(&mut t, b"\x1b]8;;http://example.com/\x1b\\\x1bc");
    assert_eq!(t.open_link(), None, "RIS");
}
