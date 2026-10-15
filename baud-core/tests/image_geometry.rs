//! What an image client asks a terminal before it sends a picture: how big
//! a cell is in pixels, how big the text area is, and what graphics the
//! terminal will take.
//!
//! yazi 26.5.6 is the client these are written against, and its own source
//! is pinned as `yazi-emulator` so the shapes below are the ones it parses
//! rather than the ones this file guesses at. It sends one burst -- `CSI > q`,
//! `CSI 16 t`, `OSC 11 ; ?`, `CSI 0 c` -- and takes the cell size from the
//! `CSI 6 ; h ; w t` in the answer, or failing that from the pty's
//! `ws_xpixel` and `ws_ypixel`. With neither it sizes every image to a fixed
//! 600x900 whatever the pane, which is what baud gave it until this change.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use baud_domain::ImagePx;

/// A screenful of text, one distinct line a row, so a scroll moves row 0.
fn numbered() -> Vec<u8> {
    let mut v = Vec::new();
    for r in 0..24 {
        if r > 0 {
            v.extend_from_slice(b"\r\n");
        }
        v.extend_from_slice(format!("row{r:02}").as_bytes());
    }
    v
}

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

/// **XTSMGRAPHICS is recognised as not SU, and counted.** `CSI ? Pi ; Pa ;
/// Pv S` shares its final byte with `CSI Ps S` (scroll up), and a parser
/// that dropped the `?` would scroll the screen when a Sixel client asked
/// how many colour registers there are. A note carried from the Sixel work
/// said this happened; the parser routes `?` through its private branch
/// first, and this is the test that says which is true.
///
/// Since 2026-10-15 the three-parameter form is answered
/// (`sixel_modes.rs`); here, on a terminal not configured for graphics, with
/// failures, and a form of another arity is still counted unanswered.
#[test]
fn xtsmgraphics_does_not_scroll_and_is_counted() {
    support::with_term(&WS, |t| {
        support::feed(t, &numbered());
        let before = support::screen(t);
        let counted = t.unsupported;
        // Colour registers, Sixel geometry and ReGIS geometry, each read, and
        // the registers read with two parameters.
        support::feed(t, b"\x1b[?1;1;0S\x1b[?2;1;0S\x1b[?3;1;0S\x1b[?1;1S");
        assert_eq!(
            support::screen(t),
            before,
            "an XTSMGRAPHICS query moved the screen"
        );
        assert_eq!(
            t.reply(),
            b"\x1b[?1;3S\x1b[?2;3S\x1b[?3;1S",
            "not configured for graphics: failures, and no ReGIS item"
        );
        assert_eq!(
            t.unsupported,
            counted + 1,
            "the two-parameter form is counted"
        );
        // The control: plain SU does scroll, so a screen that did not move is
        // the private marker being honoured and not a test that cannot fail.
        support::feed(t, b"\x1b[2S");
        assert_ne!(support::screen(t), before, "SU itself did not scroll");
    });
}

/// The cell yazi would see on this Mac's Retina display at the owner's size:
/// 17 by 34 device pixels, the unit every render path draws a Sixel pixel in.
fn retina(t: &mut Term<'_>) {
    t.set_cell_size(ImagePx::of(17), ImagePx::of(34));
}

/// `[[source: ctlseqs]]`'s three XTWINOPS reports, from the cell size the
/// embedder gave and the grid the terminal has.
#[test]
fn the_window_queries_answer_from_the_cell_size_and_the_grid() {
    support::with_term(&WS, |t| {
        retina(t);
        support::feed(t, b"\x1b[14t\x1b[16t\x1b[18t");
        assert_eq!(
            t.reply(),
            b"\x1b[4;816;1360t\x1b[6;34;17t\x1b[8;24;80t",
            "text area 24x34 by 80x17, the cell, the grid"
        );
    });
}

/// **Before the embedder says how big a cell is, the pixel answers are
/// silence and a count**, the way an unanswerable `DSR` is: a zero would be
/// the right shape and a size nobody has. The cell count needs no pixels.
#[test]
fn without_a_cell_size_only_the_cell_count_is_answered() {
    support::with_term(&WS, |t| {
        let counted = t.unsupported_param;
        support::feed(t, b"\x1b[14t\x1b[16t\x1b[18t");
        assert_eq!(t.reply(), b"\x1b[8;24;80t");
        assert_eq!(t.unsupported_param, counted + 2);
        // And a form baud refuses is counted the same way, not as an unknown
        // operation: `CSI 22 ; 2 t` is vim's title push.
        let (param, unknown) = (t.unsupported_param, t.unsupported);
        support::feed(t, b"\x1b[22;2t\x1b[14;2t\x1b[t");
        assert_eq!(t.reply(), b"\x1b[8;24;80t", "a refused form was answered");
        assert_eq!(t.unsupported_param, param + 3);
        assert_eq!(t.unsupported, unknown, "XTWINOPS is a known operation");
    });
}

