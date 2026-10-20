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
    assert_eq!(o.reply, b"\x1b]4;9;rgb:ffff/0000/8080\x07");

    let st = run(b"\x1b]4;9;rgb:ff/00/80\x07\x1b]4;9;?\x1b\\");
    assert_eq!(
        st.reply, b"\x1b]4;9;rgb:ffff/0000/8080\x1b\\",
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

/// **A colour name is the X server's.** `[[source: ctlseqs]]`: "The spec can
/// be a name or RGB specification as per XParseColor", which sends a name to
/// the server, whose table is X.Org's rgb.txt
/// (`[[source: xorg-rgb-0d2caec]]`) and whose lookup ignores case and
/// nothing else (`[[source: xserver-oscolor-98aed9d]]`). The sixteen
/// colours are the resource file in xterm's FAQ
/// (`[[source: xterm-faq]]` 7609-7669), and its `cursorColor`, `lime
/// green` (7683), is a name with a space in it.
#[test]
fn xterms_own_colour_names_resolve() {
    let mut bytes = Vec::new();
    for (i, name, _) in FAQ {
        bytes.extend_from_slice(format!("\x1b]4;{i};").as_bytes());
        bytes.extend_from_slice(name);
        bytes.push(0x07);
    }
    let o = run(&bytes);
    for (i, name, want) in FAQ {
        let at = usize::from(i);
        assert_eq!(o.pal[at], Some(want), "{}", String::from_utf8_lossy(name));
    }

    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(
        &mut t,
        b"\x1b]12;lime green\x07\x1b]10;Red\x07\x1b]11;GRAY90\x07",
    );
    assert_eq!(t.unsupported, 0, "every name is resolved, none counted");
    assert_eq!(t.dynamic_color(DynamicColor::Cursor), Some((50, 205, 50)));
    assert_eq!(t.dynamic_color(DynamicColor::Foreground), Some((255, 0, 0)));
    assert_eq!(
        t.dynamic_color(DynamicColor::Background),
        Some((229, 229, 229))
    );
    support::feed(&mut t, b"\x1b]12;?\x07");
    assert_eq!(t.reply(), b"\x1b]12;rgb:3232/cdcd/3232\x07");
}

const FAQ: [(u8, &[u8], Rgb); 16] = [
    (0, b"black", (0, 0, 0)),
    (1, b"red3", (205, 0, 0)),
    (2, b"green3", (0, 205, 0)),
    (3, b"yellow3", (205, 205, 0)),
    (4, b"blue3", (0, 0, 205)),
    (5, b"magenta3", (205, 0, 205)),
    (6, b"cyan3", (0, 205, 205)),
    (7, b"gray90", (229, 229, 229)),
    (8, b"gray30", (77, 77, 77)),
    (9, b"red", (255, 0, 0)),
    (10, b"green", (0, 255, 0)),
    (11, b"yellow", (255, 255, 0)),
    (12, b"blue", (0, 0, 255)),
    (13, b"magenta", (255, 0, 255)),
    (14, b"cyan", (0, 255, 255)),
    (15, b"white", (255, 255, 255)),
];

/// **A name the server does not hold is left alone and counted**: case is
/// the only thing its lookup forgives, so a doubled or trailing space is a
/// different name, and Debian's `DebianRed`, which kitty's table carries,
/// is not in X.Org's. `cms red`, one of the three sample names a stock
/// libX11 installs in its client-side `Xcms.txt`
/// (`[[source: libx11-xcms-txt-b8d5a70]]`), is not built: the Xlib manual
/// reads "zero or more client-side databases" (`[[source:
/// xlib-ch06-b8d5a70]]` 533-537), and the file is "provided just as an
/// example".
#[test]
fn a_name_the_server_lacks_is_counted() {
    for spec in [
        &b"notacolour"[..],
        b"ghost  white",
        b"red ",
        b"DebianRed",
        b"cms red",
        b"1",
    ] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        support::feed(&mut t, &[&b"\x1b]4;3;"[..], spec, b"\x07"].concat());
        assert_eq!(t.palette(Chan::from_u8(3)), None, "{spec:?}");
        assert_eq!(t.unsupported, 1, "{spec:?}");
    }
}

/// A malformed spec does not corrupt the entry.
#[test]
fn a_malformed_spec_is_ignored() {
    assert_eq!(run(b"\x1b]4;3;rgb:zz/00/00\x07").pal[3], None);
    assert_eq!(run(b"\x1b]4;999;rgb:ff/ff/ff\x07").pal[255], None);
}

