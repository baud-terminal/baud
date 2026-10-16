//! A kitty placement the terminal does not make is never answered `OK`
//! (`[[source: kitty-graphics]]`, "Display images on screen": "the
//! terminal emulator will reply to the placement request with an
//! acknowledgement code"; "Suppressing responses from the terminal": `q=1`
//! suppresses `OK` and `q=2` failures too).
//!
//! A full placement table -- the store's 64 on screen, the
//! stream's [`KITTY_VIRTUALS`] virtual ones -- refuses the placement and
//! answers `ENOSPC`, the code the text names for storage that has run
//! out, in its error form (a code, a colon, a message). Each case is fed
//! at every read size and at every two-read split of the refused command,
//! at `q` absent, 1 and 2; the immediate `a=T` and the deferred one (`o=z`,
//! answered when its decode completes) are each held too.

use baud_core::*;
use baud_domain::{
    DecodeBudget, DecodeProgress, ImageBudget, ImagePx, KITTY_VIRTUALS, KittyId, KittyPlacement,
};

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

/// The store's placement table, `storage.rs`'s `NPLACEMENT`, which the
/// crate does not export; `fill_placements` holds the premise.
const NPLACEMENT: usize = 64;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            match k <= q.len() {
                true => out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => out.push(b'='),
            }
        }
    }
    out
}