/// A query answers with what the terminal has *now*: after a resize, the new
/// grid; after a zoom, the new cell.
#[test]
fn the_answers_follow_a_resize_and_a_zoom() {
    support::with_term(&WS, |t| {
        retina(t);
        support::shrink(
            t,
            &WinSize {
                ws_col: 60,
                ws_row: 20,
            },
        );
        t.set_cell_size(ImagePx::of(9), ImagePx::of(18));
        t.clear_reply();
        support::feed(t, b"\x1b[14t\x1b[16t\x1b[18t");
        assert_eq!(t.reply(), b"\x1b[4;360;540t\x1b[6;18;9t\x1b[8;20;60t");
    });
}

/// yazi 26.5.6's detection burst, byte for byte from `[[source:
/// yazi-emulator]]`'s `detect`: the Kitty graphics probe (sent because baud
/// is no brand it knows), `CSI > q`, `CSI 16 t`, `OSC 11 ; ? BEL`, `CSI 0 c`.
const YAZI_DETECT: &[u8] =
    b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[>q\x1b[16t\x1b]11;?\x07\x1b[0c";

/// yazi's own `csi_16t`, ported from `[[source: yazi-emulator]]` with the
/// `unsafe` UTF-8 conversion made checked and nothing else changed: the first
/// `ESC [ 6 ;`, then digits, `;`, digits, `t`, returned `(w, h)`. Reading the
/// reply with the consumer's parser rather than with this file's own
/// expectation is the check that the shape is one yazi can take.
fn yazi_csi_16t(resp: &str) -> Option<(u16, u16)> {
    let b = resp.split_once("\x1b[6;")?.1.as_bytes();
    let h: Vec<_> = b
        .iter()
        .copied()
        .take_while(|&c| c.is_ascii_digit())
        .collect();
    b.get(h.len()).filter(|&&c| c == b';')?;
    let w: Vec<_> = b[h.len() + 1..]
        .iter()
        .copied()
        .take_while(|&c| c.is_ascii_digit())
        .collect();
    b.get(h.len() + 1 + w.len()).filter(|&&c| c == b't')?;
    let (w, h) = (String::from_utf8(w).ok()?, String::from_utf8(h).ok()?);
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// **What yazi reads from baud's answer to its burst: the cell size.**
/// Without it `Dimension::cell_size` (`[[source: yazi-dimension]]`) falls back
/// to the pty's pixels, and with those zero too, to nothing -- and every
/// image is drawn at a fixed 600x900 whatever the pane.
///
/// It must also end where yazi stops reading. `read_until_da1` stops at a
/// `c` whose last `ESC` begins `[?`, so a reply that put the cell size after
/// the DA would be lost; the core answers in the order asked, so the DA is
/// last.
#[test]
fn yazi_reads_the_cell_size_from_its_detection_burst() {
    support::with_term(&WS, |t| {
        retina(t);
        support::feed(t, YAZI_DETECT);
        let resp = String::from_utf8(t.reply().to_vec()).expect("the replies are ASCII");
        assert_eq!(yazi_csi_16t(&resp), Some((17, 34)), "{resp:?}");
        let last_esc = resp.rfind('\x1b').expect("an escape");
        assert!(
            resp[last_esc..].starts_with("\x1b[?") && resp.ends_with('c'),
            "the DA1 reply, which ends yazi's read, is not last: {resp:?}"
        );
    });
    // The control: with no cell size yazi's parser finds nothing, so the
    // assertion above is about the reply and not a parser that matches
    // anything.
    support::with_term(&WS, |t| {
        support::feed(t, YAZI_DETECT);
        let resp = String::from_utf8(t.reply().to_vec()).expect("the replies are ASCII");
        assert_eq!(yazi_csi_16t(&resp), None, "{resp:?}");
    });
}

/// The burst answered the same whatever the read size, down to one byte a
/// read, where no sequence is ever whole in one `feed`.
#[test]
fn the_answers_are_chunk_invariant() {
    let whole = support::with_term(&WS, |t| {
        retina(t);
        support::feed(t, YAZI_DETECT);
        support::feed(t, b"\x1b[14t\x1b[18t");
        t.reply().to_vec()
    });
    for chunk in [1, 2, 3, 5, 7] {
        let split = support::with_term(&WS, |t| {
            retina(t);
            let mut bytes = YAZI_DETECT.to_vec();
            bytes.extend_from_slice(b"\x1b[14t\x1b[18t");
            for piece in bytes.chunks(chunk) {
                t.write_at(BytePos::ZERO, piece);
                t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
            }
            t.reply().to_vec()
        });
        assert_eq!(split, whole, "read size {chunk}");
    }
}

/// **`?2048`'s pixel fields are the text area once the cell size is known**,
/// which `[[source: inband-resize]]` makes an obligation: "A terminal MUST
/// report pixel sizes if it is capable of reporting them."
#[test]
fn the_inband_report_carries_pixels_once_the_cell_is_known() {
    support::with_term(&WS, |t| {
        retina(t);
        support::feed(t, b"\x1b[?2048h");
        assert_eq!(t.reply(), b"\x1b[48;24;80;816;1360t");
        t.clear_reply();
        support::shrink(
            t,
            &WinSize {
                ws_col: 60,
                ws_row: 20,
            },
        );
        assert_eq!(t.reply(), b"\x1b[48;20;60;680;1020t");
    });
}

/// **A cell-size change owes a report even when the grid does not move.**
/// `[[source: inband-resize]]`: "Font size changes can also affect the text
/// area size." A zoom that re-lays to the same column and row count still
/// changed every pixel the report carries.
#[test]
fn a_cell_size_change_reports_even_with_the_grid_unchanged() {
    support::with_term(&WS, |t| {
        retina(t);
        support::feed(t, b"\x1b[?2048h");
        t.clear_reply();
        t.set_cell_size(ImagePx::of(20), ImagePx::of(40));
        assert_eq!(t.reply(), b"\x1b[48;24;80;960;1600t");
        // The same size again changes nothing, so nothing is sent.
        t.clear_reply();
        t.set_cell_size(ImagePx::of(20), ImagePx::of(40));
        assert!(t.reply().is_empty(), "an unchanged cell was reported");
    });
    // And with the mode off, a cell-size change says nothing.
    support::with_term(&WS, |t| {
        retina(t);
        t.set_cell_size(ImagePx::of(20), ImagePx::of(40));
        assert!(t.reply().is_empty(), "a report arrived unasked for");
    });
}

/// yazi's own `light_bg`, ported from `[[source: yazi-emulator]]`: it takes
/// what follows `]11;rgb:` and reads the channels at the **fixed offsets**
/// 0..2, 5..7 and 10..12 -- the top byte of each channel in the four-digit
/// `rrrr/gggg/bbbb` form xterm answers with. yazi turns a failed read into
/// "dark" through `unwrap_or_default`; this returns `None` instead, so the
/// test can tell a dark answer from an unreadable one.
fn yazi_light_bg(resp: &str) -> Option<bool> {
    let (_, s) = resp.split_once("]11;rgb:")?;
    if s.len() < 14 {
        return None;
    }
    let chan = |r: core::ops::Range<usize>| {
        s.get(r)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .map(f32::from)
    };
    let (r, g, b) = (chan(0..2)?, chan(5..7)?, chan(10..12)?);
    let luma = r * 0.2627 / 256.0 + g * 0.6780 / 256.0 + b * 0.0593 / 256.0;
    Some(luma > 0.6)
}

/// **yazi can tell a light background from a dark one only if the reply is
/// four digits a channel.** `[[source: ctlseqs]]` accepts either width in
/// what an application *sends*, and xterm answers in four; yazi, like other
/// readers of that answer, parses it at fixed offsets. The two-digit
/// `rgb:ef/ef/ef` baud sent until 2026-09-22 read as a failure there, which
/// yazi reports as dark whatever the theme.
#[test]
fn yazi_reads_a_light_background_as_light_and_a_dark_one_as_dark() {
    for (bg, light) in [
        ((0xefu8, 0xefu8, 0xefu8), true),
        ((0x10, 0x10, 0x10), false),
    ] {
        support::with_term(&WS, |t| {
            t.set_default_colors((0x27, 0x27, 0x27), bg);
            support::feed(t, YAZI_DETECT);
            let resp = String::from_utf8(t.reply().to_vec()).expect("the replies are ASCII");
            assert_eq!(yazi_light_bg(&resp), Some(light), "{resp:?}");
        });
    }
}