/// **Every pair of an `OSC 4` is applied, in order.** `[[source: ctlseqs]]`:
/// "Any number of c/spec pairs may be given."
#[test]
fn osc_4_applies_every_pair() {
    let o = run(b"\x1b]4;1;#ff0000;2;rgb:00/ff/00;255;blue\x07");
    assert_eq!(o.pal[1], Some((0xff, 0, 0)));
    assert_eq!(o.pal[2], Some((0, 0xff, 0)));
    assert_eq!(o.pal[255], Some((0, 0, 0xff)));
    assert_eq!(o.pal[3], None, "only the named entries change");
}

/// **A `?` in any pair queries that entry**, each answered in its own
/// reply in the order asked and with the query's terminator: "Because more
/// than one pair of color number and specification can be given in one
/// control sequence, xterm can make more than one reply" (`[[source:
/// ctlseqs]]`). A set between two queries is applied before the second is
/// answered, as xterm's `ChangeAnsiColorRequest` walks the pairs
/// (`[[source: xterm-misc]]` 2993-3023).
#[test]
fn a_query_in_any_pair_is_answered_in_turn() {
    let o = run(b"\x1b]4;9;rgb:ff/00/80;9;?;10;#102030;10;?\x1b\\");
    assert_eq!(
        o.reply,
        &b"\x1b]4;9;rgb:ffff/0000/8080\x1b\\\x1b]4;10;rgb:1010/2020/3030\x1b\\"[..]
    );
}

/// **A bad pair ends the walk and keeps what came before**: xterm "quit[s]
/// on any error" in the index and stops "on any error" in the spec
/// (`[[source: xterm-misc]]` 3003-3020). The request not honoured is
/// counted, once.
#[test]
fn a_bad_pair_stops_the_walk_and_is_counted() {
    for (bytes, kept) in [
        (&b"\x1b]4;1;red;x;blue;3;green\x07"[..], "a bad index"),
        (b"\x1b]4;1;red;2;notacolour;3;green\x07", "a bad spec"),
        (
            b"\x1b]4;1;red;300;blue;3;green\x07",
            "an index past the table",
        ),
        (b"\x1b]4;1;red;3\x07", "an index with no spec"),
    ] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        support::feed(&mut t, bytes);
        assert_eq!(t.palette(Chan::from_u8(1)), Some((255, 0, 0)), "{kept}");
        assert_eq!(t.palette(Chan::from_u8(3)), None, "{kept}");
        assert_eq!(t.unsupported, 1, "{kept}");
    }
}

/// **An index of 256 to 260 names one of xterm's special colours** --
/// "The special colors can also be set by adding the maximum number of
/// colors (e.g., 88 or 256) to these codes in an OSC 4 control" (`[[source:
/// ctlseqs]]`), `MAXCOLORS` being 261 (`[[source: xterm-ptyx-h]]`) -- set,
/// and the walk goes on.
#[test]
fn a_special_colour_by_osc_4_is_set_and_the_walk_goes_on() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"\x1b]4;256;red;260;blue;5;#00ff00\x07");
    assert_eq!(t.special_color(SpecialColor::Bold), Some((255, 0, 0)));
    assert_eq!(t.special_color(SpecialColor::Italic), Some((0, 0, 255)));
    assert_eq!(t.palette(Chan::from_u8(5)), Some((0, 255, 0)));
    assert_eq!(t.unsupported, 0);
}

/// **`OSC 104` resets every entry it names.** `[[source: ctlseqs]]`: "Any
/// number of c parameters may be given ... If no parameters are given, the
/// entire table will be reset."
#[test]
fn osc_104_resets_every_entry_it_names() {
    let set = b"\x1b]4;1;red;2;red;3;red\x07";
    let o = run(&[&set[..], b"\x1b]104;1;2\x07"].concat());
    assert_eq!((o.pal[1], o.pal[2]), (None, None));
    assert_eq!(o.pal[3], Some((255, 0, 0)), "an entry not named is kept");
}

/// **An index is its digits, however many**: `[[source: ctlseqs]]` gives
/// `c` as a colour number, and xterm reads it with `atoi`
/// (`[[source: xterm-misc]]` 3001), so `0001` is entry 1 in `OSC 4` and in
/// `OSC 104`. A field that is not digits is still refused.
#[test]
fn an_index_with_leading_zeros_is_its_number() {
    let o = run(b"\x1b]4;0001;red;00000000000000000002;blue\x07");
    assert_eq!(o.pal[1], Some((255, 0, 0)));
    assert_eq!(o.pal[2], Some((0, 0, 255)));
    let o = run(b"\x1b]4;1;red;2;red\x07\x1b]104;0001\x07");
    assert_eq!((o.pal[1], o.pal[2]), (None, Some((255, 0, 0))));
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"\x1b]4;+1;red\x07");
    assert_eq!(t.palette(Chan::from_u8(1)), None, "a sign is not a digit");
    assert_eq!(t.unsupported, 1);
}

