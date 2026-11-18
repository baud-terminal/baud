//! **A kitty graphics retransmission replaces the image it names at the
//! end of its transmission, and not before.**
//!
//! "When re-transmitting image data for a specific id, the existing image
//! and all its placements must be deleted. The new data replaces the old
//! image data" (`[[source: kitty-graphics-rst]]`, the note under "Display
//! images on screen"), and "terminals must not display anything, until the
//! entire sequence is received and validated" ("Remote client"). So a
//! frame drawn between two reads of a retransmission -- inside one
//! command, or between its chunks -- shows the old image and every
//! placement of it, a virtual one included, and the terminator of the
//! transmission's last chunk deletes them and makes the new image in one
//! step. A transmission that never ends replaces nothing: one dropped past
//! [`KITTY_BODY_MAX`](baud_domain::KITTY_BODY_MAX) leaves the old image
//! drawn, and one cancelled before it ends keeps the old image and drops
//! the new one. When the arena cannot hold the old image and the
//! new one together, the old one is deleted when the new one is reserved,
//! and no other image goes for it.
use baud_core::*;
use baud_domain::{
    ControlStringEnd, DecodeBudget, DecodeProgress, ImageBudget, ImageOwner, ImagePx,
    KITTY_BODY_MAX, KittyId, KittyPlacement, Row,
};

const WS: WinSize = WinSize {
    ws_row: 12,
    ws_col: 60,
};

/// Room for every image these tests store.
const ROOMY: ImageBudget = ImageBudget { pixels: 1 << 16 };

const RED: u32 = 0xffff_0000;
const BLUE: u32 = 0xff00_00ff;
const GREEN: u32 = 0xff00_ff00;

/// RFC 4648 base64 of `data`, padded.
fn b64(data: &[u8]) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let n = q
            .iter()
            .enumerate()
            .fold(0u32, |w, (i, &b)| w | u32::from(b) << (16 - 8 * i));
        for k in 0..4 {
            match k <= q.len() {
                true => out.push(A[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => out.push(b'='),
            }
        }
    }
    out
}

/// The base64 of RGB pixels (`f=24`).
fn pixels(px: &[[u8; 3]]) -> Vec<u8> {
    b64(&px.concat())
}

/// `APC G <keys> ; <payload> ST`.
fn apc(keys: &str, payload: &[u8]) -> Vec<u8> {
    let mut s = b"\x1b_G".to_vec();
    s.extend_from_slice(keys.as_bytes());
    s.push(b';');
    s.extend_from_slice(payload);
    s.extend_from_slice(b"\x1b\\");
    s
}

/// What a frame drawn now shows of the kitty images: the pixels of every
/// kitty placement whose image is whole, and of id 7's virtual placement.
#[derive(Debug, PartialEq, Eq)]
struct Drawn {
    placed: Vec<Vec<u32>>,
    virtual7: Option<Vec<u32>>,
}

fn drawn(t: &Term<'_>) -> Drawn {
    let imgs = t.images();
    let px = |id| {
        imgs.image(id)
            .map(|v| v.pixels.iter().map(|p| p.bits()).collect::<Vec<u32>>())
    };
    Drawn {
        placed: imgs
            .placements()
            .filter(|(_, p)| matches!(imgs.owner(p.image), Some(ImageOwner::Kitty(..))))
            .filter_map(|(_, p)| px(p.image))
            .collect(),
        virtual7: t
            .kitty_virtual(KittyId::of(7), KittyPlacement::NONE)
            .and_then(|v| px(v.image)),
    }
}

/// A terminal with `budget` of arena and a 10x20 cell.
fn with<R>(budget: ImageBudget, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    with_under(ControlStringEnd::Standard, budget, f)
}

