//! kitty's colour control, `OSC 21 ; key=value ; ... ST`
//! (`[[source: kitty-6d5d0c4-color-stack]]`): a program written for kitty
//! reads and sets the default colours, the selection's, the cursor's, the
//! text under the cursor, the visual bell's and the 256 palette entries,
//! and the answer is kitty's to the byte (`[[source: kitty-window-py]]`
//! 571-641, and the cases of kitty's own test_color_profile, `[[source:
//! kitty-6d5d0c4-tests-screen]]` 1947-2000).

mod support;

use baud_core::*;
use baud_domain::themefile::CellPaint;

const FG: (u8, u8, u8) = (0xe0, 0xe0, 0xe0);
const BG: (u8, u8, u8) = (0x11, 0x11, 0x11);
const SELECTION: (u8, u8, u8) = (0x33, 0x66, 0x99);

/// xterm's 256, with the sixteen made distinct.
fn table() -> [(u8, u8, u8); 256] {
    const STEPS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    let mut t = [(0, 0, 0); 256];
    for (i, e) in t.iter_mut().enumerate().take(16) {
        let v = u8::try_from(i * 16).unwrap();
        *e = (v, v, 0x40);
    }
    for i in 0..216 {
        t[16 + i] = (STEPS[i / 36], STEPS[(i / 6) % 6], STEPS[i % 6]);
    }
    for k in 0..24 {
        let v = u8::try_from(8 + k * 10).unwrap();
        t[232 + k] = (v, v, v);
    }
    t
}

fn theme(palette: &[(u8, u8, u8); 256]) -> ThemeColors<'_> {
    ThemeColors {
        fg: FG,
        bg: BG,
        palette,
        selection: Some(CellPaint::Colour(SELECTION)),
        selection_text: None,
        cursor: None,
        cursor_text: None,
        cursor_text_under: baud_domain::themefile::CursorText::UnderEveryCursor,
        unset_selection_text: baud_domain::themefile::SelectionText::Reverse,
    }
}

/// A terminal told its colours, `bytes` fed at read sizes 1, 3, 7 and
/// whole, and `f` run on each.
fn each_read_size(bytes: &[u8], f: impl Fn(&mut Term<'_>)) {
    let table = table();
    for size in [1, 3, 7, bytes.len().max(1)] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.set_theme(&theme(&table), Overrides::Keep);
        for chunk in bytes.chunks(size) {
            support::feed(&mut t, chunk);
        }
        f(&mut t);
    }
}