/// **`OSC 104` goes on past a number out of range**, as xterm's
/// `ResetAnsiColorRequest` passes it to `ResetOneAnsiColor`, which ignores
/// it (`[[source: xterm-misc]]` 3036-3083): `104;1;999;2` resets 1 and 2.
/// The number ignored is counted. A field that is no number still ends it.
#[test]
fn osc_104_goes_on_past_a_number_out_of_range() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"\x1b]4;1;red;2;red;3;red\x07\x1b]104;1;999;2\x07");
    assert_eq!(t.palette(Chan::from_u8(1)), None);
    assert_eq!(t.palette(Chan::from_u8(2)), None);
    assert_eq!(t.palette(Chan::from_u8(3)), Some((255, 0, 0)));
    assert_eq!(t.unsupported, 1, "999");
    support::feed(&mut t, b"\x1b]104;3x;1\x07");
    assert_eq!(
        t.palette(Chan::from_u8(3)),
        Some((255, 0, 0)),
        "3x is no number"
    );
}

// ── special colours ─────────────────────────────────────────────────────────

fn special_term(bytes: &[u8], f: impl FnOnce(&Term<'_>)) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.set_default_colors((0xe0, 0xe0, 0xe0), (0x11, 0x11, 0x11));
    support::feed(&mut t, bytes);
    f(&t);
}

/// **`OSC 5` sets and answers xterm's special colours**: "Change Special
/// Color Number c to the color specified by spec", any number of pairs,
/// and a `?` answered "with a control sequence of the same form"
/// (`[[source: ctlseqs]]`): `OSC 5 ; c` when asked by `OSC 5`, `OSC 4 ;
/// 256+c` when asked by `OSC 4`, as xterm's `ReportAnsiColorRequest`
/// writes them. A colour no program set answers the text foreground it
/// draws in. A number past the five ends the walk, counted.
#[test]
fn osc_5_sets_and_answers_the_special_colours() {
    special_term(
        b"\x1b]5;0;red;4;#0000ff\x07\x1b]5;0;?;1;?\x07\x1b]4;260;?\x1b\\",
        |t| {
            assert_eq!(t.special_color(SpecialColor::Bold), Some((255, 0, 0)));
            assert_eq!(t.special_color(SpecialColor::Italic), Some((0, 0, 255)));
            assert_eq!(t.special_color(SpecialColor::Underline), None);
            assert_eq!(
                t.reply(),
                &b"\x1b]5;0;rgb:ffff/0000/0000\x07\x1b]5;1;rgb:e0e0/e0e0/e0e0\x07\
\x1b]4;260;rgb:0000/0000/ffff\x1b\\"[..]
            );
            assert_eq!(t.unsupported, 0);
        },
    );
    special_term(b"\x1b]5;2;red;5;blue;3;red\x07", |t| {
        assert_eq!(t.special_color(SpecialColor::Blink), Some((255, 0, 0)));
        assert_eq!(
            t.special_color(SpecialColor::Reverse),
            None,
            "the walk ended"
        );
        assert_eq!(t.unsupported, 1);
    });
}

/// **`OSC 105` resets the special colours it names, and all with none**:
/// "Reset Special Color Number c ... If no parameters are given, all
/// special colors will be reset" (`[[source: ctlseqs]]`). `OSC 104`
/// resets them too, by 256 more and with no parameter, as xterm's reset
/// walks all of its 261 colours.
#[test]
fn osc_105_and_104_reset_the_special_colours() {
    let set = b"\x1b]5;0;red;1;red;2;red\x07";
    special_term(&[&set[..], b"\x1b]105;0\x07"].concat(), |t| {
        assert_eq!(t.special_color(SpecialColor::Bold), None);
        assert_eq!(t.special_color(SpecialColor::Underline), Some((255, 0, 0)));
    });
    special_term(&[&set[..], b"\x1b]105\x07"].concat(), |t| {
        assert!(
            SpecialColor::ALL
                .iter()
                .all(|&c| t.special_color(c).is_none())
        );
    });
    special_term(&[&set[..], b"\x1b]104;257\x07"].concat(), |t| {
        assert_eq!(t.special_color(SpecialColor::Underline), None);
        assert_eq!(t.special_color(SpecialColor::Bold), Some((255, 0, 0)));
    });
    special_term(&[&set[..], b"\x1b]104\x07"].concat(), |t| {
        assert!(
            SpecialColor::ALL
                .iter()
                .all(|&c| t.special_color(c).is_none())
        );
    });
}

/// **`OSC 6` and `OSC 106` turn the modes on and off**: "enable the
/// corresponding color mode if nonzero, disable it if zero"
/// (`[[source: ctlseqs]]`), `c` 5 being colorAttrMode as ctlseqs lists it.
/// An empty field before a `;` is 0, as xterm reads it; a `c` with no `f`
/// ends the walk and a `c` no mode has is passed over, each counted.
#[test]
fn osc_6_and_106_turn_the_modes_on_and_off() {
    special_term(b"\x1b]6;0;1;5;7\x07", |t| {
        assert!(t.color_modes().has(ColorMode::Bold));
        assert!(t.color_modes().has(ColorMode::Attr));
        assert!(!t.color_modes().has(ColorMode::Italic));
    });
    special_term(b"\x1b]6;0;1\x07\x1b]106;0;0\x07", |t| {
        assert_eq!(t.color_modes(), ColorModes::NONE);
    });
    special_term(b"\x1b]6;;1;9;1;3;1\x07", |t| {
        assert!(t.color_modes().has(ColorMode::Bold), "an empty c is 0");
        assert!(
            t.color_modes().has(ColorMode::Reverse),
            "past the unknown 9"
        );
        assert_eq!(t.unsupported, 1);
    });
    special_term(b"\x1b]6;1\x07", |t| {
        assert_eq!(t.color_modes(), ColorModes::NONE);
        assert_eq!(t.unsupported, 1);
    });
    special_term(b"\x1b]6\x07", |t| {
        assert_eq!(t.unsupported, 0, "no parameters: no effect");
    });
}

/// **`RIS` turns the modes off and clears the colours; `DECSTR` clears the
/// colours and keeps the modes**: ECMA-48's initial state for the first,
/// and xterm's reset of its 261 colours for both.
#[test]
fn the_resets_clear_the_special_colours() {
    let set = b"\x1b]5;0;red\x07\x1b]6;0;1\x07";
    special_term(&[&set[..], b"\x1bc"].concat(), |t| {
        assert_eq!(t.special_color(SpecialColor::Bold), None);
        assert_eq!(t.color_modes(), ColorModes::NONE);
    });
    special_term(&[&set[..], b"\x1b[!p"].concat(), |t| {
        assert_eq!(t.special_color(SpecialColor::Bold), None);
        assert!(t.color_modes().has(ColorMode::Bold));
    });
}

/// **A special colour or a mode that changes moves the colour epoch**, so a
/// renderer resolves every rendition again under it; one that changes
/// nothing does not.
#[test]
fn a_special_colour_or_mode_moves_the_colour_epoch() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let e0 = t.color_epoch();
    support::feed(&mut t, b"\x1b]6;0;1\x07");
    let e1 = t.color_epoch();
    assert_ne!(e0, e1, "a mode turned on");
    support::feed(&mut t, b"\x1b]6;0;1\x07");
    assert_eq!(t.color_epoch(), e1, "already on");
    support::feed(&mut t, b"\x1b]5;0;red\x07");
    let e2 = t.color_epoch();
    assert_ne!(e2, e1, "a colour set");
    support::feed(&mut t, b"\x1b]5;0;red\x07");
    assert_eq!(t.color_epoch(), e2, "the same colour");
}

