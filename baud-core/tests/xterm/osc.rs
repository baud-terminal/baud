//! OSC dispatch: titles, the colour palette, and the clipboard refusal.
//!
//! `[[source: ctlseqs]]` § "Set Text Parameters" gives the form as
//! `OSC Ps ; Pt BEL` or `OSC Ps ; Pt ST`, with both terminators accepted, and
//! the operations as: "Ps = 0 -> Change Icon Name and Window Title to Pt.
//! Ps = 1 -> Change Icon Name to Pt. Ps = 2 -> Change Window Title to Pt.
//! Ps = 4 ; c ; spec -> Change Color Number c to the color specified by spec."
//!
//! Every byte here is attacker-controlled -- a title is whatever the far end of
//! an ssh session sent -- so the bounds are part of the protocol, not an
//! implementation detail.

use crate::support;
use baud_core::*;

struct Out {
    title: Vec<u8>,
    icon: Vec<u8>,
    reply: Vec<u8>,
    pal: Vec<Option<(u8, u8, u8)>>,
}

fn run(bytes: &[u8]) -> Out {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    Out {
        title: t.title().to_vec(),
        icon: t.icon_name().to_vec(),
        reply: t.reply().to_vec(),
        pal: (0..=255u8).map(|i| t.palette(Chan::from_u8(i))).collect(),
    }
}

// ── titles ──────────────────────────────────────────────────────────────────

/// `OSC 2` sets the window title and leaves the icon name alone.
#[test]
fn osc_2_sets_the_window_title() {
    let o = run(b"\x1b]2;hello\x07");
    assert_eq!(o.title, b"hello");
    assert!(o.icon.is_empty());
}

/// `OSC 1` sets the icon name only.
#[test]
fn osc_1_sets_the_icon_name() {
    let o = run(b"\x1b]1;icon\x07");
    assert_eq!(o.icon, b"icon");
    assert!(o.title.is_empty());
}

/// `OSC 0` sets both.
#[test]
fn osc_0_sets_both() {
    let o = run(b"\x1b]0;both\x07");
    assert_eq!(o.title, b"both");
    assert_eq!(o.icon, b"both");
}

/// Both terminators work. `[[source: ctlseqs]]`: "XTerm accepts either BEL or
/// ST for terminating OSC sequences."
#[test]
fn both_terminators_are_accepted() {
    assert_eq!(run(b"\x1b]2;bel\x07").title, b"bel");
    assert_eq!(run(b"\x1b]2;st\x1b\\").title, b"st");
}

/// A title may contain semicolons: only the *first* separates `Ps` from `Pt`.
#[test]
fn a_title_may_contain_semicolons() {
    assert_eq!(run(b"\x1b]2;a;b;c\x07").title, b"a;b;c");
}

/// An empty title clears it rather than being ignored.
#[test]
fn an_empty_title_clears_it() {
    assert!(run(b"\x1b]2;set\x07\x1b]2;\x07").title.is_empty());
}

/// An overlong title is truncated, not dropped.
///
/// Dropping it would leave the previous title showing with no signal that
/// anything happened; growing to fit is not available to a `no_std` core.
#[test]
fn an_overlong_title_is_truncated() {
    let mut v = b"\x1b]2;".to_vec();
    v.extend(std::iter::repeat_n(b'x', OSC_MAX * 3));
    v.push(0x07);
    let o = run(&v);
    // The cap is on the whole payload, `2;` included -- so the title is two
    // bytes shorter than `OSC_MAX`, not equal to it. Asserting `OSC_MAX` here
    // would be asserting the wrong invariant and would break the moment a
    // longer `Ps` was supported.
    assert_eq!(o.title.len(), OSC_MAX - b"2;".len());
    assert!(o.title.iter().all(|&b| b == b'x'));
    assert!(
        o.title.len() <= OSC_MAX,
        "bounded, whatever the prefix costs"
    );
}

/// A bare `ESC` inside the payload cancels the string.
///
/// `[[source: williams]]`: "receiving the ESC character will cancel the control
/// string." The title must not be set, and the cancelled-to sequence must run.
#[test]
fn a_bare_esc_cancels_the_osc() {
    let o = run(b"\x1b]2;never\x1bE");
    assert!(o.title.is_empty(), "a cancelled OSC must not set the title");
}