fn told(f: impl FnOnce(&mut Term<'_>)) {
    let table = table();
    support::with_corpus(|t| {
        t.set_theme(&theme(&table), Overrides::Keep);
        f(t);
    });
}

fn drain(t: &mut Term<'_>) -> Vec<u8> {
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// kitty's own first case: the defaults, an entry, and a key it does not
/// know, which answers `?`; one reply, in the order asked, each colour
/// `rgb:rr/gg/bb`.
#[test]
fn a_query_answers_in_kittys_shape() {
    each_read_size(
        b"\x1b]21;background=?;foreground=?;213=?;unknown=?\x07",
        |t| {
            assert_eq!(
                text(t.reply()),
                "\x1b]21;background=rgb:11/11/11;foreground=rgb:e0/e0/e0;\
                 213=rgb:ff/87/ff;unknown=?\x1b\\"
            );
        },
    );
}

/// **The reply ends in `ESC \` whatever ended the request**, as kitty
/// writes every OSC (`[[source: kitty-6d5d0c4-screen]]` 1728-1741), and
/// it is 7-bit.
#[test]
fn the_reply_ends_in_st_whatever_ended_the_request() {
    for end in [&b"\x07"[..], b"\x1b\\"] {
        told(|t| {
            let mut seq = b"\x1b]21;foreground=?".to_vec();
            seq.extend_from_slice(end);
            support::feed(t, &seq);
            assert_eq!(text(t.reply()), "\x1b]21;foreground=rgb:e0/e0/e0\x1b\\");
        });
    }
}

/// A set before a query in one sequence is what the query finds, and a key
/// asked twice is answered once, where it was first asked, with what the
/// last ask found -- kitty's responses dict.
#[test]
fn a_key_asked_twice_is_answered_once_with_the_last_value() {
    each_read_size(
        b"\x1b]21;foreground=?;background=?;foreground=white;foreground=?\x1b\\",
        |t| {
            assert_eq!(
                text(t.reply()),
                "\x1b]21;foreground=rgb:ff/ff/ff;background=rgb:11/11/11\x1b\\"
            );
            assert_eq!(
                t.dynamic_color(DynamicColor::Foreground),
                Some((255, 255, 255))
            );
        },
    );
}

/// A key is echoed as it was sent: `01` and `1` are two keys naming one
/// entry, as they are two keys of kitty's dict.
#[test]
fn a_key_is_echoed_as_sent() {
    told(|t| {
        support::feed(t, b"\x1b]21;1=?;01=?\x07");
        assert_eq!(
            text(t.reply()),
            "\x1b]21;1=rgb:10/10/40;01=rgb:10/10/40\x1b\\"
        );
    });
}

/// No query, no reply; and `21` alone asks nothing.
#[test]
fn a_sequence_that_asks_nothing_is_not_answered() {
    told(|t| {
        support::feed(t, b"\x1b]21;foreground=red\x07\x1b]21\x07\x1b]21;\x07");
        assert_eq!(t.reply(), b"");
        assert_eq!(t.dynamic_color(DynamicColor::Foreground), Some((255, 0, 0)));
    });
}

/// A palette entry takes a colour and a reset, the bare key; an entry of
/// the sixteen and one past them alike.
#[test]
fn a_palette_entry_is_set_and_reset() {
    told(|t| {
        support::feed(t, b"\x1b]21;1=#123456;200=red;5=#008000\x07");
        assert_eq!(t.palette(Chan::from_u8(1)), Some((0x12, 0x34, 0x56)));
        assert_eq!(t.palette(Chan::from_u8(200)), Some((0xff, 0, 0)));
        support::feed(t, b"\x1b]21;1;200\x07");
        assert_eq!(t.palette(Chan::from_u8(1)), None);
        assert_eq!(t.palette(Chan::from_u8(200)), None);
        assert_eq!(t.palette(Chan::from_u8(5)), Some((0, 0x80, 0)));
        support::feed(t, b"\x1b]21;1=?;200=?\x07");
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;1=rgb:10/10/40;200=rgb:ff/00/d7\x1b\\"
        );
    });
}

/// **An empty value on a palette entry is refused and counted**: "Setting
/// the ANSI color table colors to dynamic is not allowed" (`[[source:
/// kitty-6d5d0c4-color-stack]]` 82). The entry keeps the colour it had,
/// within the sequence too, so a query after it in the same sequence
/// answers that colour; the sequence's other records still apply, and the
/// set sends nothing. An entry of the sixteen and one past them alike.
#[test]
fn a_palette_entry_takes_no_dynamic_value() {
    told(|t| {
        support::feed(t, b"\x1b]21;5=red;200=#0a0b0c\x07");
        let before = t.unsupported;
        support::feed(t, b"\x1b]21;5=;200=;7=#0000ff\x07");
        assert_eq!(t.unsupported, before + 2);
        assert!(drain(t).is_empty());
        assert_eq!(t.palette(Chan::from_u8(5)), Some((0xff, 0, 0)));
        assert_eq!(t.palette(Chan::from_u8(200)), Some((0x0a, 0x0b, 0x0c)));
        assert_eq!(t.palette(Chan::from_u8(7)), Some((0, 0, 0xff)));
        support::feed(t, b"\x1b]21;5=#008000;5=;5=?;200=;200=?\x07");
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;5=rgb:00/80/00;200=rgb:0a/0b/0c\x1b\\"
        );
        assert_eq!(t.palette(Chan::from_u8(5)), Some((0, 0x80, 0)));
    });
    // Never set: the entry stays the configured one, and answers it.
    told(|t| {
        support::feed(t, b"\x1b]21;3=;3=?;30=;30=?\x07");
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;3=rgb:30/30/40;30=rgb:00/87/87\x1b\\"
        );
        assert_eq!(t.palette(Chan::from_u8(3)), None);
        assert_eq!(t.palette(Chan::from_u8(30)), None);
    });
}