// ── allowed features ────────────────────────────────────────────────────────

fn reply_of(bytes: &[u8], f: impl Fn(&mut Term<'_>)) -> (Vec<u8>, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    f(&mut t);
    support::feed(&mut t, bytes);
    (t.reply().to_vec(), t.unsupported)
}

/// **`OSC 60`, XTQALLOWED, names the categories baud honours whole**, in
/// xterm's shape:
/// "XTerm replies with OSC 6 0 ; Pt ST where Pt is a comma-separated list
/// of the allowed optional runtime feature categories" (`[[source:
/// ctlseqs]]`), with the query's terminator. Fonts, termcap queries,
/// most window operations and the pointer and Tektronix colours (`OSC 13`
/// to `16` and `18`, which xterm gates under the same `SetColor` and
/// `GetColor` as `OSC 10`) are refused, so the mouse and the title
/// operations are named: every mouse protocol xterm lists, the DEC locator
/// (DECELR) among them, is honoured.
#[test]
fn osc_60_names_the_categories_baud_honours_whole() {
    let (r, n) = reply_of(b"\x1b]60\x07", |_| {});
    assert_eq!(r, b"\x1b]60;allowMouseOps,allowTitleOps\x07");
    assert_eq!(n, 0);
    let (r, _) = reply_of(b"\x1b]60\x1b\\", |_| {});
    assert_eq!(r, b"\x1b]60;allowMouseOps,allowTitleOps\x1b\\");
}

/// **`OSC 61 ; cat`, XTQDISALLOWED, names the subcategories baud
/// refuses**: "the optional
/// runtime feature subcategories which would be disallowed if the main
/// feature is disabled" (`[[source: ctlseqs]]`), in xterm's model where a
/// category not allowed allows all but its disallowed list. The name is
/// matched in any case, `allowWinOps` is xterm's alias, and a name that is
/// no category gets an empty list, as xterm's `unparse_disallowed_ops`.
/// `OSC 52`'s write is refused on a storage with no clipboard area, and
/// named only there. XTCHECKSUM, `SetChecksum`, is honoured and not named,
/// whatever the checksum setting.
#[test]
fn osc_61_names_what_baud_refuses() {
    let windows: &[u8] = b"\x1b]61;RestoreWin,MinimizeWin,SetWinPosition,\
SetWinSizePixels,RaiseWin,LowerWin,RefreshWin,SetWinSizeChars,MaximizeWin,\
FullscreenWin,GetIconTitle,GetWinTitle,SetWinLines,GetSelection,SetSelection,\
SetXprop\x07";
    for name in ["allowWindowOps", "ALLOWWINDOWOPS", "allowWinOps"] {
        let q = format!("\x1b]61;{name}\x07");
        assert_eq!(reply_of(q.as_bytes(), |_| {}).0, windows, "{name}");
    }
    assert_eq!(
        reply_of(b"\x1b]61;allowFontOps\x07", |_| {}).0,
        b"\x1b]61;SetFont,GetFont\x07"
    );
    assert_eq!(
        reply_of(b"\x1b]61;allowColorOps\x07", |_| {}).0,
        b"\x1b]61;SetColor,GetColor\x07",
        "OSC 13 to 16 and 18 are refused"
    );
    for empty in ["allowTitleOps", "allowMouseOps", "allowPasteControls", "1"] {
        let q = format!("\x1b]61;{empty}\x07");
        let (r, n) = reply_of(q.as_bytes(), |_| {});
        assert_eq!(r, b"\x1b]61\x07", "{empty}");
        assert_eq!(n, 0, "{empty}");
    }
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws).with_clipboard(ClipboardBudget::of_text(64));
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"\x1b]52;c;aGk=\x07\x1b]61;allowWindowOps\x07");
    assert_eq!(t.clipboard_writes_refused, 0, "the write was honoured");
    assert!(
        t.reply()
            .ends_with(b",SetWinLines,GetSelection,SetXprop\x07"),
        "a provisioned clipboard is not refused: {}",
        String::from_utf8_lossy(t.reply())
    );
    let (r, _) = reply_of(b"\x1b]61;allowWindowOps\x07", |t| {
        t.set_checksum_report(baud_domain::ChecksumReport::Off)
    });
    assert!(
        r.windows(b",GetChecksum,".len())
            .any(|w| w == b",GetChecksum,"),
        "DECRQCRA switched off is refused: {}",
        String::from_utf8_lossy(&r)
    );
    assert!(
        !r.windows(b"SetChecksum".len()).any(|w| w == b"SetChecksum"),
        "XTCHECKSUM is honoured with the checksum off"
    );
}