fn apc(ctrl: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    if !payload.is_empty() {
        v.push(b';');
        v.extend_from_slice(payload);
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

/// A 6x4 RGBA image's pixels.
fn pixels() -> Vec<u8> {
    (0..24u8).flat_map(|i| [i, 2 * i, 3 * i, 255]).collect()
}

/// The 6x4 image transmitted under `ctrl`, stored when its chunk ends.
fn image(ctrl: &str) -> Vec<u8> {
    apc(&format!("f=32,s=6,v=4,{ctrl}"), &b64(&pixels()))
}

/// The 6x4 image through `o=z`: placed at its terminator, answered when
/// its deferred decode completes.
fn image_z(ctrl: &str) -> Vec<u8> {
    let z = miniz_oxide::deflate::compress_to_vec_zlib(&pixels(), 6);
    apc(&format!("f=32,s=6,v=4,o=z,{ctrl}"), &b64(&z))
}

/// Feed `bytes` as reads cut at `cuts`, finishing any deferred decode
/// after each read, and keep every reply.
fn feed_cut(t: &mut Term<'_>, bytes: &[u8], cuts: &[usize]) -> Vec<u8> {
    let mut replies = Vec::new();
    let mut from = 0;
    for &to in cuts.iter().chain([bytes.len()].iter()) {
        let read = &bytes[from..to];
        from = to;
        if read.is_empty() {
            continue;
        }
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
        while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
        replies.extend_from_slice(t.reply());
        t.clear_reply();
    }
    replies
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    feed_cut(t, bytes, &[])
}

/// Every way to read `bytes`: whole, in reads of each size, and in two
/// reads split at each byte.
fn cuts(len: usize) -> Vec<Vec<usize>> {
    let mut all = vec![Vec::new()];
    for n in 1..len {
        all.push((n..len).step_by(n).collect());
    }
    for at in 1..len {
        all.push(vec![at]);
    }
    all
}

/// A terminal with image memory and a 10x20 cell, image 3 stored.
fn with<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    assert!(feed(&mut t, &image("a=t,i=3,q=2")).is_empty());
    f(&mut t)
}

/// Fill the virtual table with placements of image 3.
fn fill_virtuals(t: &mut Term<'_>) {
    for p in 1..=KITTY_VIRTUALS {
        let r = feed(t, &apc(&format!("a=p,U=1,i=3,p={p},c=1,r=1,q=2"), b""));
        assert!(r.is_empty());
    }
    assert_eq!(t.kitty().virtuals_refused, 0, "the premise: all held");
}

/// Fill the placement table with placements of image 3, the cursor kept.
fn fill_placements(t: &mut Term<'_>) {
    for p in 1..=NPLACEMENT {
        let r = feed(t, &apc(&format!("a=p,i=3,p={p},C=1,q=2"), b""));
        assert!(r.is_empty());
    }
    assert_eq!(t.images().placements().count(), NPLACEMENT, "the premise");
    assert_eq!(t.images().placements_refused, 0, "the premise");
}

/// The reply a refused command owes at each `q`: the error at `q` absent
/// and `q=1`, nothing at `q=2`.
fn owed(q: &str, keys: &str, error: &str) -> Vec<u8> {
    match q {
        ",q=2" => Vec::new(),
        _ => format!("\x1b_G{keys};{error}\x1b\\").into_bytes(),
    }
}

const QUIET: [&str; 3] = ["", ",q=1", ",q=2"];
const VIRTUAL_FULL: &str = "ENOSPC:No room for another virtual placement";
const PLACEMENT_FULL: &str = "ENOSPC:No room for another placement";

/// **A virtual placement the full table refuses is answered ENOSPC**,
/// never `OK`: `a=p,U=1` at every read size and two-read split. On the
/// commit before, `q` absent answered `OK` and `q=1` nothing.
#[test]
fn a_virtual_placement_a_full_table_refuses_is_answered_enospc() {
    for q in QUIET {
        let cmd = apc(&format!("a=p,U=1,i=3,p=999,c=1,r=1{q}"), b"");
        for cut in cuts(cmd.len()) {
            with(|t| {
                fill_virtuals(t);
                let r = feed_cut(t, &cmd, &cut);
                assert_eq!(
                    String::from_utf8_lossy(&r),
                    String::from_utf8_lossy(&owed(q, "i=3,p=999", VIRTUAL_FULL)),
                    "q{q:?} cut {cut:?}"
                );
                assert_eq!(t.kitty().virtuals_refused, 1);
                assert_eq!(t.kitty().refused, 1, "refused with an error");
                let v = t.kitty_virtual(KittyId::of(3), KittyPlacement::of(999));
                assert!(v.is_none(), "nothing held");
            });
        }
    }
}

/// **A full placement table answers ENOSPC and moves no cursor**: `a=p`
/// at every read size and two-read split, with the cursor's move asked
/// for. On the commit before, `q` absent answered `OK`.
#[test]
fn a_placement_a_full_table_refuses_is_answered_enospc() {
    for q in QUIET {
        let cmd = apc(&format!("a=p,i=3,p=999{q}"), b"");
        for cut in cuts(cmd.len()) {
            with(|t| {
                fill_placements(t);
                let before = t.cursor();
                let r = feed_cut(t, &cmd, &cut);
                assert_eq!(
                    String::from_utf8_lossy(&r),
                    String::from_utf8_lossy(&owed(q, "i=3,p=999", PLACEMENT_FULL)),
                    "q{q:?} cut {cut:?}"
                );
                assert_eq!(t.cursor(), before, "no placement, no move");
                assert_eq!(t.images().placements_refused, 1);
                assert_eq!(t.kitty().refused, 1, "refused with an error");
            });
        }
    }
}

/// **`a=T` answers for the placement it asked for**, as it answers EINVAL
/// for a relative one: the image is stored and the placement refused, so
/// the reply is the refusal -- the immediate transmission at its
/// terminator and the deferred one when its decode completes, into either
/// table. The image stays, as any transmitted image does.
#[test]
fn a_transmission_whose_placement_is_refused_is_answered_enospc() {
    type Fill = fn(&mut Term<'_>);
    type Make = fn(&str) -> Vec<u8>;
    let cases: [(&str, Fill, &str, Make); 4] = [
        (
            "a=T,U=1,i=4,p=1,c=1,r=1",
            fill_virtuals,
            VIRTUAL_FULL,
            image,
        ),
        (
            "a=T,U=1,i=4,p=1,c=1,r=1",
            fill_virtuals,
            VIRTUAL_FULL,
            image_z,
        ),
        ("a=T,i=4,p=1", fill_placements, PLACEMENT_FULL, image),
        ("a=T,i=4,p=1", fill_placements, PLACEMENT_FULL, image_z),
    ];
    for (ctrl, fill, error, make) in cases {
        for q in QUIET {
            let cmd = make(&format!("{ctrl}{q}"));
            for cut in cuts(cmd.len()) {
                with(|t| {
                    fill(t);
                    let before = t.cursor();
                    let r = feed_cut(t, &cmd, &cut);
                    assert_eq!(
                        String::from_utf8_lossy(&r),
                        String::from_utf8_lossy(&owed(q, "i=4,p=1", error)),
                        "{ctrl}{q} cut {cut:?}"
                    );
                    assert_eq!(t.cursor(), before, "no placement, no move");
                    assert!(
                        t.images().kitty_image(KittyId::of(4)).is_some(),
                        "the image is stored"
                    );
                    assert_eq!(t.kitty().refused, 1, "refused with an error");
                });
            }
        }
    }
}

/// **The controls: a table with room answers `OK`**, and a virtual
/// placement of an image no id names -- `a=T,U=1` with neither `i` nor
/// `I` -- answers nothing, the text replying only to a command that named
/// an image ("If it does so, the terminal emulator will reply"); it is
/// refused and counted, a placeholder having no id to name it by.
#[test]
fn a_table_with_room_answers_ok_and_a_nameless_image_answers_nothing() {
    with(|t| {
        let r = feed(t, &apc("a=p,U=1,i=3,p=1,c=1,r=1", b""));
        assert_eq!(r, b"\x1b_Gi=3,p=1;OK\x1b\\", "virtual");
        let r = feed(t, &apc("a=p,i=3,p=1,C=1", b""));
        assert_eq!(r, b"\x1b_Gi=3,p=1;OK\x1b\\", "on screen");
        assert_eq!(t.kitty().refused, 0);
    });
    for q in QUIET {
        with(|t| {
            let r = feed(t, &image(&format!("a=T,U=1,c=1,r=1{q}")));
            assert!(r.is_empty(), "q{q:?}: {}", String::from_utf8_lossy(&r));
            assert_eq!(t.kitty().virtuals_refused, 1, "q{q:?}");
            assert_eq!(t.kitty().refused, 1, "q{q:?}: refused, unanswered");
            assert!(!t.kitty_virtuals(), "q{q:?}: nothing held");
        });
    }
}
