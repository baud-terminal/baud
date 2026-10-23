//! iTerm2's `OSC 1337` messages: each key baud acts on, at every read size,
//! with each answer byte for byte in iTerm2's shape, and every key it does
//! not act on counted by key (`[[source: iterm2-escape-codes]]`).

use baud_core::*;
use baud_domain::{
    BackingScale, ClipboardBudget, ClipboardContent, CursorStyle, DynamicColor, ExitStatus,
    FileHost, ItermDoes, ItermKey, Messages, Selection, SpecialColor, UnicodeWidths,
};

const ONE: Messages = Messages::of(1);

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 10,
};

/// The read sizes every test feeds at: a byte at a time, a prime that splits
/// every key, and whole.
const SIZES: [usize; 4] = [1, 3, 7, 4096];

fn feed(t: &mut Term<'_>, bytes: &[u8], size: usize) {
    for read in bytes.chunks(size) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
    }
}

/// A term with a history and a clipboard area, fed `bytes` in reads of
/// `size`.
fn with<R>(bytes: &[u8], size: usize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(100))
        .with_clipboard(ClipboardBudget::of_text(8192));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes, size);
    f(&mut t)
}

fn osc(body: &str) -> Vec<u8> {
    format!("\x1b]1337;{body}\x07").into_bytes()
}

fn b64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for g in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..g.len()].copy_from_slice(g);
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            let s = (n >> (18 - 6 * i)) & 63;
            out.push(match i <= g.len() {
                true => char::from(A[usize::try_from(s).expect("a sextet")]),
                false => '=',
            });
        }
    }
    out
}

// ── CursorShape ─────────────────────────────────────────────────────────────

/// "0: Block 1: Vertical bar 2: Underline": the shape changes and the blink
/// stays (Jev, KEEP_BLINK); a value that is not a number is 0, as iTerm2's
/// `intValue` reads it (Jev, BLOCK); one outside 0-2 changes nothing and is
/// counted.
#[test]
fn cursor_shape_sets_the_shape_and_keeps_the_blink() {
    for size in SIZES {
        with(&osc("CursorShape=1"), size, |t| {
            assert_eq!(
                t.cursor_style(),
                CursorStyle::BlinkingBar,
                "reads of {size}"
            );
            feed(t, b"\x1b[2 q", size);
            feed(t, &osc("CursorShape=2"), size);
            assert_eq!(
                t.cursor_style(),
                CursorStyle::SteadyUnderline,
                "reads of {size}"
            );
            feed(t, &osc("CursorShape=bar"), size);
            assert_eq!(
                t.cursor_style(),
                CursorStyle::SteadyBlock,
                "reads of {size}"
            );
            feed(t, &osc("CursorShape=3"), size);
            assert_eq!(
                t.cursor_style(),
                CursorStyle::SteadyBlock,
                "reads of {size}"
            );
            assert_eq!(
                t.iterm_counts().of(ItermKey::CursorShape),
                ONE,
                "reads of {size}"
            );
        });
    }
}

// ── ClearScrollback ─────────────────────────────────────────────────────────

fn lines(n: usize) -> Vec<u8> {
    (0..n)
        .flat_map(|i| format!("line {i}\r\n").into_bytes())
        .collect()
}

fn screen_text(t: &Term<'_>) -> Vec<Vec<Content>> {
    let v = t.viewport();
    (0..v.rows().index())
        .map(|r| {
            v.row(Row::from_index(r))
                .iter()
                .map(|c| c.content)
                .collect()
        })
        .collect()
}

/// "To erase the scrollback history": the saved lines go and the screen
/// stays, as `CSI 3 J` (the documentation's sentence; iTerm2's source
/// clears the screen too, a departure Jev did not reach 0.9 for).
#[test]
fn clear_scrollback_erases_the_history_and_keeps_the_screen() {
    for size in SIZES {
        with(&lines(40), size, |t| {
            assert_eq!(t.scrollback().len(), 31, "the control: a history");
            let before = screen_text(t);
            let erasures = t.history_erasures();
            feed(t, &osc("ClearScrollback"), size);
            assert_eq!(t.scrollback().len(), 0, "reads of {size}");
            assert_eq!(screen_text(t), before, "reads of {size}: the screen stays");
            assert_ne!(t.history_erasures(), erasures, "reads of {size}");
        });
    }
}