/// The default colours take a colour and a reset; an empty value is "Not
/// applicable" to them in kitty's table, refused and counted.
#[test]
fn the_default_colours_take_no_automatic_value() {
    told(|t| {
        support::feed(t, b"\x1b]21;background=#000080\x07");
        assert_eq!(
            t.dynamic_color(DynamicColor::Background),
            Some((0, 0, 0x80))
        );
        let before = t.unsupported;
        support::feed(t, b"\x1b]21;background=\x07");
        assert_eq!(t.unsupported, before + 1);
        assert_eq!(
            t.dynamic_color(DynamicColor::Background),
            Some((0, 0, 0x80))
        );
        support::feed(t, b"\x1b]21;background\x07");
        assert_eq!(t.dynamic_color(DynamicColor::Background), None);
    });
}

/// The selection's colour: a program's, else the configured one, and an
/// empty value is "no defined value", answered empty, as kitty's own test
/// has it. The `OSC 17` query of the same colour answers reverse video's
/// text, as it does with none configured.
#[test]
fn a_selection_made_automatic_answers_empty() {
    told(|t| {
        support::feed(t, b"\x1b]21;selection_background=?\x07");
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;selection_background=rgb:33/66/99\x1b\\"
        );
        support::feed(t, b"\x1b]21;selection_background=\x07");
        support::feed(t, b"\x1b]21;selection_background=?\x07\x1b]17;?\x07");
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;selection_background=\x1b\\\x1b]17;rgb:e0e0/e0e0/e0e0\x07"
        );
        support::feed(
            t,
            b"\x1b]21;selection_background\x07\x1b]21;selection_background=?\x07",
        );
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;selection_background=rgb:33/66/99\x1b\\"
        );
    });
}

/// **A selection that takes each cell's own colours has no one colour to
/// answer**, so `OSC 21` answers it empty, "if the queried color is one
/// that does not have a defined value ... the value must be empty"
/// (`[[source: kitty-6d5d0c4-color-stack]]` 88-93), as kitty answers its
/// own selection_foreground of none, while `OSC 17`/`19`, which have no
/// empty answer, answer the default page and text in the cell's place. Each
/// colour is marked apart, so a selection page of one colour still answers
/// it; a program's colour wins over the mark, and a reset brings it back.
#[test]
fn a_selection_in_each_cells_colours_answers_empty() {
    let table = table();
    support::with_corpus(|t| {
        t.set_theme(
            &ThemeColors {
                selection: Some(CellPaint::Page),
                selection_text: Some(CellPaint::Text),
                ..theme(&table)
            },
            Overrides::Keep,
        );
        support::feed(
            t,
            b"\x1b]21;selection_background=?;selection_foreground=?\x07\x1b]17;?\x07\x1b]19;?\x07",
        );
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;selection_background=;selection_foreground=\x1b\\\
             \x1b]17;rgb:1111/1111/1111\x07\x1b]19;rgb:e0e0/e0e0/e0e0\x07"
        );
        support::feed(
            t,
            b"\x1b]21;selection_foreground=#102030;selection_foreground=?\x07",
        );
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;selection_foreground=rgb:10/20/30\x1b\\"
        );
        support::feed(
            t,
            b"\x1b]21;selection_foreground\x07\x1b]21;selection_foreground=?\x07",
        );
        assert_eq!(text(&drain(t)), "\x1b]21;selection_foreground=\x1b\\");
        t.set_theme(
            &ThemeColors {
                selection_text: Some(CellPaint::Page),
                ..theme(&table)
            },
            Overrides::Drop,
        );
        support::feed(
            t,
            b"\x1b]21;selection_background=?;selection_foreground=?\x07\x1b]19;?\x07",
        );
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;selection_background=rgb:33/66/99;selection_foreground=\x1b\\\
             \x1b]19;rgb:1111/1111/1111\x07"
        );
    });
}

