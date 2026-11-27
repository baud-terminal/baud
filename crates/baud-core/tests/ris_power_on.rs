//! **A full reset returns what a program set to the state at power-on, and
//! a soft reset keeps it.** `RIS` "causes a device to be reset to its
//! initial state, i.e. the state it has after it is made operational"
//! (`[[source: ecma48]]` 8.3.105), and `DECSTR`'s table
//! (`[[source: vt510-decstr]]`) names none of the state here, so it keeps
//! all of it. Each test sets one piece, sends `RIS`, and finds it as a
//! fresh terminal has it; then sets it again, sends `DECSTR`, and finds it
//! kept.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use baud_domain::themefile::{CellPaint, CursorText, SelectionText};

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 10,
};

fn iterm(body: &str) -> Vec<u8> {
    format!("\x1b]1337;{body}\x07").into_bytes()
}

fn kitty_query(t: &mut Term<'_>) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, b"\x1b[?u");
    t.reply().to_vec()
}

/// **`RIS` empties both kitty keyboard stacks**, the shown screen's and
/// the one parked for the alternate screen, so a program that crashed with
/// the protocol on leaves the shell's keys as they were at power-on once
/// the user types `reset` -- the case `[[source: kitty-keyboard]]` keeps
/// Enter, Tab and Backspace legacy for. A pop after it finds nothing saved
/// from before. `DECSTR` keeps the flags.
#[test]
fn ris_empties_both_kitty_keyboard_stacks_and_decstr_keeps_them() {
    support::with_term(&WS, |t| {
        // The primary's flags, with an older entry saved beneath them, and
        // the alternate screen's, parked when it leaves.
        support::feed(t, b"\x1b[=1u\x1b[>31u\x1b[?1049h\x1b[>3u\x1b[?1049l");
        assert_eq!(t.kitty_flags(), KittyFlags::ALL, "the primary's");
        support::feed(t, b"\x1bc");
        assert!(t.kitty_flags().is_none(), "RIS");
        assert_eq!(kitty_query(t), b"\x1b[?0u", "and the query says so");
        support::feed(t, b"\x1b[>4u\x1b[<1u");
        assert!(t.kitty_flags().is_none(), "nothing saved from before RIS");
        support::feed(t, b"\x1b[?1049h");
        assert!(t.kitty_flags().is_none(), "the parked stack too");
        support::feed(t, b"\x1b[?1049l\x1b[=5u\x1b[!p");
        assert_eq!(t.kitty_flags().bits(), 0b101, "DECSTR keeps them");
    });
}

/// **`RIS` empties the window title and the icon name**, for the embedder
/// to show its own until a program names one again; the encoding they
/// arrived under goes back to UTF-8 with them. `DECSTR` keeps both.
#[test]
fn ris_empties_the_title_and_icon_name_and_decstr_keeps_them() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b%@\x1b]2;vim\x07\x1b]1;v\x07\x1b%G");
        assert_eq!(t.title(), b"vim");
        assert_eq!(t.title_encoding(), InputEncoding::Latin1);
        support::feed(t, b"\x1bc");
        assert_eq!(t.title(), b"", "RIS");
        assert_eq!(t.icon_name(), b"", "RIS");
        assert_eq!(t.title_encoding(), InputEncoding::Utf8, "RIS");
        assert_eq!(t.icon_name_encoding(), InputEncoding::Utf8, "RIS");
        support::feed(t, b"\x1b]0;less\x07\x1b[!p");
        assert_eq!(t.title(), b"less", "DECSTR");
        assert_eq!(t.icon_name(), b"less", "DECSTR");
    });
}

/// **`RIS` clears iTerm2's user variables**: a terminal made operational
/// holds none. `DECSTR` keeps them.
#[test]
fn ris_clears_the_user_variables_and_decstr_keeps_them() {
    support::with_term(&WS, |t| {
        // "main" and "dev", base64.
        support::feed(t, &iterm("SetUserVar=branch=bWFpbg=="));
        support::feed(t, &iterm("SetUserVar=host=ZGV2"));
        assert_eq!(t.user_var(b"branch"), Some(&b"main"[..]));
        support::feed(t, b"\x1bc");
        assert_eq!(t.user_var(b"branch"), None, "RIS");
        assert_eq!(t.user_var(b"host"), None, "RIS");
        support::feed(t, &iterm("SetUserVar=branch=bWFpbg=="));
        support::feed(t, b"\x1b[!p");
        assert_eq!(t.user_var(b"branch"), Some(&b"main"[..]), "DECSTR");
    });
}