// ── CurrentDir and RemoteHost ───────────────────────────────────────────────

/// `CurrentDir` names the directory as sent; a `RemoteHost` names the host
/// the following directories are on, and alone leaves the directory unknown
/// (Jev, LAST_REMOTE_HOST, 0.96/0.98); an empty `CurrentDir` leaves none and
/// is counted (Jev's lean, LEAVES_NONE).
#[test]
fn current_dir_and_remote_host_name_the_directory() {
    for size in SIZES {
        with(&osc("CurrentDir=/tmp/a%20b c"), size, |t| {
            let d = t.working_directory().expect("kept");
            assert_eq!(d.host, FileHost::Local, "reads of {size}");
            assert_eq!(d.path, b"/tmp/a%20b c", "reads of {size}: as sent");
            feed(t, &osc("RemoteHost=me@corp@box.example"), size);
            assert_eq!(t.working_directory(), None, "reads of {size}");
            feed(t, &osc("CurrentDir=/srv"), size);
            let d = t.working_directory().expect("kept");
            assert_eq!(d.host, FileHost::Named(b"box.example"), "reads of {size}");
            assert_eq!(d.path, b"/srv", "reads of {size}");
            feed(t, &osc("RemoteHost=me@"), size);
            feed(t, &osc("CurrentDir=/home/me"), size);
            assert_eq!(t.working_directory().expect("kept").host, FileHost::Local);
            let refused = t.directories_refused();
            feed(t, &osc("CurrentDir="), size);
            assert_eq!(t.working_directory(), None, "reads of {size}");
            assert_ne!(t.directories_refused(), refused, "reads of {size}");
        });
    }
}

// ── ReportCellSize ──────────────────────────────────────────────────────────

/// "OSC 1337 ; ReportCellSize=[height];[width];[scale] ST", in points, each
/// as iTerm2 writes it (Jev, COMPACTED), ended as the query was (Jev's lean,
/// QUERY_TERMINATOR); silent and counted while the cell or the scale is
/// untold.
#[test]
fn report_cell_size_answers_in_points() {
    for size in SIZES {
        with(b"", size, |t| {
            t.set_cell_size(ImagePx::of(17), ImagePx::of(34));
            t.set_backing_scale(BackingScale::of(2.0));
            feed(t, &osc("ReportCellSize"), size);
            assert_eq!(t.reply(), b"\x1b]1337;ReportCellSize=17.0;8.5;2.0\x07");
            t.clear_reply();
            feed(t, b"\x1b]1337;ReportCellSize\x1b\\", size);
            assert_eq!(t.reply(), b"\x1b]1337;ReportCellSize=17.0;8.5;2.0\x1b\\");
            t.clear_reply();
            t.set_backing_scale(BackingScale::of(3.0));
            feed(t, &osc("ReportCellSize"), size);
            assert_eq!(t.reply(), b"\x1b]1337;ReportCellSize=11.33;5.67;3.0\x07");
        });
        with(b"", size, |t| {
            t.set_cell_size(ImagePx::of(17), ImagePx::of(34));
            feed(t, &osc("ReportCellSize"), size);
            assert_eq!(t.reply(), b"", "reads of {size}: no scale told");
            assert_eq!(t.iterm_counts().of(ItermKey::ReportCellSize), ONE);
        });
    }
}

// ── ReportVariable ──────────────────────────────────────────────────────────