/// A trailing NUL is not part of the title.
#[test]
fn a_trailing_nul_is_trimmed() {
    assert_eq!(run(b"\x1b]2;title\0\x07").title, b"title");
}

/// Titles survive however the read boundaries fall.
///
/// The fast path copies the payload out of the arena in one go and the
/// byte-at-a-time machine accumulates it; a payload handled in only one of them
/// is the two-feeds divergence this codebase keeps having.
#[test]
fn osc_is_chunk_invariant() {
    let seq = b"\x1b]0;a title here\x1b\\rest";
    let whole = run(seq).title;
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
        assert_eq!(t.title(), &whole[..], "differed at chunk {chunk}");
    }
}

// ── palette ─────────────────────────────────────────────────────────────────

/// `OSC 4` sets a palette entry from an `rgb:` spec.
#[test]
fn osc_4_sets_a_palette_entry() {
    let o = run(b"\x1b]4;9;rgb:ff/00/80\x07");
    assert_eq!(o.pal[9], Some((0xff, 0x00, 0x80)));
    assert_eq!(o.pal[8], None, "only the named index changes");
}

/// The `#RRGGBB` spelling works too.
#[test]
fn osc_4_accepts_the_hash_form() {
    assert_eq!(run(b"\x1b]4;1;#ff0080\x07").pal[1], Some((0xff, 0, 0x80)));
}

/// XParseColor scales short digit runs from the top: `rgb:f/f/f` is white.
///
/// Reading it as rgb(15,15,15) would make every 1-digit spec nearly black.
#[test]
fn short_hex_runs_scale_to_the_top() {
    assert_eq!(run(b"\x1b]4;2;rgb:f/f/f\x07").pal[2], Some((255, 255, 255)));
    assert_eq!(run(b"\x1b]4;3;rgb:0/0/0\x07").pal[3], Some((0, 0, 0)));
    assert_eq!(
        run(b"\x1b]4;4;rgb:ffff/0000/ffff\x07").pal[4],
        Some((255, 0, 255)),
        "four digits per channel is the same colour as one"
    );
}

/// `OSC 104` with an index resets that entry; with none, all of them.
#[test]
fn osc_104_resets_the_palette() {
    let o = run(b"\x1b]4;5;rgb:ff/00/00\x07\x1b]104;5\x07");
    assert_eq!(o.pal[5], None);

    let all = run(b"\x1b]4;5;rgb:ff/00/00\x07\x1b]4;6;rgb:00/ff/00\x07\x1b]104\x07");
    assert_eq!(all.pal[5], None);
    assert_eq!(all.pal[6], None);
}

/// `OSC 4 ; c ; ?` replies in the same form, with the same terminator.
///
/// `[[source: ctlseqs]]`: "xterm replies with a control sequence of the same
/// form which can be used to set the corresponding color", and it "uses the
/// same terminator used in a query".
#[test]
fn osc_4_query_replies_in_the_same_form() {
    let o = run(b"\x1b]4;9;rgb:ff/00/80\x07\x1b]4;9;?\x07");
    assert_eq!(o.reply, b"\x1b]4;9;rgb:ff/00/80\x07");

    let st = run(b"\x1b]4;9;rgb:ff/00/80\x07\x1b]4;9;?\x1b\\");
    assert_eq!(
        st.reply, b"\x1b]4;9;rgb:ff/00/80\x1b\\",
        "an ST query must get an ST reply"
    );
}

/// Querying a colour nothing overrode says nothing.
///
/// The core holds no built-in palette -- index-to-pixels belongs to the
/// renderer -- so inventing a value here would answer with a colour the
/// renderer does not actually use.
#[test]
fn querying_an_unset_colour_is_silent() {
    assert!(run(b"\x1b]4;7;?\x07").reply.is_empty());
}

/// A colour *name* is not guessed at.
///
/// Resolving `red` needs X11's rgb.txt, which baud has no citable copy of.
/// Approximating it would be inventing a protocol, so the entry is left alone.
#[test]
fn a_colour_name_is_not_approximated() {
    assert_eq!(run(b"\x1b]4;3;red\x07").pal[3], None);
}

