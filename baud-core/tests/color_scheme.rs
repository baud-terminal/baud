//! `CSI ? 996 n`, mode `?2031` and its `CSI ? 997 ; 1|2 n` notification.
//!
//! `[[source: contour-palette-notifications]]` specifies the query, the two
//! replies and the mode; note Y (section 7, decision 5) settles what the spec
//! leaves open: the class is that of what `OSC 10`/`11 ; ?` would answer, one
//! embedder call sends at most one notification, an application's own colour
//! escapes send none, and nothing is sent on `2031 h`.

mod support;

use baud_core::*;

const DARK_FG: (u8, u8, u8) = (0xe5, 0xe5, 0xe5);
const DARK_BG: (u8, u8, u8) = (0x10, 0x10, 0x10);
const LIGHT_FG: (u8, u8, u8) = (0x27, 0x27, 0x27);
const LIGHT_BG: (u8, u8, u8) = (0xef, 0xef, 0xef);
const TABLE: [(u8, u8, u8); 256] = [(1, 2, 3); 256];

const DARK: &[u8] = b"\x1b[?997;1n";
const LIGHT: &[u8] = b"\x1b[?997;2n";

fn theme(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> ThemeColors<'static> {
    ThemeColors {
        fg,
        bg,
        palette: &TABLE,
        selection: None,
        selection_text: None,
        cursor: None,
        cursor_text: None,
        cursor_text_under: baud_domain::themefile::CursorText::UnderEveryCursor,
        unset_selection_text: baud_domain::themefile::SelectionText::Reverse,
    }
}

/// A fresh terminal told the dark theme, fed `bytes` at read sizes 1, 3, 7
/// and whole, its replies handed over.
fn each_read_size(bytes: &[u8], f: impl Fn(&[u8])) {
    let ws = support::CORPUS;
    for size in [1, 3, 7, bytes.len().max(1)] {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.set_theme(&theme(DARK_FG, DARK_BG), Overrides::Keep);
        for chunk in bytes.chunks(size) {
            support::feed(&mut t, chunk);
        }
        f(t.reply());
    }
}

fn drain(t: &mut Term<'_>) -> Vec<u8> {
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

#[test]
fn the_query_answers_the_class_of_the_theme() {
    each_read_size(b"\x1b[?996n", |got| assert_eq!(got, DARK));
    support::with_corpus(|t| {
        t.set_theme(&theme(LIGHT_FG, LIGHT_BG), Overrides::Keep);
        support::feed(t, b"\x1b[?996n");
        assert_eq!(t.reply(), LIGHT);
    });
}

/// Where `OSC 11 ; ?` would be silent -- no embedder told the core its
/// colours -- so is the query: a terminal told nothing has nothing true to
/// say.
#[test]
fn the_query_is_silent_on_a_terminal_told_no_colours() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b]11;?\x07\x1b[?996n");
        assert_eq!(t.reply(), b"", "OSC 11 and 996 both silent");
    });
}

/// The query reads what `OSC 11` answers, override included, so the two can
/// never disagree.
#[test]
fn the_query_follows_an_applications_override() {
    each_read_size(
        b"\x1b]11;rgb:ff/ff/ff\x07\x1b]10;rgb:00/00/00\x07\x1b[?996n",
        |got| assert_eq!(got, LIGHT),
    );
}

#[test]
fn decrqm_answers_the_bit_reset_at_power_on_and_nothing_is_sent_on_set() {
    each_read_size(b"\x1b[?2031$p", |got| assert_eq!(got, b"\x1b[?2031;2$y"));
    each_read_size(b"\x1b[?2031h", |got| {
        assert_eq!(got, b"", "a report on 2031 h")
    });
    each_read_size(b"\x1b[?2031h\x1b[?2031$p", |got| {
        assert_eq!(got, b"\x1b[?2031;1$y")
    });
}

/// RIS clears the mode; DECSTR keeps it, since xterm's soft reset restores
/// only its own list and xterm has no `?2031` -- baud's rule, not xterm's.
#[test]
fn ris_clears_the_mode_and_decstr_keeps_it() {
    each_read_size(b"\x1b[?2031h\x1bc\x1b[?2031$p", |got| {
        assert_eq!(got, b"\x1b[?2031;2$y")
    });
    each_read_size(b"\x1b[?2031h\x1b[!p\x1b[?2031$p", |got| {
        assert_eq!(got, b"\x1b[?2031;1$y")
    });
}