/// **`OSC 62 ; cat`, XTQALLOWABLE, names every subcategory**, "the optional runtime
/// feature subcategories which correspond to the main feature category"
/// (`[[source: ctlseqs]]`), xterm's table for it.
#[test]
fn osc_62_names_every_subcategory() {
    assert_eq!(
        reply_of(b"\x1b]62;allowcolorops\x07", |_| {}).0,
        b"\x1b]62;SetColor,GetColor,GetAnsiColor\x07"
    );
    assert_eq!(
        reply_of(b"\x1b]62;nothing\x1b\\", |_| {}).0,
        b"\x1b]62\x1b\\"
    );
}

/// **A malformed feature query is counted and answered with silence**, as
/// xterm's `do_osc` returns on "found unwanted data" for `OSC 60` and
/// "found no data" for `61` and `62`.
#[test]
fn a_malformed_feature_query_is_counted() {
    for q in [&b"\x1b]60;x\x07"[..], b"\x1b]61\x07", b"\x1b]62;\x07"] {
        let (r, n) = reply_of(q, |_| {});
        assert!(r.is_empty(), "{}", String::from_utf8_lossy(q));
        assert_eq!(n, 1, "{}", String::from_utf8_lossy(q));
    }
}

/// **The longest feature reply comes back whole**: `OSC 62 ;
/// allowWindowOps`, 336 bytes with its `BEL`.
#[test]
fn the_longest_feature_reply_fits() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"\x1b]62;allowWindowOps\x07");
    assert_eq!(t.reply().len(), 336);
    assert!(t.reply().ends_with(b",SetSelection,SetXprop\x07"));
    assert_eq!(t.reply_overflows, 0);
}

// ── the pointer ─────────────────────────────────────────────────────────────

fn pointer_after(bytes: &[u8]) -> (Option<PointerShape>, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    (t.pointer_shape(), t.unsupported)
}