/// The cursor with no colour of its own answers empty, as kitty's text's
/// example does ("cursor="); a colour set is `OSC 12`'s colour.
#[test]
fn the_cursor_is_the_osc_12_colour() {
    told(|t| {
        support::feed(t, b"\x1b]21;cursor=?\x07");
        assert_eq!(text(&drain(t)), "\x1b]21;cursor=\x1b\\");
        support::feed(
            t,
            b"\x1b]21;cursor=#ff8000\x07\x1b]12;?\x07\x1b]21;cursor=?\x07",
        );
        assert_eq!(
            text(&drain(t)),
            "\x1b]12;rgb:ffff/8080/0000\x07\x1b]21;cursor=rgb:ff/80/00\x1b\\"
        );
    });
}

/// **The visual bell takes a colour**, "The color of a visual bell"
/// (`[[source: kitty-6d5d0c4-color-stack]]`): set, asked and reset as the
/// other named keys are. With none set it answers empty, since baud
/// configures no bell colour and its automatic flash, the other video, has
/// no one colour; an empty value is that automatic flash, and a bare key
/// the colour it would have if never set, which here is the same flash.
#[test]
fn the_visual_bell_is_set_asked_and_reset() {
    told(|t| {
        let before = t.unsupported;
        support::feed(t, b"\x1b]21;visual_bell=?\x07");
        assert_eq!(text(&drain(t)), "\x1b]21;visual_bell=\x1b\\");
        assert_eq!(t.program_paint(PaintRole::VisualBell), ProgramPaint::Unset);
        support::feed(t, b"\x1b]21;visual_bell=red;visual_bell=?\x07");
        assert_eq!(text(&drain(t)), "\x1b]21;visual_bell=rgb:ff/00/00\x1b\\");
        assert_eq!(
            t.program_paint(PaintRole::VisualBell),
            ProgramPaint::Colour((0xff, 0, 0))
        );
        assert_eq!(t.unsupported, before, "a colour is honoured");
        support::feed(t, b"\x1b]21;visual_bell=;visual_bell=?\x07");
        assert_eq!(text(&drain(t)), "\x1b]21;visual_bell=\x1b\\");
        assert_eq!(
            t.program_paint(PaintRole::VisualBell),
            ProgramPaint::Automatic
        );
        support::feed(t, b"\x1b]21;visual_bell=#102030\x07\x1b]21;visual_bell\x07");
        assert_eq!(t.program_paint(PaintRole::VisualBell), ProgramPaint::Unset);
        assert_eq!(t.unsupported, before, "nothing refused");
        // Kept apart from the text under the cursor, which a shared slot
        // would overwrite.
        support::feed(t, b"\x1b]21;cursor_text=#010203;visual_bell=#a0b0c0\x07");
        assert_eq!(
            t.program_paint(PaintRole::CursorText),
            ProgramPaint::Colour((1, 2, 3))
        );
        assert_eq!(
            t.program_paint(PaintRole::VisualBell),
            ProgramPaint::Colour((0xa0, 0xb0, 0xc0))
        );
        support::feed(t, b"\x1b]21;visual_bell=?;cursor_text=?\x07");
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;visual_bell=rgb:a0/b0/c0;cursor_text=rgb:01/02/03\x1b\\"
        );
    });
}

/// **kitty's per-colour transparency is a key baud does not know**: its
/// query answers `?`, as kitty answers transparent_background_color9,
/// and a set is counted.
#[test]
fn a_transparent_background_colour_is_an_unknown_key() {
    told(|t| {
        let before = t.unsupported;
        support::feed(
            t,
            b"\x1b]21;transparent_background_color2=?;transparent_background_color2=red@0.5\x07",
        );
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;transparent_background_color2=?\x1b\\"
        );
        assert_eq!(t.unsupported, before + 1);
    });
}

/// **Nothing changes until the terminator**, and a cancelled sequence
/// changes nothing: `CAN` and an `ESC` that is not `ST` end it unexecuted.
#[test]
fn a_cancelled_sequence_changes_nothing() {
    told(|t| {
        support::feed(t, b"\x1b]21;1=#ffffff;2=#ffffff");
        assert_eq!(
            t.palette(Chan::from_u8(1)),
            None,
            "not before the terminator"
        );
        support::feed(t, b"\x18\x1b]21;3=#ffffff\x1b[m");
        for i in 1..=3 {
            assert_eq!(t.palette(Chan::from_u8(i)), None, "entry {i}");
        }
        support::feed(t, b"\x1b]21;4=#ffffff\x1b\\");
        assert_eq!(t.palette(Chan::from_u8(4)), Some((255, 255, 255)));
    });
}

