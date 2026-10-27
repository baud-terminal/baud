//! **NP, PP, PPA, PPR and PPB on baud's one page** (`[[source: ecma48]]`
//! 8.3.87, 8.3.95 to 8.3.98; `[[source: dec-std-070]]` pp. 5-77 to 5-83).
//! baud's page memory is one page, its screen (Jev, ONE_PAGE, 0.94/0.94).
//! STD 070's algorithms clamp the page to `number_of_pages` and NP and PP
//! then take "the home position in the new page obeying Origin Mode", so
//! on one page NP and PP home the cursor and PPA, PPR and PPB, which keep
//! the line and column, move nothing (Jev, STD070_HOME, 0.92/0.98); the
//! VT420's and VT510's "If there is only one page, then the terminal
//! ignores NP" is the products' departure. A page asked for that baud
//! does not have is counted in `Term::pages_missing`.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 12,
    ws_col: 40,
};

fn cursor(bytes: &[u8]) -> (usize, usize) {
    support::render(&WS, bytes, |t| {
        let (r, c) = t.cursor();
        (r.index(), c.index())
    })
}

/// **NP and PP home the cursor**, whatever their count, 0 and 1 meaning
/// one page.
#[test]
fn np_and_pp_home_the_cursor_on_the_one_page() {
    for f in ["U", "0U", "1U", "3U", "V", "0V", "2V"] {
        let q = format!("\x1b[5;7H\x1b[{f}");
        assert_eq!(cursor(q.as_bytes()), (0, 0), "CSI {f}");
    }
}

/// **Obeying Origin Mode**: "if (origin_mode == DISPLACED) { line =
/// top_margin; column = left_margin; }" (pp. 5-78, 5-80), the left margin
/// DECSLRM's under DECLRMM.
#[test]
fn np_and_pp_home_to_the_margins_under_decom() {
    assert_eq!(cursor(b"\x1b[3;10r\x1b[?6h\x1b[5;7H\x1b[U"), (2, 0), "NP");
    assert_eq!(
        cursor(b"\x1b[3;10r\x1b[?69h\x1b[5;20s\x1b[?6h\x1b[5;7H\x1b[V"),
        (2, 4),
        "PP"
    );
    assert_eq!(
        cursor(b"\x1b[3;10r\x1b[5;7H\x1b[U"),
        (0, 0),
        "DECOM reset: the page's first line and column"
    );
}

/// **PPA, PPR and PPB keep the line and column** ("to the corresponding
/// line and column", pp. 5-81 to 5-83), and the page is the one page.
#[test]
fn ppa_ppr_and_ppb_keep_the_line_and_column() {
    for f in [" P", "1 P", "4 P", " Q", "2 Q", " R", "3 R"] {
        let q = format!("\x1b[5;7H\x1b[{f}x");
        support::render(&WS, q.as_bytes(), |t| {
            let (r, c) = t.cursor();
            assert_eq!((r.index(), c.index()), (4, 7), "CSI {f}");
            assert_eq!(t.unsupported, 0, "CSI {f} is acted on");
        });
    }
}

/// **Each is acted on, not counted as unsupported**: before this, all five
/// reached no handler.
#[test]
fn the_page_functions_are_not_unsupported() {
    support::render(&WS, b"\x1b[U\x1b[V\x1b[ P\x1b[ Q\x1b[ R", |t| {
        assert_eq!(t.unsupported, 0)
    });
}

/// **A page baud does not have is counted**: NP, PP, PPR and PPB always
/// name another page, PPA one past the first; PPA 1, and PPA's default,
/// name the one page and count nothing.
#[test]
fn a_page_baud_lacks_is_counted() {
    let count = |b: &[u8]| support::render(&WS, b, |t| t.pages_missing());
    let none = count(b"");
    assert_eq!(count(b"\x1b[ P"), none, "PPA's default, page 1");
    assert_eq!(count(b"\x1b[1 P"), none, "PPA 1");
    assert_eq!(count(b"\x1b[0 P"), none, "PPA 0, page 1");
    for f in ["U", "V", "2 P", " Q", " R"] {
        let q = format!("\x1b[{f}");
        assert_ne!(count(q.as_bytes()), none, "CSI {f}");
    }
    assert_eq!(
        count(b"\x1b[U\x1b[V\x1b[2 P"),
        count(b"\x1b[U\x1b[U\x1b[U"),
        "one a control"
    );
}

/// **NP ends a pending wrap** as the home of CUP does: the next character
/// goes at the home, not after a wrap.
#[test]
fn np_ends_a_pending_wrap() {
    let mut fill = b"\x1b[1;40H".to_vec();
    fill.extend_from_slice(b"x\x1b[Uy");
    support::render(&WS, &fill, |t| {
        let first = t.viewport().row(Row::from_index(0))[0];
        assert_eq!(
            first.content.as_codepoint().map(|c| c.value()),
            Some(u32::from('y'))
        );
        assert_eq!(t.cursor().0.index(), 0);
    });
}

/// **The private namespace has none of them**: `CSI ? U` and `CSI ? SP P`
/// are counted and move nothing.
#[test]
fn a_private_marker_is_no_page_function() {
    for q in [&b"\x1b[5;7H\x1b[?U"[..], b"\x1b[5;7H\x1b[?2 P"] {
        support::render(&WS, q, |t| {
            assert_eq!(t.unsupported, 1);
            assert_eq!(t.cursor().0.index(), 4);
        });
    }
}

/// **Below Level 3 none acts** ("Levels: 3x (Windowing), 4"): DECSCL 62
/// leaves the cursor and counts each.
#[test]
fn below_level_three_the_page_functions_do_nothing() {
    support::render(&WS, b"\x1b[62\"p\x1b[5;7H\x1b[U\x1b[V", |t| {
        let (r, c) = t.cursor();
        assert_eq!((r.index(), c.index()), (4, 6));
        assert_eq!(t.unsupported, 2);
    });
    assert_eq!(cursor(b"\x1b[63\"p\x1b[5;7H\x1b[U"), (0, 0), "Level 3");
}

/// **DECPCCM is not recognized** (Jev's lean, NOT_RECOGNIZED, 0.32/0.52):
/// one page has no other page to couple, and STD 070 has "DEC Private
/// modes that cannot be set or reset from the host will report as
/// unknown" (p. 5-194). DECRQM answers 0 and `SM ? 64` and `RM ? 64` are
/// counted.
#[test]
fn decpccm_is_not_recognized_on_one_page() {
    support::render(&WS, b"\x1b[?64$p", |t| {
        assert_eq!(t.reply(), b"\x1b[?64;0$y")
    });
    support::render(&WS, b"\x1b[?64h\x1b[?64l", |t| {
        assert_eq!(t.unsupported_param, 2)
    });
}

/// **DECRQDE still names page 1 after NP**: the one page is displayed.
#[test]
fn decrqde_names_the_one_page_after_np() {
    support::render(&WS, b"\x1b[3U\x1b[\"v", |t| {
        assert_eq!(t.reply(), b"\x1b[12;40;1;1;1\"w")
    });
}