/// Every variable is answered with iTerm2's empty value while reports are
/// off, its shape for one not reported (Jev's lean, EMPTY_AS_DENIED), ended
/// as the query was; a name that is not base64 gets no reply, as in iTerm2,
/// and is counted.
#[test]
fn report_variable_answers_with_the_empty_value() {
    for size in SIZES {
        let ask = format!("ReportVariable={}", b64(b"session.name"));
        with(&osc(&ask), size, |t| {
            assert_eq!(
                t.reply(),
                b"\x1b]1337;ReportVariable=\x07",
                "reads of {size}"
            );
            t.clear_reply();
            feed(t, &format!("\x1b]1337;{ask}\x1b\\").into_bytes(), size);
            assert_eq!(
                t.reply(),
                b"\x1b]1337;ReportVariable=\x1b\\",
                "reads of {size}"
            );
            t.clear_reply();
            feed(t, &osc("ReportVariable=no!"), size);
            assert_eq!(t.reply(), b"", "reads of {size}");
            assert_eq!(t.iterm_counts().of(ItermKey::ReportVariable), ONE);
        });
    }
}

// ── UnicodeVersion ──────────────────────────────────────────────────────────

/// The column the cursor stands at after `text` is written from column 0.
fn width_of(t: &mut Term<'_>, text: &str, size: usize) -> usize {
    feed(t, b"\r\x1b[2K", size);
    feed(t, text.as_bytes(), size);
    t.cursor().1.index()
}

/// **"8 or 9"**: under 8 an emoji Unicode 9 made wide, U+1F600 and U+231A,
/// takes one column, CJK still two and a flag still two, the pair widened as
/// iTerm2 widens it, where a lone regional indicator is East Asian Width
/// 8.0's one column; a flag that reaches the right margin wraps whole. 9 and
/// 0 are the current tables. `push` keeps the version under a label and
/// `pop` restores it; a label no entry has empties the stack and changes
/// nothing. Cells already written keep their width.
#[test]
fn unicode_version_selects_the_width_tables() {
    for size in SIZES {
        with(&osc("UnicodeVersion=8"), size, |t| {
            assert_eq!(t.unicode_widths(), UnicodeWidths::Eight, "reads of {size}");
            assert_eq!(width_of(t, "\u{1F600}", size), 1, "reads of {size}");
            assert_eq!(width_of(t, "\u{231A}", size), 1);
            assert_eq!(width_of(t, "\u{4E2D}", size), 2);
            assert_eq!(width_of(t, "\u{1F1EF}\u{1F1F5}", size), 2, "a flag");
            assert_eq!(width_of(t, "\u{1F1EF}", size), 1, "a lone indicator");
            assert_eq!(
                width_of(t, "\u{1F1EF}\u{1F1F5}\u{1F1EF}", size),
                3,
                "a flag and a lone indicator"
            );
            // The flag's first indicator in the last column: the pair goes
            // to the next row whole, the last column left blank.
            let edge = format!(
                "{}{}",
                "a".repeat(usize::from(WS.ws_col) - 1),
                "\u{1F1EF}\u{1F1F5}"
            );
            feed(t, b"\x1b[H\x1b[2J", size);
            feed(t, edge.as_bytes(), size);
            assert_eq!(
                t.cursor(),
                (Row::from_index(1), Col::at_index(2)),
                "reads of {size}"
            );
            feed(t, &osc("UnicodeVersion=9"), size);
            assert_eq!(width_of(t, "\u{1F1EF}", size), 2, "a lone indicator, 9");
            assert_eq!(width_of(t, "\u{1F600}", size), 2, "reads of {size}");
            assert_eq!(width_of(t, "\u{231A}", size), 2);
            // push under a label, change, pop the label back.
            feed(t, &osc("UnicodeVersion=push vim"), size);
            feed(t, &osc("UnicodeVersion=8"), size);
            feed(t, &osc("UnicodeVersion=push"), size);
            feed(t, &osc("UnicodeVersion=9"), size);
            feed(t, &osc("UnicodeVersion=pop vim"), size);
            assert_eq!(
                t.unicode_widths(),
                UnicodeWidths::Current,
                "reads of {size}"
            );
            feed(t, &osc("UnicodeVersion=push a"), size);
            feed(t, &osc("UnicodeVersion=8"), size);
            feed(t, &osc("UnicodeVersion=pop nobody"), size);
            assert_eq!(
                t.unicode_widths(),
                UnicodeWidths::Eight,
                "no entry has the label"
            );
            feed(t, &osc("UnicodeVersion=pop"), size);
            assert_eq!(
                t.unicode_widths(),
                UnicodeWidths::Eight,
                "the stack was emptied"
            );
            feed(t, &osc("UnicodeVersion=0"), size);
            assert_eq!(
                t.unicode_widths(),
                UnicodeWidths::Current,
                "0 is the default"
            );
            feed(t, &osc("UnicodeVersion=10"), size);
            assert_eq!(t.iterm_counts().of(ItermKey::UnicodeVersion), ONE);
        });
    }
}

