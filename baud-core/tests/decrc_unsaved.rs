//! `DECRC` with nothing saved: the cursor goes home and the state the save
//! buffer holds goes back to its reset values.
//!
//! `[[source: vt510-decrc]]`: "If nothing was saved by DECSC, then DECRC
//! performs the following actions: Moves the cursor to the home position
//! (upper left of screen). Resets origin mode (DECOM). Turns all character
//! attributes off (normal setting). Maps the ASCII character set into GL, and
//! the DEC Supplemental Graphic set into GR." `[[source: vt220-rm-ch4]]` § 4.7
//! says the same and ends "the default character set mapping is established",
//! and its Table 4-10 gives the saved state a `DECSTR` leaves as the cursor
//! home, the default sets, "Selective erase attribute bit write state Off",
//! normal SGR and origin mode reset. `[[source: dec-std-070]]` § 5.13.1,
//! DECSC note 3 (p. 5-187), clears the buffer to the same values on
//! initialization or reset, `attribute.selective erase = FALSE` among them, and
//! note 4 has `DECSTR` do so too; `DECSTR` and `RIS` leave nothing saved here,
//! so a `DECRC` after either takes this path.
//!
//! **What none of the texts names is left alone**: `DECAWM`, which 070's
//! buffer does not hold and vt510's no-save list does not reset. GR goes back
//! to G2 with the slots (`the_resets_restore_gr_and_the_supplemental_slots`).
//! `SCORC` keeps its own path, `scorc_without_a_save_is_inert`:
//! its own text, MS-DOS 2.00's ANSI.SYS (`[[source: msdos2-ansi-doc]]`),
//! restores a position it says nothing about before a save, and what xterm
//! does is note AE's Q13.
//!
//! Every case is asserted at every read size.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 12,
    ws_col: 20,
};

/// Each read size of `bytes` gives the same `f`, and that is `want`.
fn at_every_read_size<R: PartialEq + core::fmt::Debug>(
    bytes: &[u8],
    want: R,
    f: impl Fn(&mut Term<'_>) -> R,
) {
    for read in 1..=bytes.len() {
        let got = support::with_term(&WS, |t| {
            for piece in bytes.chunks(read) {
                t.write_at(BytePos::ZERO, piece);
                t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
            }
            f(t)
        });
        assert_eq!(got, want, "{read}-byte reads of {bytes:02x?}");
    }
}

fn cursor(t: &mut Term<'_>) -> (usize, usize) {
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

fn text(t: &mut Term<'_>, row: usize) -> String {
    t.row(Row::from_index(row)).expect("a row on the grid")[..20]
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => '.',
        })
        .collect()
}

/// What row 0's first cell shows.
fn first(t: &mut Term<'_>) -> char {
    text(t, 0).chars().next().unwrap_or('?')
}

#[test]
fn decrc_with_nothing_saved_homes_the_cursor() {
    at_every_read_size(b"\x1b[4;9H\x1b8", (0, 0), cursor);
}

/// Origin mode reset: with DECOM left set, `CUP 3;1` would land on row 6, the
/// third row of the 5-10 region; reset, it lands on row 2. And home is the
/// screen's, not the region's.
#[test]
fn decrc_with_nothing_saved_resets_origin_mode() {
    at_every_read_size(b"\x1b[5;10r\x1b[?6h\x1b[3;4H\x1b8", (0, 0), cursor);
    at_every_read_size(b"\x1b[5;10r\x1b[?6h\x1b8\x1b[3;1H", (2, 0), cursor);
}

/// "Turns all character attributes off": a bold red rendition is gone, and
/// the next character is written in the default one.
#[test]
fn decrc_with_nothing_saved_turns_the_attributes_off() {
    at_every_read_size(b"\x1b[1;31;44m\x1b8X", true, |t| {
        let c = t.row(Row::ZERO).expect("a row on the grid")[0];
        t.style_of(c) == StyleWord::DEFAULT
    });
    // And an `SGR` after it builds on the default, not on what was set
    // before: underline alone, neither bold nor red.
    at_every_read_size(b"\x1b[1;31m\x1b8\x1b[4mX\x1b[0;4mY", true, |t| {
        let r = t.row(Row::ZERO).expect("a row on the grid");
        let (x, y) = (r[0], r[1]);
        t.style_of(x) == t.style_of(y)
    });
}

/// The default mapping: ASCII in G0 and G1 and DEC Supplemental, the
/// user-preference supplemental set, in G2 and G3 (`[[source: dec-std-070]]`
/// §3.6.4.3), G0 in GL and G2 in GR (`gr_slot`). A G0 designated DEC Special
/// Graphics draws `q` as `q` again, a G1 invoked by `SO` is shifted back out,
/// and a G1 designated DEC Special Graphics is ASCII again for a later `SO`.
#[test]
fn decrc_with_nothing_saved_establishes_the_default_mapping() {
    at_every_read_size(b"\x1b(0\x1b8q", 'q', first);
    at_every_read_size(b"\x1b)0\x0e\x1b8q", 'q', first);
    // GL is G0 again, not G1 holding ASCII: a G1 designated after the
    // restore does not reach GL without another `SO`.
    at_every_read_size(b"\x1b)0\x0e\x1b8\x1b)0q", 'q', first);
    at_every_read_size(b"\x1b)0\x1b8\x0eq", 'q', first);
    let sup = Charset::Dec(DecSet::Supplemental);
    at_every_read_size(
        b"\x1b*0\x1b+0\x1b|\x1b8",
        ([Charset::Ascii, Charset::Ascii, sup, sup], GSlot::G2),
        |t| {
            let slots = [GSlot::G0, GSlot::G1, GSlot::G2, GSlot::G3].map(|g| t.charset_in(g));
            (slots, t.gr_slot())
        },
    );
}

/// The selective erase attribute off, as 070's reset buffer and vt220's
/// Table 4-10 hold it: a character written after `DECSCA 1` and `DECRC` is
/// erasable, so `DECSED` makes it a space.
#[test]
fn decrc_with_nothing_saved_writes_erasable_characters() {
    at_every_read_size(b"\x1b[1\"q\x1b8X\x1b[?2J", ' ', first);
}

/// After `DECSTR` and after `RIS` nothing is saved, so a save made before
/// either is gone and `DECRC` takes the path above.
#[test]
fn decrc_after_a_reset_takes_the_nothing_saved_path() {
    at_every_read_size(b"\x1b[5;5H\x1b7\x1b[!p\x1b[3;3H\x1b8", (0, 0), cursor);
    at_every_read_size(b"\x1b[5;5H\x1b7\x1bc\x1b[3;3H\x1b8", (0, 0), cursor);
}

/// **Guards.** `DECAWM` is not among what any text resets, so a line longer
/// than the row still clips. A save still restores what it saved. `SCORC`
/// with nothing saved stays where it is, on its own path.
#[test]
fn decrc_with_nothing_saved_leaves_the_rest_alone() {
    at_every_read_size(
        b"\x1b[?7l\x1b8abcdefghijklmnopqrstuvwxyz",
        ("abcdefghijklmnopqrsz".to_string(), (0, 19)),
        |t| (text(t, 0), cursor(t)),
    );
    at_every_read_size(b"\x1b[4;9H\x1b7\x1b[1;1H\x1b8", (3, 8), cursor);
    at_every_read_size(b"\x1b[4;9H\x1b[u", (3, 8), cursor);
    at_every_read_size(b"\x1b(0\x1b[u", Charset::DecGraphics, |t| t.charset());
}