/// Replies leave in the order they were asked: a 996 before a DA1 in one
/// read is answered first, which helix's detection depends on.
#[test]
fn the_reply_precedes_a_da1_asked_after_it() {
    each_read_size(b"\x1b[?996n\x1b[c", |got| {
        assert!(got.starts_with(DARK), "{got:?}");
        assert!(got.ends_with(b"c"), "{got:?}");
    });
}

#[test]
fn an_embedder_flip_reports_once_and_a_repeat_reports_nothing() {
    support::with_corpus(|t| {
        t.set_theme(&theme(LIGHT_FG, LIGHT_BG), Overrides::Keep);
        support::feed(t, b"\x1b[?2031h");
        assert_eq!(drain(t), b"");
        t.set_theme(&theme(DARK_FG, DARK_BG), Overrides::Drop);
        assert_eq!(drain(t), DARK, "the flip");
        t.set_theme(&theme(DARK_FG, DARK_BG), Overrides::Drop);
        assert_eq!(drain(t), b"", "the same class again");
        t.set_default_colors(LIGHT_FG, LIGHT_BG);
        assert_eq!(drain(t), LIGHT, "set_default_colors reports too");
    });
}

/// Without `?2031`, a flip is silent -- the control that the report above is
/// the mode's.
#[test]
fn a_flip_without_the_mode_is_silent() {
    support::with_corpus(|t| {
        t.set_theme(&theme(LIGHT_FG, LIGHT_BG), Overrides::Keep);
        t.set_theme(&theme(DARK_FG, DARK_BG), Overrides::Drop);
        assert_eq!(t.reply(), b"");
    });
}

/// **The class is compared across the whole call.** An application's light
/// `OSC 11` on a dark theme, then an embedder dropping it and setting the dark
/// theme again: the class the application sees goes light to dark, one
/// report. The same drop with the application's colours matching the theme
/// leaves the class where it was: nothing.
#[test]
fn a_drop_and_set_in_one_call_reports_only_a_change_of_class() {
    support::with_corpus(|t| {
        t.set_theme(&theme(DARK_FG, DARK_BG), Overrides::Keep);
        support::feed(
            t,
            b"\x1b[?2031h\x1b]11;rgb:ff/ff/ff\x07\x1b]10;rgb:00/00/00\x07",
        );
        assert_eq!(drain(t), b"", "an application's own escapes send nothing");
        t.set_theme(&theme(DARK_FG, DARK_BG), Overrides::Drop);
        assert_eq!(drain(t), DARK);
        support::feed(t, b"\x1b]11;rgb:00/00/00\x07");
        assert_eq!(drain(t), b"");
        t.set_theme(&theme(DARK_FG, DARK_BG), Overrides::Drop);
        assert_eq!(drain(t), b"", "dark before, dark after");
    });
}

/// **A class the drop and the set cross between them is not reported.** The
/// text black with an application's background override black is a tie,
/// which reads light. Setting the light theme alone would put its dark text
/// on that black -- dark -- and the drop then restores the theme's page --
/// light again. Compared across the whole call, nothing changed and nothing
/// is sent; compared per step, two reports would tell an application about
/// a class it never saw.
#[test]
fn a_class_crossed_inside_one_call_is_not_reported() {
    support::with_corpus(|t| {
        t.set_default_colors((0, 0, 0), (0xff, 0xff, 0xff));
        support::feed(t, b"\x1b[?2031h\x1b]11;rgb:00/00/00\x07\x1b[?996n");
        assert_eq!(drain(t), LIGHT, "the control: a tie reads light");
        t.set_theme(&theme(LIGHT_FG, LIGHT_BG), Overrides::Drop);
        assert_eq!(drain(t), b"");
    });
}

