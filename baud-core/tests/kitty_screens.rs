//! **Each screen names its own kitty images**: an `i=` or an `I=` sent on
//! one screen names that screen's image and never the other's, so a
//! transmission, a put, a delete and a placeholder on the primary leave the
//! alternate screen's pictures as they were, and the other way round
//! (`[[source: kitty-screen-c]]` `screen_toggle_screen_buffer()`,
//! `[[source: kitty-graphics-c]]`).

use baud_core::*;
use baud_domain::{
    DecodeBudget, DecodeProgress, ImageBudget, ImagePx, ImageScreen, KittyId, KittyNumber,
    KittyPlacement,
};

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

fn with<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    f(&mut t)
}

/// Feed `bytes` and finish every decode; the replies they owed.
fn feed(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            if k <= q.len() {
                out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]);
            } else {
                out.push(b'=');
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

/// A 6x4 RGBA image, transmitted under `ctrl`.
fn image(ctrl: &str) -> Vec<u8> {
    let px: Vec<u8> = (0..24u8).flat_map(|i| [i, 2 * i, 3 * i, 255]).collect();
    apc(&format!("f=32,s=6,v=4,{ctrl}"), &b64(&px))
}

/// The screen and image of every placement, in slot order.
fn placed(t: &Term<'_>) -> Vec<(ImageScreen, baud_domain::ImageId)> {
    t.images()
        .placements()
        .map(|(_, p)| (p.screen, p.image))
        .collect()
}

/// **An `i=5` on each screen is two images**: the alternate screen's
/// transmission does not replace the primary's, and each is placed on its
/// own screen.
#[test]
fn an_id_sent_on_each_screen_is_two_images() {
    with(|t| {
        feed(t, &image("a=T,i=5,q=2"));
        feed(t, b"\x1b[?1049h");
        feed(t, &image("a=T,i=5,q=2"));
        let p = placed(t);
        assert_eq!(p.len(), 2, "one placement on each screen: {p:?}");
        assert_eq!(p[0].0, ImageScreen::Primary);
        assert_eq!(p[1].0, ImageScreen::Alternate);
        assert_ne!(p[0].1, p[1].1, "two images");
        feed(t, b"\x1b[?1049l");
        assert_eq!(placed(t).len(), 2, "leaving keeps the alternate's");
    });
}

/// **`a=d,d=I,i=5` on the primary leaves the alternate screen's image 5**,
/// placed and stored, and takes the primary's own.
#[test]
fn deleting_an_id_on_the_primary_leaves_the_alternates() {
    with(|t| {
        feed(t, b"\x1b[?1049h");
        feed(t, &image("a=T,i=5,q=2"));
        feed(t, b"\x1b[?1049l");
        let alt = placed(t);
        assert_eq!(alt.len(), 1, "kept on leaving");
        assert_eq!(alt[0].0, ImageScreen::Alternate);
        feed(t, &apc("a=d,d=I,i=5,q=2", b""));
        assert_eq!(placed(t), alt, "the alternate's image 5, from the primary");
        feed(t, &image("a=T,i=5,q=2"));
        assert_eq!(placed(t).len(), 2, "the primary's own image 5");
        feed(t, &apc("a=d,d=I,i=5,q=2", b""));
        assert_eq!(placed(t), alt, "the primary's image 5 alone");
        for d in ["a=d,d=r,x=1,y=9,q=2", "a=d,d=Z,z=0,q=2", "a=d,d=N,I=5,q=2"] {
            feed(t, &apc(d, b""));
            assert_eq!(placed(t), alt, "{d} on the primary");
        }
        feed(t, b"\x1b[?47h");
        feed(t, &apc("a=d,d=I,i=5,q=2", b""));
        assert!(placed(t).is_empty(), "its own screen's delete");
        assert!(t.images().kitty_image(KittyId::of(5)).is_none(), "freed");
    });
}

/// **A put names the live screen's image**: image 7 stored on the primary
/// is no image on the alternate screen, which answers ENOENT; image
/// number 9 names the newest of the live screen's.
#[test]
fn a_put_names_the_live_screens_image() {
    with(|t| {
        feed(t, &image("a=t,i=7,q=2"));
        feed(t, &image("a=t,I=9,q=2"));
        let nine = t.images().kitty_newest(KittyNumber::of(9));
        assert!(nine.is_some(), "the primary's image number 9");
        feed(t, b"\x1b[?1049h");
        let r = feed(t, &apc("a=p,i=7", b""));
        assert!(
            r.starts_with(b"\x1b_Gi=7;ENOENT"),
            "{:?}",
            String::from_utf8_lossy(&r)
        );
        assert!(placed(t).is_empty(), "nothing placed");
        feed(t, &image("a=t,I=9,q=2"));
        feed(t, b"\x1b[?1049l");
        feed(t, &apc("a=p,I=9,q=2", b""));
        let p = placed(t);
        assert_eq!(p.len(), 1, "placed on the primary");
        assert_eq!(
            Some(p[0].1),
            nine,
            "the primary's image number 9, though the alternate's is newer"
        );
        let r = feed(t, &apc("a=p,i=7", b""));
        assert_eq!(r, b"\x1b_Gi=7;OK\x1b\\", "the primary's image 7");
    });
}

/// **A placeholder shows the live screen's virtual placement**: image 3
/// has one on each screen, two cells wide on the primary and five on the
/// alternate, and each screen's placeholder finds its own.
#[test]
fn a_placeholder_shows_the_live_screens_virtual_placement() {
    with(|t| {
        let cols = |t: &Term<'_>| {
            t.kitty_virtual(KittyId::of(3), KittyPlacement::NONE)
                .map(|v| v.place.cols.index())
        };
        feed(t, &image("a=T,U=1,i=3,c=2,r=1,q=2"));
        feed(t, b"\x1b[?1049h");
        assert_eq!(cols(t), None, "none on the alternate screen");
        feed(t, &image("a=T,U=1,i=3,c=5,r=1,q=2"));
        assert_eq!(cols(t), Some(5), "the alternate's");
        feed(t, b"\x1b[?1049l");
        assert_eq!(cols(t), Some(2), "the primary's, kept");
    });
}