/// A malformed spec does not corrupt the entry.
#[test]
fn a_malformed_spec_is_ignored() {
    assert_eq!(run(b"\x1b]4;3;rgb:zz/00/00\x07").pal[3], None);
    assert_eq!(run(b"\x1b]4;999;rgb:ff/ff/ff\x07").pal[255], None);
}

// ── clipboard ───────────────────────────────────────────────────────────────

/// `OSC 52` read gets **no** answer.
///
/// A hostile program on the far end of an ssh session could otherwise read back
/// whatever the user last copied, which may be a password. Answering with
/// anything -- including an empty payload -- tells it the feature works, so the
/// refusal is silence.
#[test]
fn a_clipboard_read_is_refused_silently() {
    assert!(run(b"\x1b]52;c;?\x07").reply.is_empty());
}

/// A clipboard write is accepted and does nothing here: the core has no
/// clipboard. It must not be mistaken for text and printed.
#[test]
fn a_clipboard_write_is_consumed() {
    let o = run(b"\x1b]52;c;aGVsbG8=\x07");
    assert!(o.reply.is_empty());
    assert!(o.title.is_empty(), "the payload is not a title");
}

/// An unknown OSC is consumed, not printed.
#[test]
fn an_unknown_osc_is_consumed() {
    let o = run(b"\x1b]777;whatever\x07");
    assert!(o.title.is_empty());
    assert!(o.reply.is_empty());
}

/// A C0 byte inside an OSC payload is ignored, not stored.
///
/// `[[source: williams]]`'s `osc_string` state: `01-17,19,1C-1F` take the
/// *ignore* action and only `20-7F` take `osc_put`. baud appended everything
/// that was not BEL or ESC, so a raw control byte landed in the title — an
/// attacker-supplied string carrying bytes the terminal had already been told
/// to drop.
#[test]
fn a_c0_in_an_osc_payload_is_ignored() {
    assert_eq!(
        run(b"\x1b]0;ab\x01cd\x07").title,
        run(b"\x1b]0;abcd\x07").title,
        "the SOH must not reach the title"
    );
}

/// **`OSC 52` is refused, and the refusal is countable.**
///
/// A clipboard *read* gets no reply, and that is load-bearing: answering with
/// anything, empty string included, tells a remote program the feature works
/// and it will keep asking — and what it would read back is whatever the user
/// last copied, which may be a password.
///
/// The silence was also invisible, which is a different thing. An embedder
/// that wants to implement the clipboard has to know the requests are
/// arriving, and `docs/vt-coverage.md` refuses "did something, told nobody"
/// everywhere else.
#[test]
fn clipboard_requests_are_refused_but_counted() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let feed = |t: &mut Term, bytes: &[u8]| {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
    };
    feed(&mut t, b"\x1b]52;c;?\x07");
    assert_eq!(t.clipboard_reads_refused, 1, "the read is counted");
    assert!(t.reply().is_empty(), "and answered with silence");

    feed(&mut t, b"\x1b]52;c;aGVsbG8=\x07");
    assert_eq!(t.clipboard_writes_dropped, 1, "the write is counted");
    assert!(t.reply().is_empty(), "writes are not answered either");
    // Recognised rather than refused, and `baud-core` says so with the two
    // counters that exist for exactly this: a read is refused and counted, a
    // write dropped and counted. `strings_ignored` is what an *unrecognised*
    // OSC increments, so zero there is the assertion core2 spells with its
    // `unsupported` counter.
    assert_eq!(t.strings_ignored, 0, "OSC 52 is recognised, not ignored");
}

// ── dynamic colours ─────────────────────────────────────────────────────────

/// Feed `bytes` after the embedder has said what its theme draws, the way
/// both drivers do through `host::tell_defaults`, and return the reply and
/// the two dynamic colours.
/// An `(r, g, b)` triple, as `Term::dynamic_color` answers.
type Rgb = (u8, u8, u8);

fn dynamic(defaults: Option<(Rgb, Rgb)>, bytes: &[u8]) -> (Vec<u8>, [Option<Rgb>; 2]) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    if let Some((fg, bg)) = defaults {
        t.set_default_colors(fg, bg);
    }
    support::feed(&mut t, bytes);
    (
        t.reply().to_vec(),
        [
            t.dynamic_color(DynamicColor::Foreground),
            t.dynamic_color(DynamicColor::Background),
        ],
    )
}