/// **The version is saved with the cursor and kept by `RIS`**, as iTerm2
/// keeps both; a push past sixteen entries is refused and counted, and
/// cells written under 8 keep their one column under 9.
#[test]
fn unicode_version_is_saved_with_the_cursor_and_kept_by_ris() {
    for size in SIZES {
        with(&osc("UnicodeVersion=8"), size, |t| {
            feed(t, "\x1b7".as_bytes(), size);
            feed(t, &osc("UnicodeVersion=9"), size);
            feed(t, "\x1b8".as_bytes(), size);
            assert_eq!(
                t.unicode_widths(),
                UnicodeWidths::Eight,
                "DECRC, reads of {size}"
            );
            feed(t, b"\x1bc", size);
            assert_eq!(
                t.unicode_widths(),
                UnicodeWidths::Eight,
                "RIS, reads of {size}"
            );
            feed(t, "\u{1F600}".as_bytes(), size);
            feed(t, &osc("UnicodeVersion=9"), size);
            assert_eq!(t.cursor().1.index(), 1, "the cell written under 8 stays");
            for _ in 0..17 {
                feed(t, &osc("UnicodeVersion=push"), size);
            }
            assert_eq!(
                t.iterm_counts().of(ItermKey::UnicodeVersion),
                ONE,
                "reads of {size}"
            );
        });
    }
}

// ── RequestUpload ───────────────────────────────────────────────────────────

/// **Refused with silence**, baud's refusal where an answer is unsafe:
/// neither "abort" and a newline, which would reach a shell as an entered
/// command, nor iTerm2's Control-C, which is not a status the page names;
/// any format alike. Counted among the refusals.
#[test]
fn request_upload_is_refused_with_silence() {
    for size in SIZES {
        with(&osc("RequestUpload=format=tgz;version=eA=="), size, |t| {
            assert_eq!(t.reply(), b"", "reads of {size}");
            t.clear_reply();
            feed(t, &osc("RequestUpload=format=zip"), size);
            assert_eq!(t.reply(), b"", "reads of {size}");
            let n = t.iterm_counts().of(ItermKey::RequestUpload);
            assert_eq!(n, Messages::of(2), "reads of {size}");
            assert_eq!(t.iterm_counts().refused(), Messages::of(2));
        });
    }
}

// ── SetMark ─────────────────────────────────────────────────────────────────

/// **"Record a location and then jump back to it later"**: the cursor's line
/// is a stop for Cmd-Up and Cmd-Down, beside the shell's prompts, with no
/// exit status; `saveScrollPosition` marks the window's top line; on the
/// alternate screen it is counted and marks nothing.
#[test]
fn set_mark_is_a_line_the_prompt_jumps_stop_at() {
    for size in SIZES {
        // The mark goes on the line `line 3` is written on, the fourth.
        let mut bytes = lines(3);
        bytes.extend(osc("SetMark"));
        bytes.extend(lines(30));
        with(&bytes, size, |t| {
            let marks: Vec<PromptMark> = t.prompts().collect();
            assert_eq!(marks.len(), 1, "reads of {size}");
            assert_eq!(marks[0].status, ExitStatus::Unreported);
            // 33 lines and the cursor's empty one on a 10-row screen: the
            // screen's top is line 24, the mark line 3, 21 back.
            assert_eq!(
                marks[0].line,
                PromptLine::Back(Rows::of_count(21)),
                "reads of {size}"
            );
            assert!(t.jump_to_prompt(PromptJump::Previous), "reads of {size}");
            assert_eq!(t.window_back(), Rows::of_count(21), "reads of {size}");
            assert!(!t.jump_to_prompt(PromptJump::Previous), "nothing older");
            assert!(!t.jump_to_prompt(PromptJump::Next), "nothing newer");
            t.window_live();
            // `saveScrollPosition`: the window's top, three lines back.
            t.set_window_back(Rows::of_count(3));
            feed(t, &osc("SetMark=saveScrollPosition"), size);
            t.window_live();
            assert!(t.jump_to_prompt(PromptJump::Previous), "reads of {size}");
            assert_eq!(t.window_back(), Rows::of_count(3), "reads of {size}");
            t.window_live();
            // The alternate screen keeps no history: counted, no mark.
            feed(t, b"\x1b[?1049h", size);
            feed(t, &osc("SetMark"), size);
            feed(t, b"\x1b[?1049l", size);
            assert_eq!(t.prompts().count(), 2, "reads of {size}");
            assert_eq!(
                t.iterm_counts().of(ItermKey::SetMark),
                ONE,
                "reads of {size}"
            );
        });
    }
}