/// **`RIS` clears the badge and its colour**: iTerm2's `SetBadgeFormat`
/// and `SetColors=badge=`, none of either at power-on. `DECSTR` keeps
/// both.
#[test]
fn ris_clears_the_badge_and_its_colour_and_decstr_keeps_them() {
    support::with_term(&WS, |t| {
        // "on main", base64.
        support::feed(t, &iterm("SetBadgeFormat=b24gbWFpbg=="));
        support::feed(t, &iterm("SetColors=badge=ff8000"));
        assert_eq!(t.badge_format(), Some("on main"));
        assert!(t.badge_colour().is_some());
        support::feed(t, b"\x1bc");
        assert_eq!(t.badge_format(), None, "RIS");
        assert_eq!(t.badge_colour(), None, "RIS");
        support::feed(t, &iterm("SetBadgeFormat=b24gbWFpbg=="));
        support::feed(t, &iterm("SetColors=badge=ff8000"));
        support::feed(t, b"\x1b[!p");
        assert_eq!(t.badge_format(), Some("on main"), "DECSTR");
        assert!(t.badge_colour().is_some(), "DECSTR");
    });
}

/// **`RIS` forgets the host `RemoteHost` named**, with the directory: a
/// `CurrentDir` after it is on this machine until another names a host.
/// `DECSTR` keeps the host.
#[test]
fn ris_forgets_the_remote_host_and_decstr_keeps_it() {
    support::with_term(&WS, |t| {
        support::feed(t, &iterm("RemoteHost=me@box"));
        support::feed(t, b"\x1bc");
        support::feed(t, &iterm("CurrentDir=/srv"));
        let d = t.working_directory().expect("kept");
        assert_eq!(d.host, FileHost::Local, "RIS");
        support::feed(t, &iterm("RemoteHost=me@box"));
        support::feed(t, b"\x1b[!p");
        support::feed(t, &iterm("CurrentDir=/srv"));
        let d = t.working_directory().expect("kept");
        assert_eq!(d.host, FileHost::Named(b"box"), "DECSTR");
    });
}

/// **After a program changed the font, `RIS` asks the embedder for the
/// configured one**: the configured family at the configured size, in
/// place of whatever was still queued, as Set-Up's saved settings are the
/// state a reset returns to. With no change since power-on or the last
/// `RIS` it asks nothing, and `DECSTR` asks nothing.
#[test]
fn ris_asks_for_the_configured_font_after_a_program_changed_it() {
    support::with_term(&WS, |t| {
        t.set_font(b"JetBrains Mono", FontSize::Configured);
        support::feed(t, b"\x1bc");
        assert!(t.font_requests().is_empty(), "no change: RIS asks nothing");
        support::feed(t, b"\x1b]50;Menlo\x07\x1b[!p");
        assert_eq!(t.font_requests().len(), 1, "DECSTR asks nothing");
        t.clear_font_requests();
        support::feed(t, b"\x1b]50;#+2\x07\x1bc");
        let asked = t.font_requests();
        assert_eq!(asked.len(), 1, "the change superseded: {asked:?}");
        assert_eq!(asked[0].size, SizeStep::Configured);
        assert_eq!(asked[0].family(), FontFamily::Configured);
        assert_eq!(asked[0].name(), None);
        t.clear_font_requests();
        support::feed(t, b"\x1bc");
        assert!(t.font_requests().is_empty(), "once");
    });
}

const TABLE: [(u8, u8, u8); 256] = [(0, 0, 0); 256];

/// The embedder's colours, each one distinct, so a reset that returns to
/// them answers something a program's override could not be mistaken
/// for.
fn configured(t: &mut Term<'_>) {
    t.set_theme(
        &ThemeColors {
            fg: (0xe0, 0xe0, 0xe0),
            bg: (0x11, 0x11, 0x11),
            palette: &TABLE,
            selection: Some(CellPaint::Colour((0x3f, 0x63, 0x8b))),
            selection_text: Some(CellPaint::Colour((0xff, 0xff, 0xff))),
            cursor: Some((0x52, 0x8b, 0xff)),
            cursor_text: Some((0x09, 0x08, 0x07)),
            cursor_text_under: CursorText::UnderEveryCursor,
            unset_selection_text: SelectionText::Reverse,
        },
        Overrides::Keep,
    );
}

fn ask(t: &mut Term<'_>, query: &[u8]) -> Vec<u8> {
    t.clear_reply();
    support::feed(t, query);
    t.reply().to_vec()
}

/// One dynamic colour a program set, `RIS`, and the embedder's answered
/// again with no override standing; then set again, `DECSTR`, and the
/// program's kept. `paint` reads the override itself where a query alone
/// could not tell it from a configured colour.
fn dynamic_colour(set: &[u8], query: &[u8], ours: &[u8], theirs: &[u8], which: DynamicColor) {
    support::with_term(&WS, |t| {
        configured(t);
        support::feed(t, set);
        assert_eq!(ask(t, query), theirs, "the program's");
        support::feed(t, b"\x1bc");
        assert_eq!(ask(t, query), ours, "RIS");
        assert_eq!(t.dynamic_color(which), None, "RIS: no override");
        support::feed(t, set);
        support::feed(t, b"\x1b[!p");
        assert_eq!(ask(t, query), theirs, "DECSTR");
        assert!(t.dynamic_color(which).is_some(), "DECSTR");
    });
}