/// **`OSC 22` names the pointer's shape**: "Change pointer cursor shape to
/// Pt" (`[[source: ctlseqs]]`), one of the 77 names of xterm's table,
/// matched as `strcmp` matches.
#[test]
fn osc_22_sets_the_pointer_shape() {
    assert_eq!(
        pointer_after(b"\x1b]22;hand2\x07"),
        (Some(PointerShape::Hand2), 0)
    );
    assert_eq!(
        pointer_after(b"\x1b]22;xterm\x1b\\"),
        (Some(PointerShape::Xterm), 0)
    );
    assert_eq!(
        pointer_after(b"\x1b]22;X_cursor\x07"),
        (Some(PointerShape::XCursor), 0)
    );
    for s in PointerShape::ALL {
        assert_eq!(PointerShape::of(s.name()), Some(s), "{:?}", s);
    }
}

/// **An empty or unknown name is the embedder's own pointer**: "If Pt is
/// empty, or does not match any of the standard names, xterm uses the
/// resource's default" (`[[source: ctlseqs]]`). xterm's code returns on an
/// empty `Pt` and changes nothing, where the text is followed. An unknown
/// name is counted; so is a name in the wrong case.
#[test]
fn an_empty_or_unknown_osc_22_is_the_embedders_pointer() {
    assert_eq!(pointer_after(b"\x1b]22;hand2\x07\x1b]22;\x07"), (None, 0));
    assert_eq!(pointer_after(b"\x1b]22;hand2\x07\x1b]22\x07"), (None, 0));
    assert_eq!(
        pointer_after(b"\x1b]22;hand2\x07\x1b]22;Hand2\x07"),
        (None, 1)
    );
    assert_eq!(
        pointer_after(b"\x1b]22;hand2\x07\x1b]22;nothing\x07"),
        (None, 1)
    );
}

/// **`RIS` returns the embedder's pointer**, the state after power-up
/// (`[[source: ecma48]]` 8.3.105); `DECSTR` keeps the shape.
#[test]
fn ris_returns_the_embedders_pointer() {
    assert_eq!(pointer_after(b"\x1b]22;watch\x07\x1bc").0, None);
    assert_eq!(
        pointer_after(b"\x1b]22;watch\x07\x1b[!p").0,
        Some(PointerShape::Watch)
    );
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

    // A storage given no clipboard area -- `Owned::new` gives none -- refuses
    // the write and counts it. Writes a provisioned storage honours are
    // `tests/clipboard.rs`'s.
    feed(&mut t, b"\x1b]52;c;aGVsbG8=\x07");
    assert_eq!(t.clipboard_writes_refused, 1, "the write is counted");
    assert_eq!(
        t.clipboard_refusals().unprovisioned,
        1,
        "for want of an area"
    );
    assert!(t.reply().is_empty(), "writes are not answered either");
    // Recognised rather than refused, and `baud-core` says so with the two
    // counters that exist for exactly this: a read is refused and counted, a
    // write refused and counted. `strings_ignored` is what an *unrecognised*
    // OSC increments, so zero there is the assertion core2 spelled with its
    // `unsupported` counter.
    assert_eq!(t.strings_ignored, 0, "OSC 52 is recognised, not ignored");
}

// ── dynamic colours ─────────────────────────────────────────────────────────

/// An `(r, g, b)` triple, as `Term::dynamic_color` answers.
type Rgb = (u8, u8, u8);

/// Feed `bytes` after the embedder has said what its theme draws, the way
/// both drivers do through `host::tell_defaults`, and return the reply and
/// the two dynamic colours.
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
    assert_eq!(reply, b"\x1b]11;rgb:1111/1111/1111\x07");
    let (reply, _) = dynamic(
        Some(((0xe0, 0xe0, 0xe0), (0x11, 0x11, 0x11))),
        b"\x1b]10;?\x1b\\",
    );
    assert_eq!(
        reply, b"\x1b]10;rgb:e0e0/e0e0/e0e0\x1b\\",
        "an ST query must get an ST reply"
    );
}