/// **A sequence of any length is honoured**: all 256 entries set in one,
/// about 3.5 KB, and asked in one; kitty takes up to 256 KiB.
#[test]
fn every_entry_is_set_and_asked_in_one_sequence() {
    let mut set = b"\x1b]21".to_vec();
    let mut ask = b"\x1b]21".to_vec();
    let mut want = String::from("\x1b]21;");
    for i in 0..=255u8 {
        set.extend_from_slice(format!(";{i}=#{i:02x}{:02x}01", 255 - i).as_bytes());
        ask.extend_from_slice(format!(";{i}=?").as_bytes());
        if i > 0 {
            want.push(';');
        }
        want.push_str(&format!("{i}=rgb:{i:02x}/{:02x}/01", 255 - i));
    }
    set.extend_from_slice(b"\x07");
    ask.extend_from_slice(b"\x07");
    want.push_str("\x1b\\");
    assert!(set.len() > 3 * OSC_MAX);
    each_read_size(&[set.as_slice(), ask.as_slice()].concat(), |t| {
        for i in 0..=255u8 {
            assert_eq!(t.palette(Chan::from_u8(i)), Some((i, 255 - i, 1)));
        }
        assert_eq!(text(t.reply()), want);
    });
}

/// A record longer than `OSC_MAX` is not the record sent: refused and
/// counted, and the records around it honoured. Cut where it was, this one
/// would read `red`; whole it is no colour.
#[test]
fn a_record_past_osc_max_is_refused_and_counted() {
    told(|t| {
        let before = t.unsupported;
        let mut seq = b"\x1b]21;1=red;2=red".to_vec();
        seq.extend(std::iter::repeat_n(b' ', OSC_MAX));
        seq.extend_from_slice(b"x;3=green\x07");
        support::feed(t, &seq);
        assert_eq!(t.palette(Chan::from_u8(1)), Some((255, 0, 0)));
        assert_eq!(t.palette(Chan::from_u8(2)), None);
        assert_eq!(t.palette(Chan::from_u8(3)), Some((0, 255, 0)));
        assert_eq!(t.unsupported, before + 1);
    });
}

/// **Only printable ASCII is kept**, as kitty drops every other byte before
/// it splits the body: a C0 control or a UTF-8 character inside a key
/// vanishes from it.
#[test]
fn a_byte_past_printable_ascii_is_dropped() {
    told(|t| {
        support::feed(t, "\x1b]21;fore\u{e9}ground=?;cur\tsor=?\x07".as_bytes());
        assert_eq!(
            text(t.reply()),
            "\x1b]21;foreground=rgb:e0/e0/e0;cursor=\x1b\\"
        );
    });
}

/// **A value is read by kitty's text**: `rgb:` scaled, `#` as the high
/// bits, `rgbi:` clipped to 0 to 1 -- every `rgbi:` of which kitty's own
/// parser refuses -- kitty's names in any case, and an `@` alpha checked
/// and dropped. What the text does not define is refused and counted:
/// `oklch(`, which kitty's parser takes, and an alpha off the grammar.
#[test]
fn a_value_is_read_by_kittys_text() {
    told(|t| {
        for (spec, want) in [
            ("rgb:8/80/800", (0x88, 0x80, 0x80)),
            ("#3a7", (0x30, 0xa0, 0x70)),
            ("rgbi:1/0.5/2", (0xff, 0xbc, 0xff)),
            ("rgbi:-1/0/1e0", (0, 0, 0xff)),
            ("AliceBlue", (0xf0, 0xf8, 0xff)),
            ("red@0.5", (0xff, 0, 0)),
            (" #ff0000 ", (0xff, 0, 0)),
        ] {
            support::feed(t, format!("\x1b]21;7={spec}\x07").as_bytes());
            assert_eq!(t.palette(Chan::from_u8(7)), Some(want), "{spec}");
        }
        for spec in [
            "oklch(0.5 0.1 30)",
            "red@half",
            "rgb:1/2/3/4",
            "#12345",
            "nosuchname",
        ] {
            support::feed(t, b"\x1b]21;7\x07");
            let before = t.unsupported;
            support::feed(t, format!("\x1b]21;7={spec}\x07").as_bytes());
            assert_eq!(t.palette(Chan::from_u8(7)), None, "{spec}");
            assert_eq!(t.unsupported, before + 1, "{spec}");
        }
    });
}