/// [`with`], with an `APC G` ending where `end` says.
fn with_under<R>(
    end: ControlStringEnd,
    budget: ImageBudget,
    f: impl FnOnce(&mut Term<'_>) -> R,
) -> R {
    let mut owned = Owned::new(&WS).with_images(budget);
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    t.set_control_string_end(end);
    f(&mut t)
}

/// One read, every deferred decode run out, the replies kept.
fn feed(t: &mut Term<'_>, bytes: &[u8], replies: &mut Vec<u8>) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
    replies.extend_from_slice(t.reply());
    t.clear_reply();
}

/// Every row's text, for a payload byte that must not print.
fn rows(t: &Term<'_>) -> Vec<String> {
    (0..usize::from(WS.ws_row))
        .map(|y| {
            t.row(Row::from_index(y))
                .expect("a row")
                .iter()
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .map_or(' ', |cp| char::from_u32(cp.value()).unwrap_or('?'))
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

/// Id 7 as a 2x1 red image placed twice on screen and once virtually.
fn old_image() -> Vec<u8> {
    let mut s = apc(
        "a=T,f=24,s=2,v=1,i=7,p=1,C=1,q=2",
        &pixels(&[[255, 0, 0]; 2]),
    );
    s.extend(apc("a=p,i=7,p=2,C=1,q=2", b""));
    s.extend(apc("a=p,U=1,i=7,c=2,r=1,q=2", b""));
    s
}

/// What a frame shows of [`old_image`].
fn old_drawn() -> Drawn {
    Drawn {
        placed: vec![vec![RED; 2], vec![RED; 2]],
        virtual7: Some(vec![RED; 2]),
    }
}

/// `prior`, then `retransmit` cut in two reads at every byte, under `end`:
/// between the reads a frame shows `between`, and after the second
/// `after`; fed whole it shows `after` too.
fn split_everywhere(
    end: ControlStringEnd,
    budget: ImageBudget,
    prior: &[u8],
    retransmit: &[u8],
    between: &Drawn,
    after: &Drawn,
) {
    for cut in 0..=retransmit.len() {
        with_under(end, budget, |t| {
            let mut r = Vec::new();
            feed(t, prior, &mut r);
            feed(t, &retransmit[..cut], &mut r);
            if cut < retransmit.len() {
                assert_eq!(&drawn(t), between, "between the reads, cut at {cut}");
            }
            feed(t, &retransmit[cut..], &mut r);
            assert_eq!(&drawn(t), after, "after the terminator, cut at {cut}");
            assert!(
                rows(t).iter().all(String::is_empty),
                "text drawn: cut at {cut}"
            );
        });
    }
}

/// **A one-command retransmission read in two parts draws the old image,
/// all three placements of it, until its terminator**, `ST`, or under
/// `ControlStringEnd::Kitty` `BEL` too, at every cut; then the old image
/// and its placements are gone together, and
/// the new one is drawn for `a=T` and is not for `a=t`, which "is not
/// actually displayed until a placement for it is created".
#[test]
fn a_retransmission_read_in_two_parts_draws_the_old_image_until_its_terminator() {
    let blue = pixels(&[[0, 0, 255]; 2]);
    for (keys, after) in [
        (
            "a=T,f=24,s=2,v=1,i=7,C=1,q=2",
            Drawn {
                placed: vec![vec![BLUE; 2]],
                virtual7: None,
            },
        ),
        (
            "a=t,f=24,s=2,v=1,i=7,q=2",
            Drawn {
                placed: vec![],
                virtual7: None,
            },
        ),
    ] {
        let st = apc(keys, &blue);
        let mut bel = st[..st.len() - 2].to_vec();
        bel.push(0x07);
        for (end, retransmit) in [
            (ControlStringEnd::Standard, &st),
            (ControlStringEnd::Kitty, &st),
            (ControlStringEnd::Kitty, &bel),
        ] {
            split_everywhere(end, ROOMY, &old_image(), retransmit, &old_drawn(), &after);
        }
    }
}

/// **A chunked retransmission draws the old image until its last chunk's
/// terminator**: "terminals must not display anything, until the entire
/// sequence is received", so a frame between its chunks, or inside one,
/// still shows the old image and its placements.
#[test]
fn a_chunked_retransmission_draws_the_old_image_until_its_last_chunk_ends() {
    let mut s = apc(
        "a=T,f=24,s=2,v=1,i=7,C=1,q=2,m=1",
        &pixels(&[[0, 0, 255]; 1]),
    );
    s.extend(apc("m=0", &pixels(&[[0, 0, 255]; 1])));
    let after = Drawn {
        placed: vec![vec![BLUE; 2]],
        virtual7: None,
    };
    for end in [ControlStringEnd::Standard, ControlStringEnd::Kitty] {
        split_everywhere(end, ROOMY, &old_image(), &s, &old_drawn(), &after);
    }
}

/// **The control: a retransmission whose terminator is in the read that
/// began it** replaces the old image in that read, its two placements and
/// its virtual one with it, and answers OK.
#[test]
fn a_retransmission_in_one_read_replaces_the_old_image_in_that_read() {
    with(ROOMY, |t| {
        let mut r = Vec::new();
        feed(t, &old_image(), &mut r);
        assert_eq!(drawn(t), old_drawn(), "the premise");
        feed(
            t,
            &apc("a=T,f=24,s=2,v=1,i=7,C=1", &pixels(&[[0, 0, 255]; 2])),
            &mut r,
        );
        assert_eq!(String::from_utf8_lossy(&r), "\x1b_Gi=7;OK\x1b\\");
        assert_eq!(
            drawn(t),
            Drawn {
                placed: vec![vec![BLUE; 2]],
                virtual7: None,
            }
        );
    });
}

/// **A retransmission dropped past its cap replaces nothing**: a command
/// whose body passes [`KITTY_BODY_MAX`] is never read, so the old image
/// and its placements still draw after its terminator.
#[test]
fn a_retransmission_dropped_past_its_cap_replaces_nothing() {
    let keys = "a=T,f=24,s=2,v=1,i=7,C=1";
    let payload = vec![b'A'; KITTY_BODY_MAX];
    let s = apc(keys, &payload);
    with(ROOMY, |t| {
        let mut r = Vec::new();
        feed(t, &old_image(), &mut r);
        feed(t, &s, &mut r);
        assert_eq!(t.kitty().too_long, 1, "the premise: dropped");
        assert_eq!(r, b"", "a dropped command answers nothing");
        assert_eq!(drawn(t), old_drawn());
    });
}

/// **An arena that cannot hold the old image and the new one together
/// deletes the old one when the new one is reserved**, at its control
/// data, as it would delete an older image to make space for a new one:
/// the old image is not drawn between the reads, and no other image is
/// deleted in its place. An 8-pixel arena holds id 7, 4x1 and placed,
/// and id 9, 2x1 and unplaced; a 4x1 retransmission of id 7 fits only
/// once the old id 7 is gone, and id 9 is kept.
#[test]
fn an_arena_too_small_for_both_deletes_the_old_image_and_keeps_the_rest() {
    let mut prior = apc("a=T,f=24,s=4,v=1,i=7,C=1,q=2", &pixels(&[[255, 0, 0]; 4]));
    prior.extend(apc("a=t,f=24,s=2,v=1,i=9,q=2", &pixels(&[[0, 255, 0]; 2])));
    let s = apc("a=T,f=24,s=4,v=1,i=7,C=1,q=2", &pixels(&[[0, 0, 255]; 4]));
    let semi = s.iter().position(|&b| b == b';').expect("a ;");
    let after = Drawn {
        placed: vec![vec![BLUE; 4]],
        virtual7: None,
    };
    with(ImageBudget::of(8), |t| {
        let mut r = Vec::new();
        feed(t, &prior, &mut r);
        assert_eq!(
            drawn(t),
            Drawn {
                placed: vec![vec![RED; 4]],
                virtual7: None,
            },
            "the premise"
        );
        feed(t, &s[..=semi + 4], &mut r);
        assert_eq!(
            drawn(t),
            Drawn {
                placed: vec![],
                virtual7: None,
            },
            "the old image made room"
        );
        feed(t, &s[semi + 5..], &mut r);
        assert_eq!(drawn(t), after);
        let green = t.images().kitty_image(KittyId::of(9)).expect("id 9 kept");
        assert_eq!(
            t.images()
                .image(green)
                .map(|v| v.pixels.iter().map(|p| p.bits()).collect::<Vec<_>>()),
            Some(vec![GREEN; 2])
        );
        assert_eq!(t.images().images_evicted, 0, "nothing deleted for space");
    });
}

/// A 1x1 RGB PNG of `rgb`.
fn png(rgb: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, 1, 1);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()
        .and_then(|mut w| w.write_image_data(&rgb))
        .expect("a PNG");
    out
}

/// **A retransmission cancelled before it ends commits nothing**: the old
/// image and its placements stay, and the new image is never stored. The
/// cancel here is an inline file (`OSC 1337 ; File=`) taking the scratch
/// from a PNG still arriving in chunks, counted cancelled (`crate::file`).
/// A chunk the parser cancels inside its string, by default at a bare
/// `ESC`, reaches the stream the same way, as an upload abandoned before
/// its last terminator
/// (`a_retransmission_cancelled_by_a_bare_esc_keeps_the_old_image`,
/// `tests/control_string_end.rs`).
#[test]
fn a_retransmission_cancelled_before_it_ends_keeps_the_old_image() {
    let coded = b64(&png([0, 0, 255]));
    let mut first = b"\x1b_Ga=T,f=100,i=7,C=1,m=1;".to_vec();
    first.extend_from_slice(&coded[..8]);
    first.extend_from_slice(b"\x1b\\");
    let mut inline = b"\x1b]1337;File=inline=1:".to_vec();
    inline.extend(b64(&png([0, 255, 0])));
    inline.push(0x07);
    with(ROOMY, |t| {
        let mut r = Vec::new();
        feed(t, &old_image(), &mut r);
        feed(t, &first, &mut r);
        assert_eq!(drawn(t), old_drawn(), "between the chunks");
        feed(t, &inline, &mut r);
        assert_eq!(t.kitty().cancelled, 1, "the premise: cancelled");
        assert_eq!(drawn(t), old_drawn(), "after the cancel");
        // Id 7 still names the old image: a put places red.
        feed(t, &apc("a=p,i=7,p=3,C=1", b""), &mut r);
        assert_eq!(String::from_utf8_lossy(&r), "\x1b_Gi=7,p=3;OK\x1b\\");
        assert_eq!(drawn(t).placed, vec![vec![RED; 2]; 3]);
    });
}