/// An application's `OSC 11` changes the next answer and sends nothing.
#[test]
fn an_applications_escape_changes_the_answer_and_reports_nothing() {
    each_read_size(
        b"\x1b[?2031h\x1b]11;rgb:ff/ff/ff\x07\x1b]10;rgb:00/00/00\x07",
        |got| assert_eq!(got, b""),
    );
    each_read_size(
        b"\x1b[?2031h\x1b]11;rgb:ff/ff/ff\x07\x1b]10;rgb:00/00/00\x07\x1b[?996n",
        |got| assert_eq!(got, LIGHT),
    );
}

#[test]
fn drop_clears_every_override_and_keep_keeps_them() {
    let set = b"\x1b]4;1;rgb:12/34/56\x07\x1b]10;rgb:01/01/01\x07\x1b]11;rgb:02/02/02\x07\x1b]12;rgb:03/03/03\x07";
    for (o, gone) in [(Overrides::Keep, false), (Overrides::Drop, true)] {
        support::with_corpus(|t| {
            support::feed(t, set);
            t.set_theme(&theme(DARK_FG, DARK_BG), o);
            assert_eq!(t.palette(Chan::from_u8(1)).is_none(), gone, "{o:?}");
            for w in [
                DynamicColor::Foreground,
                DynamicColor::Background,
                DynamicColor::Cursor,
            ] {
                assert_eq!(t.dynamic_color(w).is_none(), gone, "{o:?} {w:?}");
            }
        });
    }
}

/// neovim's handshake (`[[source: neovim-tui-c]]` 225-266): it asks DECRQM
/// `?2031` and subscribes only on answer 2; an embedder flip then sends the
/// notification, and its `OSC 11` re-query gets the new colour.
#[test]
fn neovims_handshake_subscribes_and_re_queries() {
    support::with_corpus(|t| {
        t.set_theme(&theme(DARK_FG, DARK_BG), Overrides::Keep);
        support::feed(t, b"\x1b[?2031$p");
        assert_eq!(drain(t), b"\x1b[?2031;2$y", "recognised and reset");
        support::feed(t, b"\x1b[?2031h");
        assert_eq!(drain(t), b"");
        t.set_theme(&theme(LIGHT_FG, LIGHT_BG), Overrides::Drop);
        assert_eq!(drain(t), LIGHT);
        support::feed(t, b"\x1b]11;?\x07");
        assert_eq!(drain(t), b"\x1b]11;rgb:efef/efef/efef\x07");
    });
}

/// Every other private `n` stays unsupported and counted: DECXCPR is not
/// answered as a CPR, and `?996` with the wrong number is not the query.
#[test]
fn other_private_dsrs_stay_unsupported() {
    support::with_corpus(|t| {
        t.set_theme(&theme(DARK_FG, DARK_BG), Overrides::Keep);
        let before = t.unsupported;
        support::feed(t, b"\x1b[?99n\x1b[?995n\x1b[?997n");
        assert_eq!(t.reply(), b"");
        assert_eq!(t.unsupported, before + 3);
    });
}

/// tmux starting inside baud (`tests/scheme/tmux-start.raw`, captured): it
/// subscribes with `?2031h` and asks `?996n`, then DA1. The answer is the
/// dark class, nothing is sent for the subscription, and the 997 leaves
/// before the DA1 reply, at every read size.
#[test]
fn tmux_starting_is_answered_in_order() {
    let rec = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/scheme/tmux-start.raw"
    ))
    .expect("the capture");
    // The control: the capture carries what the test is about.
    let at = |n: &[u8]| rec.windows(n.len()).position(|w| w == n);
    assert!(at(b"\x1b[?2031h") < at(b"\x1b[?996n") && at(b"\x1b[?996n") < at(b"\x1b[c"));
    each_read_size(&rec, |got| {
        let find = |n: &[u8]| got.windows(n.len()).position(|w| w == n);
        let reports = got.windows(DARK.len()).filter(|w| *w == DARK).count();
        assert_eq!(reports, 1, "one report, none for 2031 h: {got:?}");
        // baud's DA1 answer opens `CSI ? 6` -- a VT102, with `;4` when it
        // claims Sixel -- which no 997 does.
        let (scheme, da1) = (find(DARK).expect("a 997"), find(b"\x1b[?6").expect("a DA1"));
        assert!(scheme < da1, "the 997 precedes DA1: {got:?}");
    });
}