/// An application's override wins the query over the theme, and `OSC 111`
/// puts the theme back -- reset means the default, not nothing.
#[test]
fn an_override_wins_the_query_and_reset_restores_the_default() {
    let defaults = Some(((0xe0, 0xe0, 0xe0), (0x11, 0x11, 0x11)));
    let (reply, d) = dynamic(defaults, b"\x1b]11;rgb:aa/bb/cc\x07\x1b]11;?\x07");
    assert_eq!(reply, b"\x1b]11;rgb:aaaa/bbbb/cccc\x07");
    assert_eq!(d[1], Some((0xaa, 0xbb, 0xcc)));
    let (reply, d) = dynamic(
        defaults,
        b"\x1b]11;rgb:aa/bb/cc\x07\x1b]111\x07\x1b]11;?\x07",
    );
    assert_eq!(reply, b"\x1b]11;rgb:1111/1111/1111\x07");
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

/// `OSC 12` -- the cursor colour -- is **stored and read**, and `OSC 112`
/// resets it. This is the test the deferral named: it was `is_counted_and_
/// not_stored` while no renderer drew a cursor colour, and it inverted when
/// `baud-render` began to. The set no longer counts as unsupported, the
/// value round-trips through `Term::dynamic_color`, the query answers in the
/// code-12 shape, and `OSC 112` clears it back to none -- there being no
/// embedder cursor default, "none" is the fresh-terminal answer too.
#[test]
fn osc_12_sets_and_reads_the_cursor_colour() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"\x1b]12;rgb:ff/00/00\x07");
    assert_eq!(t.unsupported, 0, "OSC 12 is stored, not counted");
    assert_eq!(
        t.dynamic_color(DynamicColor::Cursor),
        Some((0xff, 0x00, 0x00))
    );

    support::feed(&mut t, b"\x1b]12;?\x07");
    assert_eq!(t.reply(), b"\x1b]12;rgb:ffff/0000/0000\x07");

    support::feed(&mut t, b"\x1b]112\x07");
    assert_eq!(
        t.dynamic_color(DynamicColor::Cursor),
        None,
        "OSC 112 resets it"
    );
}

/// The three dynamic colours a sequence sets, and the count of requests
/// not honoured.
fn walked(defaults: Option<(Rgb, Rgb)>, bytes: &[u8]) -> ([Option<Rgb>; 3], Vec<u8>, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    if let Some((fg, bg)) = defaults {
        t.set_default_colors(fg, bg);
    }
    support::feed(&mut t, bytes);
    (
        [
            t.dynamic_color(DynamicColor::Foreground),
            t.dynamic_color(DynamicColor::Background),
            t.dynamic_color(DynamicColor::Cursor),
        ],
        t.reply().to_vec(),
        t.unsupported,
    )
}

/// **Each spec after the first sets the next colour.** `[[source:
/// ctlseqs]]`: "Each successive parameter changes the next color in the
/// list. The value of Ps tells the starting point in the list", so `OSC 10
/// ; fg ; bg ; cursor` sets all three and `OSC 11 ; bg ; cursor` starts at
/// the background.
#[test]
fn each_spec_after_the_first_sets_the_next_colour() {
    let (d, _, n) = walked(None, b"\x1b]10;#ff0000;#00ff00;#0000ff\x07");
    assert_eq!(d, [Some((255, 0, 0)), Some((0, 255, 0)), Some((0, 0, 255))]);
    assert_eq!(n, 0);
    let (d, _, _) = walked(None, b"\x1b]11;#102030;red\x07");
    assert_eq!(d, [None, Some((0x10, 0x20, 0x30)), Some((255, 0, 0))]);
}

/// **An empty field passes over its colour**, as xterm's
/// `ChangeColorsRequest` takes a field that begins with `;` as no name
/// (`[[source: xterm-misc]]` 3685-3700): `OSC 10 ; ; bg` sets the
/// background alone.
#[test]
fn an_empty_field_leaves_its_colour() {
    let (d, _, n) = walked(None, b"\x1b]10;;#00ff00\x07");
    assert_eq!(d, [None, Some((0, 255, 0)), None]);
    assert_eq!(n, 0);
}

/// **Each `?` in the list is answered, in turn and in its own reply**:
/// "Because more than one pair of color number and specification can be
/// given in one control sequence, xterm can make more than one reply"
/// (`[[source: ctlseqs]]`).
#[test]
fn each_query_in_the_list_is_answered() {
    let defaults = Some(((0xe0, 0xe0, 0xe0), (0x11, 0x11, 0x11)));
    let (_, reply, _) = walked(defaults, b"\x1b]10;?;?\x1b\\");
    assert_eq!(
        reply,
        &b"\x1b]10;rgb:e0e0/e0e0/e0e0\x1b\\\x1b]11;rgb:1111/1111/1111\x1b\\"[..]
    );
}

/// **A number baud keeps no colour for is counted, and the walk goes on**:
/// the pointer's two (13, 14) and Tektronix's three (15, 16, 18), and a spec
/// past 19, the last of xterm's ten.
#[test]
fn a_colour_baud_does_not_keep_is_counted_and_the_walk_goes_on() {
    let (d, _, n) = walked(None, b"\x1b]12;red;blue;blue\x07");
    assert_eq!(d, [None, None, Some((255, 0, 0))]);
    assert_eq!(n, 2, "13 and 14");
    let (_, _, n) = walked(None, b"\x1b]18;red;red;red\x07");
    assert!(n >= 2, "18, and the spec past 19: {n}");
}