/// **`RIS` returns the text colour to the embedder's**: a program's
/// `OSC 10` is not the state a terminal is made operational in. `DECSTR`
/// keeps it.
#[test]
fn ris_returns_the_text_colour_and_decstr_keeps_it() {
    dynamic_colour(
        b"\x1b]10;#ff0000\x07",
        b"\x1b]10;?\x07",
        b"\x1b]10;rgb:e0e0/e0e0/e0e0\x07",
        b"\x1b]10;rgb:ffff/0000/0000\x07",
        DynamicColor::Foreground,
    );
}

/// **`RIS` returns the page colour to the embedder's**, a program's
/// `OSC 11` gone. `DECSTR` keeps it.
#[test]
fn ris_returns_the_page_colour_and_decstr_keeps_it() {
    dynamic_colour(
        b"\x1b]11;#00ff00\x07",
        b"\x1b]11;?\x07",
        b"\x1b]11;rgb:1111/1111/1111\x07",
        b"\x1b]11;rgb:0000/ffff/0000\x07",
        DynamicColor::Background,
    );
}

/// **`RIS` returns the cursor colour to the embedder's**, a program's
/// `OSC 12` gone. `DECSTR` keeps it.
#[test]
fn ris_returns_the_cursor_colour_and_decstr_keeps_it() {
    dynamic_colour(
        b"\x1b]12;#0000ff\x07",
        b"\x1b]12;?\x07",
        b"\x1b]12;rgb:5252/8b8b/ffff\x07",
        b"\x1b]12;rgb:0000/0000/ffff\x07",
        DynamicColor::Cursor,
    );
}

/// **`RIS` returns the selection's page to the embedder's**, a program's
/// `OSC 17` gone. `DECSTR` keeps it.
#[test]
fn ris_returns_the_selection_page_and_decstr_keeps_it() {
    dynamic_colour(
        b"\x1b]17;#123456\x07",
        b"\x1b]17;?\x07",
        b"\x1b]17;rgb:3f3f/6363/8b8b\x07",
        b"\x1b]17;rgb:1212/3434/5656\x07",
        DynamicColor::HighlightBg,
    );
}

/// **`RIS` returns the selection's text to the embedder's**, a program's
/// `OSC 19` gone. `DECSTR` keeps it.
#[test]
fn ris_returns_the_selection_text_and_decstr_keeps_it() {
    dynamic_colour(
        b"\x1b]19;#405060\x07",
        b"\x1b]19;?\x07",
        b"\x1b]19;rgb:ffff/ffff/ffff\x07",
        b"\x1b]19;rgb:4040/5050/6060\x07",
        DynamicColor::HighlightFg,
    );
}

/// **`RIS` returns the visual bell to its automatic flash**, the colour a
/// program set with kitty's `OSC 21 ; visual_bell=` gone. `DECSTR` keeps
/// it, as it keeps the other colours `OSC 21` names; kitty's soft reset
/// clears every colour a program set (`do_screen_reset()`, `[[source:
/// kitty-6d5d0c4-screen]]` 209-266, the colours at 245 and 258), practice not
/// followed.
#[test]
fn ris_returns_the_visual_bell_and_decstr_keeps_it() {
    support::with_term(&WS, |t| {
        configured(t);
        let set = b"\x1b]21;visual_bell=#102030\x07";
        support::feed(t, set);
        let theirs = ProgramPaint::Colour((0x10, 0x20, 0x30));
        assert_eq!(t.program_paint(PaintRole::VisualBell), theirs);
        support::feed(t, b"\x1bc");
        assert_eq!(
            t.program_paint(PaintRole::VisualBell),
            ProgramPaint::Unset,
            "RIS"
        );
        assert_eq!(
            ask(t, b"\x1b]21;visual_bell=?\x07"),
            b"\x1b]21;visual_bell=\x1b\\",
            "RIS: nothing set answers empty"
        );
        support::feed(t, set);
        support::feed(t, b"\x1b[!p");
        assert_eq!(t.program_paint(PaintRole::VisualBell), theirs, "DECSTR");
    });
}

/// **`RIS` returns the text under the cursor to the embedder's**, the
/// colour a program set with kitty's `OSC 21 ; cursor_text=` or iTerm2's
/// `SetColors=curfg=` gone. `DECSTR` keeps it.
#[test]
fn ris_returns_the_cursor_text_and_decstr_keeps_it() {
    for set in [
        &b"\x1b]21;cursor_text=#102030\x07"[..],
        &iterm("SetColors=curfg=102030"),
    ] {
        support::with_term(&WS, |t| {
            configured(t);
            support::feed(t, set);
            let theirs = ProgramPaint::Colour((0x10, 0x20, 0x30));
            assert_eq!(t.program_paint(PaintRole::CursorText), theirs);
            support::feed(t, b"\x1bc");
            assert_eq!(
                t.program_paint(PaintRole::CursorText),
                ProgramPaint::Unset,
                "RIS"
            );
            assert_eq!(
                ask(t, b"\x1b]21;cursor_text=?\x07"),
                b"\x1b]21;cursor_text=rgb:09/08/07\x1b\\",
                "RIS: the embedder's answers"
            );
            support::feed(t, set);
            support::feed(t, b"\x1b[!p");
            assert_eq!(t.program_paint(PaintRole::CursorText), theirs, "DECSTR");
        });
    }
}