// ── SetUserVar ──────────────────────────────────────────────────────────────

/// **"Sets the value of a user-defined variable"**: `user.<name>` keeps the
/// decoded value, which the embedder reads, and `ReportVariable` answers it
/// with the empty value, as iTerm2 answers a variable the user has not
/// allowed; a bare name, or a value that is not base64, unsets it, and a
/// name with a `.` is refused, as iTerm2's `UserVariableAssignment` has it.
#[test]
fn set_user_var_is_kept_and_reported_empty() {
    let ask = |name: &str| osc(&format!("ReportVariable={}", b64(name.as_bytes())));
    for size in SIZES {
        let set = osc(&format!("SetUserVar=gitBranch={}", b64(b"main")));
        with(&set, size, |t| {
            assert_eq!(
                t.user_var(b"gitBranch"),
                Some(&b"main"[..]),
                "reads of {size}"
            );
            feed(t, &ask("user.gitBranch"), size);
            assert_eq!(
                t.reply(),
                b"\x1b]1337;ReportVariable=\x07",
                "set, reads of {size}"
            );
            t.clear_reply();
            // Only the `user.` name: the bare name is another variable.
            feed(t, &ask("gitBranch"), size);
            assert_eq!(
                t.reply(),
                b"\x1b]1337;ReportVariable=\x07",
                "reads of {size}"
            );
            t.clear_reply();
            // A second set keeps the slot; a bare name unsets it.
            feed(
                t,
                &osc(&format!("SetUserVar=gitBranch={}", b64(b"dev"))),
                size,
            );
            assert_eq!(t.user_var(b"gitBranch"), Some(&b"dev"[..]));
            feed(t, &osc("SetUserVar=gitBranch"), size);
            assert_eq!(t.user_var(b"gitBranch"), None, "reads of {size}");
            feed(t, &ask("user.gitBranch"), size);
            assert_eq!(
                t.reply(),
                b"\x1b]1337;ReportVariable=\x07",
                "reads of {size}"
            );
            t.clear_reply();
            // A value that is not base64 unsets too.
            feed(t, &osc(&format!("SetUserVar=host={}", b64(b"box"))), size);
            feed(t, &osc("SetUserVar=host=not base64!"), size);
            assert_eq!(t.user_var(b"host"), None, "reads of {size}");
            // "key contains a ., which is not allowed".
            feed(t, &osc(&format!("SetUserVar=a.b={}", b64(b"x"))), size);
            assert_eq!(t.user_var(b"a.b"), None);
            assert_eq!(
                t.iterm_counts().of(ItermKey::SetUserVar),
                ONE,
                "reads of {size}"
            );
        });
    }
}

