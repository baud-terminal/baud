//! A GPU's image textures ([`ImageTextures`]) are made once for each
//! snapshot the renderer keeps, so an image is uploaded again when its own
//! pixels change and not when another image moves the store's epoch. `()`
//! stands for a texture: the count is the cache's, and no device is needed.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, ImageBudget, ImagePx, ReadLen, Rows};
use baud_font::Face;
use baud_render::{ImageTextures, Renderer};
use support::{WS, font, renderer};

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const RED: [u8; 4] = [200, 40, 40, 255];
const GREEN: [u8; 4] = [40, 200, 40, 255];
const BLUE: [u8; 4] = [40, 40, 200, 255];

fn feed(t: &mut Term, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// `data` in base64, padded.
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

/// A kitty graphics command: the keys `ctrl`, and `px`'s RGBA pixels as
/// its payload when there are any.
fn kitty(ctrl: &str, px: &[[u8; 4]]) -> Vec<u8> {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    if !px.is_empty() {
        v.push(b';');
        v.extend_from_slice(&b64(&px.concat()));
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

/// One frame's images, and a GPU's textures made for every placement.
fn frame(r: &mut Renderer<'_, '_>, t: &Term<'_>, tex: &mut ImageTextures<()>) {
    r.set_images(t, Rows::ZERO);
    tex.keep(r.image_uploads());
    for p in r.image_placements() {
        let up = r.image_uploads().iter().find(|u| u.id == p.id);
        let up = up.expect("the premise: every placement is snapshotted");
        assert_eq!(tex.of(up, |_| Some(())), Some(()));
    }
}

/// **A texture is made again only for an image whose own pixels changed**:
/// two kitty images on screen, then the second edited in place (`a=f`,
/// `r=1`), a third stored and never placed, and the first freed and stored
/// again under its id. Each moves the epoch; the edit and the second store
/// of the first each make one texture, and the unplaced store none.
#[test]
fn a_texture_is_made_again_only_for_an_image_whose_pixels_changed() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &WS);
    let m = r.metrics();
    t.set_cell_size(
        ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
        ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
    );
    let mut tex = ImageTextures::default();
    feed(&mut t, b"\x1b[1;1H");
    feed(
        &mut t,
        &kitty("a=T,f=32,s=16,v=16,i=1,C=1,q=2", &[RED; 256]),
    );
    feed(&mut t, b"\x1b[1;20H");
    feed(
        &mut t,
        &kitty("a=T,f=32,s=16,v=16,i=2,C=1,q=2", &[BLUE; 256]),
    );
    frame(&mut r, &t, &mut tex);
    assert_eq!(r.image_placements().len(), 2, "the premise: both shown");
    assert_eq!(tex.made().value(), 2);

    feed(
        &mut t,
        &kitty("a=f,f=32,s=8,v=8,x=0,y=0,r=1,i=2,q=2", &[GREEN; 64]),
    );
    frame(&mut r, &t, &mut tex);
    assert_eq!(
        tex.made().value(),
        3,
        "the edited image alone uploads again"
    );

    feed(&mut t, &kitty("a=t,f=32,s=4,v=4,i=3,q=2", &[GREEN; 16]));
    for _ in 0..3 {
        frame(&mut r, &t, &mut tex);
    }
    assert_eq!(
        tex.made().value(),
        3,
        "an image stored off screen uploads nothing"
    );

    feed(&mut t, &kitty("a=d,d=I,i=1,q=2", &[]));
    feed(&mut t, b"\x1b[1;1H");
    feed(
        &mut t,
        &kitty("a=T,f=32,s=16,v=16,i=1,C=1,q=2", &[GREEN; 256]),
    );
    frame(&mut r, &t, &mut tex);
    assert_eq!(r.image_placements().len(), 2, "the premise: both shown");
    assert_eq!(
        tex.made().value(),
        4,
        "an image stored under a freed one's id uploads"
    );
}