/// **Under `?5` the default colours are the colours shown**, as
/// `OSC 10`/`11` are, so the two never disagree: a set acts
/// on the colour shown in its role, and a reset gives that role back.
#[test]
fn under_reverse_video_the_colours_shown_answer() {
    told(|t| {
        support::feed(
            t,
            b"\x1b[?5h\x1b]21;foreground=?;background=?\x07\x1b]10;?\x07",
        );
        assert_eq!(
            text(&drain(t)),
            "\x1b]21;foreground=rgb:11/11/11;background=rgb:e0/e0/e0\x1b\\\
             \x1b]10;rgb:1111/1111/1111\x07"
        );
        support::feed(t, b"\x1b]21;foreground=red;foreground=?\x07");
        assert_eq!(text(&drain(t)), "\x1b]21;foreground=rgb:ff/00/00\x1b\\");
        assert_eq!(t.dynamic_color(DynamicColor::Background), Some((255, 0, 0)));
        support::feed(t, b"\x1b]21;foreground\x07\x1b[?5l");
        assert_eq!(t.dynamic_color(DynamicColor::Background), None);
    });
}

/// **DECSWT stays out**: a title under `OSC 21` is read as a colour control
/// protocol record, a key baud does not know, counted, and sets no title --
/// DECSWT is a VT520's, and baud identifies as a VT100.
#[test]
fn a_title_under_osc_21_sets_no_title_and_is_counted() {
    told(|t| {
        let before = t.unsupported;
        support::feed(t, b"\x1b]21;My title\x1b\\");
        assert_eq!(t.title(), b"");
        assert_eq!(t.reply(), b"");
        assert_eq!(t.unsupported, before + 1);
    });
}

/// A terminal no embedder told its colours has no value to give, and says
/// so in kitty's shape: empty.
#[test]
fn a_terminal_told_nothing_answers_empty() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b]21;foreground=?;12=?\x07");
        assert_eq!(text(t.reply()), "\x1b]21;foreground=;12=\x1b\\");
    });
}

// What a program asks of the colours the renderer draws, read through
// `Term::program_paint`, and the text under the cursor.

/// **A program's automatic colour wins over the configured one**: kitty's
/// `selection_background=` is reverse video even under a theme that
/// configures a selection, `cursor=` the cell's colours, and a bare key
/// gives the theme back.
#[test]
fn an_automatic_colour_wins_over_the_configured_one() {
    told(|t| {
        assert_eq!(
            t.program_paint(PaintRole::SelectionBackground),
            ProgramPaint::Unset
        );
        support::feed(
            t,
            b"\x1b]21;selection_background=;cursor=;selection_foreground=#010203\x07",
        );
        assert_eq!(
            t.program_paint(PaintRole::SelectionBackground),
            ProgramPaint::Automatic
        );
        assert_eq!(t.program_paint(PaintRole::Cursor), ProgramPaint::Automatic);
        assert_eq!(
            t.program_paint(PaintRole::SelectionForeground),
            ProgramPaint::Colour((1, 2, 3))
        );
        support::feed(t, b"\x1b]21;selection_background;cursor\x07");
        assert_eq!(
            t.program_paint(PaintRole::SelectionBackground),
            ProgramPaint::Unset
        );
        assert_eq!(t.program_paint(PaintRole::Cursor), ProgramPaint::Unset);
    });
}