/// **The table is bounded, and refuses rather than evicts**: the
/// seventeenth name and a name past 64 bytes are counted and kept nowhere,
/// and the sixteen set before are all still read back.
#[test]
fn user_vars_past_the_table_are_refused_and_counted() {
    let mut bytes = Vec::new();
    for i in 0..17 {
        bytes.extend(osc(&format!("SetUserVar=v{i}={}", b64(b"x"))));
    }
    bytes.extend(osc(&format!("SetUserVar={}={}", "n".repeat(65), b64(b"x"))));
    for size in SIZES {
        with(&bytes, size, |t| {
            for i in 0..16 {
                assert_eq!(t.user_var(format!("v{i}").as_bytes()), Some(&b"x"[..]));
            }
            assert_eq!(t.user_var(b"v16"), None, "reads of {size}");
            assert_eq!(
                t.iterm_counts().of(ItermKey::SetUserVar),
                Messages::of(2),
                "reads of {size}"
            );
        });
    }
}

// ── Copy ────────────────────────────────────────────────────────────────────

/// "OSC 1337 ; Copy=:[base64] ST ... the base64-encoded string to copy to
/// the pasteboard": the general pasteboard, under `OSC 52`'s rules, and a
/// copy longer than an `OSC` body is kept whole.
#[test]
fn copy_puts_the_text_on_the_clipboard() {
    let long: Vec<u8> = b"abcdefghijklmnopqrstuvwxyz"
        .iter()
        .copied()
        .cycle()
        .take(3000)
        .collect();
    for size in SIZES {
        for text in [&b"hello"[..], &long[..], b""] {
            with(&osc(&format!("Copy=:{}", b64(text))), size, |t| {
                let w = t.clipboard().expect("published");
                assert!(w.targets.contains(Selection::Clipboard), "reads of {size}");
                assert_eq!(w.content, ClipboardContent::Set(text), "reads of {size}");
            });
        }
        with(&osc("Copy=:not base64!"), size, |t| {
            assert_eq!(t.clipboard(), None, "reads of {size}: nothing copied");
            assert_eq!(t.iterm_counts().of(ItermKey::Copy), ONE, "reads of {size}");
        });
    }
}

// ── SetColors ───────────────────────────────────────────────────────────────

/// Each `key=value` of a list split on `,` (Jev, COMMA_PAIRS) sets the
/// colour baud keeps for that role, as `OSC 4`, `OSC 10`-`19` and `OSC 5`
/// do; a P3 or Generic RGB value is converted to sRGB; a name baud keeps no
/// colour for is counted.
#[test]
fn set_colors_sets_each_colour_named() {
    for size in SIZES {
        with(
            &osc(
                "SetColors=fg=ff0000,bg=010203,curbg=fff,curfg=0a0b0c,selbg=102030,selfg=405060,bold=00ff00,br_red=abcdef,black=000",
            ),
            size,
            |t| {
                assert_eq!(t.dynamic_color(DynamicColor::Foreground), Some((255, 0, 0)));
                assert_eq!(t.dynamic_color(DynamicColor::Background), Some((1, 2, 3)));
                assert_eq!(t.dynamic_color(DynamicColor::Cursor), Some((255, 255, 255)));
                assert_eq!(
                    t.dynamic_color(DynamicColor::HighlightBg),
                    Some((0x10, 0x20, 0x30))
                );
                assert_eq!(
                    t.dynamic_color(DynamicColor::HighlightFg),
                    Some((0x40, 0x50, 0x60))
                );
                assert_eq!(t.special_color(SpecialColor::Bold), Some((0, 255, 0)));
                assert_eq!(
                    t.program_paint(PaintRole::CursorText),
                    ProgramPaint::Colour((0x0a, 0x0b, 0x0c))
                );
                assert_eq!(t.palette(Chan::from_u8(9)), Some((0xab, 0xcd, 0xef)));
                assert_eq!(t.palette(Chan::from_u8(0)), Some((0, 0, 0)));
                let before = t.unsupported;
                feed(
                    t,
                    &osc("SetColors=link=fff,tab=default,nope=fff,red=zzz"),
                    size,
                );
                assert_eq!(t.unsupported - before, 4, "reads of {size}");
                feed(t, &osc("SetColors=red=p3:ff0000,green=rgb:00ff00"), size);
                let p3 = baud_domain::themefile::gamut::to_srgb(
                    baud_domain::themefile::gamut::Space::DisplayP3,
                    [1.0, 0.0, 0.0],
                )
                .0;
                let generic = baud_domain::themefile::gamut::to_srgb(
                    baud_domain::themefile::gamut::Space::Calibrated,
                    [0.0, 1.0, 0.0],
                )
                .0;
                assert_eq!(t.palette(Chan::from_u8(1)), Some((p3[0], p3[1], p3[2])));
                assert_eq!(
                    t.palette(Chan::from_u8(2)),
                    Some((generic[0], generic[1], generic[2]))
                );
            },
        );
    }
}