/// `OSC 10` and `OSC 11` set the default foreground and background, which the
/// renderer reads through `Term::dynamic_color` before its own theme.
#[test]
fn osc_10_and_11_set_the_dynamic_colours() {
    let (_, d) = dynamic(None, b"\x1b]10;rgb:ff/00/80\x07\x1b]11;#102030\x07");
    assert_eq!(d, [Some((0xff, 0x00, 0x80)), Some((0x10, 0x20, 0x30))]);
}

/// **A query on a fresh terminal answers the theme, once the embedder said
/// what it is.** That is the case every theme-aware program depends on: it
/// sends `OSC 11 ; ?` to learn whether it is on a light or a dark background,
/// and a terminal that never answers leaves it guessing.
#[test]
fn osc_11_query_answers_the_embedders_default() {
    let (reply, _) = dynamic(
        Some(((0xe0, 0xe0, 0xe0), (0x11, 0x11, 0x11))),
        b"\x1b]11;?\x07",
    );
    assert_eq!(reply, b"\x1b]11;rgb:11/11/11\x07");
    let (reply, _) = dynamic(
        Some(((0xe0, 0xe0, 0xe0), (0x11, 0x11, 0x11))),
        b"\x1b]10;?\x1b\\",
    );
    assert_eq!(
        reply, b"\x1b]10;rgb:e0/e0/e0\x1b\\",
        "an ST query must get an ST reply"
    );
}

/// An application's override wins the query over the theme, and `OSC 111`
/// puts the theme back -- reset means the default, not nothing.
#[test]
fn an_override_wins_the_query_and_reset_restores_the_default() {
    let defaults = Some(((0xe0, 0xe0, 0xe0), (0x11, 0x11, 0x11)));
    let (reply, d) = dynamic(defaults, b"\x1b]11;rgb:aa/bb/cc\x07\x1b]11;?\x07");
    assert_eq!(reply, b"\x1b]11;rgb:aa/bb/cc\x07");
    assert_eq!(d[1], Some((0xaa, 0xbb, 0xcc)));
    let (reply, d) = dynamic(
        defaults,
        b"\x1b]11;rgb:aa/bb/cc\x07\x1b]111\x07\x1b]11;?\x07",
    );
    assert_eq!(reply, b"\x1b]11;rgb:11/11/11\x07");
    assert_eq!(d[1], None, "the renderer draws its theme again");
}

/// **A core that was never told its defaults says nothing**, for the reason
/// `OSC 4` says nothing about an entry nothing set: naming a colour the
/// renderer does not use is a wrong-shaped answer. The refusal is the truthful
/// reply of a core that has not been told, not an omission.
#[test]
fn a_query_with_no_defaults_and_no_override_is_silent() {
    let (reply, _) = dynamic(None, b"\x1b]10;?\x07\x1b]11;?\x07");
    assert!(reply.is_empty());
}

/// `OSC 12` -- the cursor colour -- is parsed and **counted**, not stored:
/// no renderer here reads a cursor colour yet, and a value recorded and never
/// read is the failure the coverage table rates worst. The day one does, this
/// test is the one that changes.
#[test]
fn osc_12_is_counted_and_not_stored() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"\x1b]12;rgb:ff/00/00\x07");
    assert_eq!(t.unsupported, 1, "OSC 12 is counted as unsupported");
}

/// The same bytes at every read size produce the same colours and the same
/// reply: an `OSC` spans reads, and the value must not depend on where they
/// fell. The core's own chunk-invariance sweep does not reach this, because
/// no recording in the corpus carries an `OSC 10` or `11`.
#[test]
fn dynamic_colours_are_chunk_invariant() {
    let seq = b"\x1b]10;rgb:12/34/56\x1b\\\x1b]11;#abcdef\x07\x1b]11;?\x07after";
    let (whole_reply, whole) = dynamic(None, seq);
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
        let got = [
            t.dynamic_color(DynamicColor::Foreground),
            t.dynamic_color(DynamicColor::Background),
        ];
        assert_eq!(got, whole, "colours differed at chunk {chunk}");
        assert_eq!(
            t.reply(),
            &whole_reply[..],
            "reply differed at chunk {chunk}"
        );
    }
}