/// **A cursor colour equal to the text follows a new text colour**, as
/// xterm's `ChangeColors` moves it when a sequence sets the text colour and
/// not the cursor's (`[[source: xterm-scroll]]` util.c 3062-3070): a
/// program that painted the cursor in the text colour keeps it visible. A
/// cursor colour of its own, or one the same sequence sets, stays.
#[test]
fn a_cursor_colour_equal_to_the_text_follows_osc_10() {
    let (d, _, _) = walked(None, b"\x1b]10;red\x07\x1b]12;red\x07\x1b]10;blue\x07");
    assert_eq!(d[2], Some((0, 0, 255)), "the cursor followed the text");
    let (d, _, _) = walked(None, b"\x1b]10;red\x07\x1b]12;lime\x07\x1b]10;blue\x07");
    assert_eq!(d[2], Some((0, 255, 0)), "a colour of its own stays");
    let (d, _, _) = walked(
        None,
        b"\x1b]10;red\x07\x1b]12;lime\x07\x1b]10;blue;;red\x07",
    );
    assert_eq!(d[2], Some((255, 0, 0)), "the sequence set the cursor too");
    let defaults = Some(((0xe0, 0xe0, 0xe0), (0x11, 0x11, 0x11)));
    let (d, _, _) = walked(defaults, b"\x1b]12;#e0e0e0\x07\x1b]10;blue\x07");
    assert_eq!(d[2], Some((0, 0, 255)), "equal to the theme's text");
    let (d, _, _) = walked(defaults, b"\x1b]10;blue\x07");
    assert_eq!(d[2], None, "a cursor nobody set stays unset");
}

fn colors(bg: Option<Rgb>, fg: Option<Rgb>) -> SelectionColors {
    SelectionColors { bg, fg }
}

/// **`OSC 17` and `OSC 19` set the selection's colours**, read through
/// `Term::highlight_colors`, and `OSC 117` and `OSC 119` reset them to none,
/// reverse video. `[[source: ctlseqs]]`: "Ps = 1 7 -> Change highlight
/// background color to Pt", "Ps = 1 9 -> Change highlight foreground color
/// to Pt", "Ps = 1 1 7 -> Reset highlight color", "Ps = 1 1 9 -> Reset
/// highlight foreground color".
#[test]
fn osc_17_and_19_set_the_selection_colours_and_117_and_119_reset_them() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"\x1b]17;#2f5f9f\x07\x1b]19;gold\x1b\\");
    assert_eq!(t.unsupported, 0, "both are honoured");
    assert_eq!(
        t.highlight_colors(),
        colors(Some((0x2f, 0x5f, 0x9f)), Some((255, 215, 0)))
    );
    support::feed(&mut t, b"\x1b]117\x07");
    assert_eq!(t.highlight_colors(), colors(None, Some((255, 215, 0))));
    support::feed(&mut t, b"\x1b]119\x07");
    assert_eq!(t.highlight_colors(), colors(None, None));
    // Carried on from 17: 18, Tektronix's cursor, is counted, 19 is set.
    support::feed(&mut t, b"\x1b]17;red;blue;#00ff00\x07");
    assert_eq!(
        t.highlight_colors(),
        colors(Some((255, 0, 0)), Some((0, 255, 0)))
    );
    assert_eq!(t.unsupported, 1);
}

/// **A query answers in `OSC 10`'s shape, with the query's terminator**:
/// the colour set, and with none set the colour a selection is drawn in --
/// reverse video, so the text shown for `17` and the page shown for `19`,
/// as a default xterm answers (Jev, 0.90 in both orders) -- and nothing on
/// a core never told its colours.
#[test]
fn a_highlight_query_answers_the_colour_the_selection_is_drawn_in() {
    let defaults = Some(((0xe0, 0xe0, 0xe0), (0x11, 0x11, 0x11)));
    let (_, reply, _) = walked(defaults, b"\x1b]17;?\x07\x1b]19;?\x1b\\");
    assert_eq!(
        reply,
        &b"\x1b]17;rgb:e0e0/e0e0/e0e0\x07\x1b]19;rgb:1111/1111/1111\x1b\\"[..]
    );
    let (_, reply, _) = walked(defaults, b"\x1b]17;#2f5f9f;?;?\x07");
    assert_eq!(
        reply,
        &b"\x1b]19;rgb:1111/1111/1111\x07"[..],
        "18 is counted and answers nothing; 19 is unset"
    );
    let (_, reply, _) = walked(defaults, b"\x1b]17;#2f5f9f\x07\x1b]17;?\x07");
    assert_eq!(reply, &b"\x1b]17;rgb:2f2f/5f5f/9f9f\x07"[..]);
    let (_, reply, _) = walked(None, b"\x1b]17;?\x07\x1b]19;?\x07");
    assert!(reply.is_empty(), "a core never told its colours is silent");
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