// ── the rest ────────────────────────────────────────────────────────────────

/// Every key baud does not act on changes nothing and is counted under its
/// key, the unsafe ones among the refusals; a key iTerm2 does not know is
/// counted apart.
#[test]
fn every_key_not_acted_on_is_counted() {
    for size in SIZES {
        for key in ItermKey::ALL {
            if key.does() == ItermDoes::Acts {
                continue;
            }
            let name = core::str::from_utf8(key.name()).unwrap();
            with(&osc(&format!("{name}=x")), size, |t| {
                assert_eq!(t.iterm_counts().of(key), ONE, "{name}, reads of {size}");
                assert_eq!(t.strings_ignored, 1, "{name}, reads of {size}");
                let unsafe_ = matches!(key.does(), ItermDoes::Refuses(_));
                let want = match unsafe_ {
                    true => ONE,
                    false => Messages::NONE,
                };
                assert_eq!(t.iterm_counts().refused(), want, "{name}");
            });
        }
        with(&osc("NoSuchKey=1"), size, |t| {
            assert_eq!(t.iterm_counts().unknown, ONE, "reads of {size}");
            assert_eq!(t.strings_ignored, 1, "reads of {size}");
        });
    }
}

/// **iTerm2's segmented copy, as its `it2copy` sends it inside tmux**:
/// `Copy=2;uid`, a `Copy=3;uid:base64` for each line of `base64 | fold`,
/// and `Copy=4;uid`, with other output between them; a piece under another
/// `uid` abandons the copy, and an end with none open is counted (Jev's
/// lean, BUILD_SEGMENTS).
#[test]
fn a_segmented_copy_is_copied_at_its_end() {
    let text: Vec<u8> = b"0123456789".iter().copied().cycle().take(700).collect();
    let enc = b64(&text);
    let mut it2copy = osc("Copy=2;31337");
    for line in enc.as_bytes().chunks(80) {
        it2copy.extend(osc(&format!(
            "Copy=3;31337:{}",
            std::str::from_utf8(line).unwrap()
        )));
        it2copy.extend_from_slice(b"output between\r\n");
    }
    for size in SIZES {
        with(&it2copy, size, |t| {
            assert_eq!(
                t.clipboard(),
                None,
                "reads of {size}: nothing before the end"
            );
            feed(t, &osc("Copy=4;31337"), size);
            let w = t.clipboard().expect("published");
            assert_eq!(
                w.content,
                ClipboardContent::Set(&text[..]),
                "reads of {size}"
            );
            assert_eq!(t.iterm_counts().of(ItermKey::Copy), Messages::NONE);
        });
        // One piece past the 512-byte body, streamed as a whole copy is.
        let mut one = osc("Copy=2;9");
        one.extend(osc(&format!("Copy=3;9:{enc}")));
        one.extend(osc("Copy=4;9"));
        with(&one, size, |t| {
            let w = t.clipboard().expect("published");
            assert_eq!(
                w.content,
                ClipboardContent::Set(&text[..]),
                "reads of {size}"
            );
        });
        let mut other = osc("Copy=2;1");
        other.extend(osc("Copy=3;2:aGk="));
        other.extend(osc("Copy=4;1"));
        with(&other, size, |t| {
            assert_eq!(t.clipboard(), None, "reads of {size}: abandoned");
            assert_eq!(t.iterm_counts().of(ItermKey::Copy), Messages::of(2));
        });
    }
}
