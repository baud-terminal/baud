//! DECID, `ESC Z` (`9A` as a C1): the obsolete spelling of a primary DA.
//!
//! `[[source: dec-std-070]]` 070-4, IDENTIFY TERMINAL (PDF 232): "The device
//! responds by transmitting an appropriate primary Device Attributes (DA)
//! response", note a: "This control is identical to a primary DA control
//! with an omitted parameter or a parameter value of zero (0)", note b:
//! "DECID is not recognized at Level 3 or above", and its algorithm calls
//! `device_attributes_1()` at levels 1 and 2. baud's DA names a VT102, a
//! Level 1 device, so DECID answers. `[[source: ctlseqs]]`: "Return Terminal
//! ID (DECID is 0x9a). Obsolete form of CSI c (DA)." Note c sends VT52 mode
//! elsewhere: there `ESC Z` is VT52's own identify, `ESC / Z`.
//!
//! Every case runs at every read size and at every two-read split, so the
//! `ESC` and the `Z` meet in `fast_esc` and in the graph's escape node.

mod support;

use baud_core::*;
use baud_domain::{ImageBudget, SixelClaim};

/// What `bytes` leaves: the reply, row 0's text and the unsupported count.
fn after(bytes: &[u8], read: usize, setup: impl Fn(&mut Term<'_>)) -> (Vec<u8>, String, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws).with_images(ImageBudget::of(64));
    let mut t = Term::new(s.storage(), &ws);
    setup(&mut t);
    for piece in bytes.chunks(read) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    let row: String = t.row(Row::from_index(0)).expect("row 0")[..8]
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => '.',
        })
        .collect();
    (t.reply().to_vec(), row, t.unsupported)
}

/// The same, split once at `at` into two reads.
fn split(bytes: &[u8], at: usize, setup: impl Fn(&mut Term<'_>)) -> (Vec<u8>, String, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws).with_images(ImageBudget::of(64));
    let mut t = Term::new(s.storage(), &ws);
    setup(&mut t);
    for piece in [&bytes[..at], &bytes[at..]] {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    let row: String = t.row(Row::from_index(0)).expect("row 0")[..8]
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => '.',
        })
        .collect();
    (t.reply().to_vec(), row, t.unsupported)
}

/// `decid` and `da` leave the same state at every read size and split, and
/// that state's reply is `want`.
fn same_as_da(decid: &[u8], da: &[u8], want: &[u8], setup: impl Fn(&mut Term<'_>) + Copy) {
    let base = after(da, da.len(), setup);
    assert_eq!(base.0, want, "the DA control itself answers {want:02x?}");
    assert_eq!(base.2, 0, "the DA program counts nothing");
    // Every read size and every split is tried before failing, so a red run
    // names each one that differs.
    let mut wrong = Vec::new();
    for read in 1..=decid.len() {
        if after(decid, read, setup) != base {
            wrong.push(format!("{read}-byte reads"));
        }
    }
    for at in 1..decid.len() {
        if split(decid, at, setup) != base {
            wrong.push(format!("split at {at}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{decid:02x?} differs from DA at {wrong:?}"
    );
}

#[test]
fn decid_answers_the_primary_da() {
    same_as_da(b"ab\x1bZcd", b"ab\x1b[ccd", b"\x1b[?6c", |_| {});
}

#[test]
fn decid_answers_the_sixel_claim_as_da_does() {
    same_as_da(b"ab\x1bZcd", b"ab\x1b[ccd", b"\x1b[?6;4c", |t| {
        t.set_sixel_claim(SixelClaim::Yes)
    });
}

/// Under `S8C1T` the answer's introducer is `9B`, as DA's is.
#[test]
fn decid_answers_in_eight_bits_after_s8c1t() {
    same_as_da(b"\x1b Gab\x1bZcd", b"\x1b Gab\x1b[ccd", b"\x9b?6c", |_| {});
}

/// `9A` is DECID's C1 under ISO 8859-1, read as `ESC Z`.
#[test]
fn the_c1_9a_under_latin1_is_decid() {
    same_as_da(b"\x1b%@ab\x9acd", b"\x1b%@ab\x1b[ccd", b"\x1b[?6c", |_| {});
}

/// In VT52 mode `ESC Z` is VT52's identify and keeps its own answer
/// (`[[source: vt510-decanm]]`: "ESC /Z Report"), DEC STD 070's note c.
#[test]
fn in_vt52_mode_esc_z_keeps_the_vt52_answer() {
    let p = b"\x1b[?2lab\x1bZcd";
    for read in 1..=p.len() {
        let (reply, row, n) = after(p, read, |_| {});
        assert_eq!(reply, b"\x1b/Z", "{read}-byte reads");
        assert_eq!(&row[..4], "abcd", "{read}-byte reads");
        assert_eq!(n, 0, "{read}-byte reads");
    }
}