/// **The text under the cursor is set, asked and reset**: a program's,
/// else the configured colour, else empty.
#[test]
fn the_text_under_the_cursor_is_set_asked_and_reset() {
    let table = table();
    support::with_corpus(|t| {
        t.set_theme(
            &ThemeColors {
                cursor_text: Some((9, 8, 7)),
                ..theme(&table)
            },
            Overrides::Keep,
        );
        support::feed(t, b"\x1b]21;cursor_text=?\x07");
        assert_eq!(text(&drain(t)), "\x1b]21;cursor_text=rgb:09/08/07\x1b\\");
        assert_eq!(t.program_paint(PaintRole::CursorText), ProgramPaint::Unset);
        support::feed(t, b"\x1b]21;cursor_text=#102030;cursor_text=?\x07");
        assert_eq!(text(&drain(t)), "\x1b]21;cursor_text=rgb:10/20/30\x1b\\");
        assert_eq!(
            t.program_paint(PaintRole::CursorText),
            ProgramPaint::Colour((0x10, 0x20, 0x30))
        );
        support::feed(t, b"\x1b]21;cursor_text=;cursor_text=?\x07");
        assert_eq!(text(&drain(t)), "\x1b]21;cursor_text=\x1b\\");
        assert_eq!(
            t.program_paint(PaintRole::CursorText),
            ProgramPaint::Automatic
        );
        support::feed(t, b"\x1b]21;cursor_text\x07");
        assert_eq!(t.program_paint(PaintRole::CursorText), ProgramPaint::Unset);
    });
}

/// XTPUSHCOLORS keeps the text under the cursor with the dynamic colours,
/// and XTPOPCOLORS puts it back.
#[test]
fn the_colour_stack_keeps_the_text_under_the_cursor() {
    told(|t| {
        support::feed(t, b"\x1b]21;cursor_text=#010101;cursor=\x07\x1b[#P");
        support::feed(t, b"\x1b]21;cursor_text=#020202;cursor=#030303\x07");
        support::feed(t, b"\x1b[#Q");
        assert_eq!(
            t.program_paint(PaintRole::CursorText),
            ProgramPaint::Colour((1, 1, 1))
        );
        assert_eq!(t.program_paint(PaintRole::Cursor), ProgramPaint::Automatic);
    });
}

/// XTPUSHCOLORS keeps the visual bell's colour, as kitty's push keeps every
/// colour a program set (`[[source: kitty-6d5d0c4-colors-c]]` 717-757), and
/// XTPOPCOLORS puts it back.
#[test]
fn the_colour_stack_keeps_the_visual_bell() {
    told(|t| {
        support::feed(
            t,
            b"\x1b]21;visual_bell=#010101;cursor_text=#040404\x07\x1b[#P",
        );
        support::feed(t, b"\x1b]21;visual_bell=#020202;cursor_text=#050505\x07");
        support::feed(t, b"\x1b[#Q");
        assert_eq!(
            t.program_paint(PaintRole::VisualBell),
            ProgramPaint::Colour((1, 1, 1))
        );
        assert_eq!(
            t.program_paint(PaintRole::CursorText),
            ProgramPaint::Colour((4, 4, 4))
        );
        support::feed(
            t,
            b"\x1b]21;visual_bell=\x07\x1b[#P\x1b]21;visual_bell=#020202\x07\x1b[#Q",
        );
        assert_eq!(
            t.program_paint(PaintRole::VisualBell),
            ProgramPaint::Automatic
        );
    });
}

/// An embedder's light/dark flip that drops the overrides drops the visual
/// bell's colour with them.
#[test]
fn dropping_the_overrides_drops_the_visual_bell() {
    let table = table();
    told(|t| {
        support::feed(t, b"\x1b]21;visual_bell=#010101\x07");
        t.set_theme(&theme(&table), Overrides::Keep);
        assert_eq!(
            t.program_paint(PaintRole::VisualBell),
            ProgramPaint::Colour((1, 1, 1)),
            "kept"
        );
        t.set_theme(&theme(&table), Overrides::Drop);
        assert_eq!(t.program_paint(PaintRole::VisualBell), ProgramPaint::Unset);
    });
}

/// An embedder's light/dark flip that drops the overrides drops the text
/// under the cursor with them.
#[test]
fn dropping_the_overrides_drops_the_text_under_the_cursor() {
    let table = table();
    told(|t| {
        support::feed(t, b"\x1b]21;cursor_text=#010101\x07");
        t.set_theme(&theme(&table), Overrides::Drop);
        assert_eq!(t.program_paint(PaintRole::CursorText), ProgramPaint::Unset);
    });
}
