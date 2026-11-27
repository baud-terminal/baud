//! **A file medium read off the parse** (`Term::hold_kitty_files`): the
//! terminator of a `t=f`, `t=t` or `t=s` command stops the read with the
//! command owed, the embedder hands the bytes and the read's end over, and
//! the rest is fed after the reply -- leaving what a lent reader read at
//! the terminator leaves, at every split of the bytes.

use baud_core::*;
use baud_domain::{
    DecodeBudget, DecodeProgress, ImageBudget, ImagePx, KittyFileFailed, KittyFileOwed,
    KittyFileRequest, KittyFiles, KittyFlow,
};
use core::num::NonZeroUsize;
use std::io::Write as _;

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

/// The single error, for image 1.
const FILE_ERR: &[u8] = b"\x1b_Gi=1;EBADF:Failed to read image file\x1b\\";

/// The `OK` for image 1.
const OK: &[u8] = b"\x1b_Gi=1;OK\x1b\\";

/// RFC 4648 base64, padded.
fn b64(data: &[u8]) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            match k <= q.len() {
                true => out.push(A[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => out.push(b'='),
            }
        }
    }
    out
}

/// An `APC G` with `ctrl` and `payload` in base64.
fn apc(ctrl: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    v.push(b';');
    v.extend_from_slice(&b64(payload));
    v.extend_from_slice(b"\x1b\\");
    v
}

/// **A reader over one named byte string**, read at the terminator as the
/// drivers' lent reader is: the length by the request's own rule.
struct One {
    name: Vec<u8>,
    data: Option<Vec<u8>>,
}

impl KittyFiles for One {
    fn read(
        &mut self,
        req: &KittyFileRequest<'_>,
        sink: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), KittyFileFailed> {
        let data = match &self.data {
            Some(d) if req.path == self.name.as_slice() => d,
            _ => return Err(KittyFileFailed),
        };
        let _ = sink(&bytes_for(req, data));
        Ok(())
    }
}

/// The bytes a request reads of `data`: from `O`, as many as the request's
/// own rule allows.
fn bytes_for(req: &KittyFileRequest<'_>, data: &[u8]) -> Vec<u8> {
    let len = u64::try_from(data.len()).unwrap();
    let n = req.read_len(req.available(len)).unwrap_or(0);
    let from = usize::try_from(req.offset).unwrap().min(data.len());
    data[from..from + n].to_vec()
}

/// A terminal with a 10x20 cell and room for images.
fn term(owned: &mut Owned) -> Term<'_> {
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    t
}

/// Feed `bytes` as one read; the replies it left.
fn feed(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

/// Every deferred decode run to its end, its replies kept.
fn settle(t: &mut Term<'_>, replies: &mut Vec<u8>) {
    while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
    replies.extend_from_slice(t.reply());
    t.clear_reply();
}

/// The replies, the cursor, each placement with its image's size and
/// pixels, and the screen's text.
fn state(t: &Term<'_>, replies: &[u8]) -> String {
    let mut s = format!("{}\n{:?}\n", String::from_utf8_lossy(replies), t.cursor());
    let imgs = t.images();
    for (_, p) in imgs.placements() {
        s.push_str(&format!("placed {:?} {:?}\n", p.line, p.col));
        match imgs.image(p.image) {
            Some(v) => {
                let px: Vec<u32> = v.pixels.iter().map(|p| p.bits()).collect();
                s.push_str(&format!("{}x{} {px:?}\n", v.width.raw(), v.height.raw()));
            }
            None => s.push_str("decoding\n"),
        }
    }
    for r in 0..usize::from(WS.ws_row) {
        let row = t.row(Row::from_index(r)).expect("row");
        let text: String = row
            .iter()
            .filter_map(|c| c.content.as_codepoint())
            .filter_map(|c| char::from_u32(c.value()))
            .collect();
        s.push_str(text.trim_end());
        s.push('\n');
    }
    s
}

/// **What a lent reader leaves**: `before` fed, `between` run on the
/// terminal, then `cmd` and `after` in one read, the file read at the
/// terminator.
fn lent(
    before: &[u8],
    cmd: &[u8],
    after: &[u8],
    name: &[u8],
    data: Option<&[u8]>,
    between: impl FnOnce(&mut Term<'_>),
) -> String {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = term(&mut owned);
    t.set_kitty_files(Box::leak(Box::new(One {
        name: name.to_vec(),
        data: data.map(<[u8]>::to_vec),
    })));
    let mut r = feed(&mut t, before);
    between(&mut t);
    r.extend(feed(&mut t, &[cmd, after].concat()));
    settle(&mut t, &mut r);
    state(&t, &r)
}

/// **What the hold leaves**: `before` fed, then `cmd` and `after` in one
/// read, which stops at the terminator with nothing replied and `after`
/// unparsed; then `between` runs on the terminal, the bytes go over
/// `piece` at a time and the read ends, and the rest is fed.
fn held(
    before: &[u8],
    cmd: &[u8],
    after: &[u8],
    data: Option<&[u8]>,
    piece: NonZeroUsize,
    between: impl FnOnce(&mut Term<'_>),
) -> String {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = term(&mut owned);
    t.hold_kitty_files();
    let mut r = feed(&mut t, before);
    let read = [cmd, after].concat();
    let first = feed(&mut t, &read);
    assert_eq!(first, b"", "nothing is replied before the read ends");
    assert_eq!(t.kitty_file_owed(), KittyFileOwed::Yes);
    let rest = t.take_held().expect("the read stops at the terminator");
    assert_eq!(rest.len.index(), after.len(), "the rest is what follows");
    between(&mut t);
    let q = t.kitty_file_asked().expect("the read is asked");
    let req = KittyFileRequest {
        medium: q.medium,
        path: b"",
        offset: q.offset,
        size: q.size,
        limit: q.limit,
        past: q.past,
    };
    match data {
        Some(d) => {
            for p in bytes_for(&req, d).chunks(piece.get()) {
                if t.kitty_file_piece(p) == KittyFlow::Stop {
                    break;
                }
            }
            t.kitty_file_end(Ok(()));
        }
        None => t.kitty_file_end(Err(KittyFileFailed)),
    }
    assert_eq!(t.kitty_file_owed(), KittyFileOwed::No);
    assert_eq!(t.kitty_file_asked(), None);
    r.extend_from_slice(t.reply());
    t.clear_reply();
    t.feed(rest.at, rest.len);
    r.extend_from_slice(t.reply());
    t.clear_reply();
    settle(&mut t, &mut r);
    state(&t, &r)
}

/// **The hold leaves what a lent reader leaves**, at every split of the
/// bytes: raw pixels placed and the text after them on the right of the
/// image, raw pixels the file leaves short, a missing file, and a query.
/// The text after the command is parsed only after the reply.
#[test]
fn a_held_file_read_leaves_what_a_lent_reader_leaves_at_every_split() {
    let rgba: Vec<u8> = (0..3 * 2 * 4)
        .map(|i| u8::try_from(i * 9).unwrap())
        .collect();
    let name: &[u8] = b"/pics/a.raw";
    let cases: [(&str, Option<&[u8]>); 4] = [
        ("a=T,i=1,t=f,f=32,s=3,v=2", Some(&rgba)),
        ("a=T,i=1,t=f,f=32,s=3,v=2", Some(&rgba[..5])),
        ("a=T,i=1,t=t,f=32,s=3,v=2", None),
        ("a=q,i=1,t=f,f=32,s=3,v=2", Some(&rgba)),
    ];
    for (ctrl, data) in cases {
        let cmd = apc(ctrl, name);
        let want = lent(b"ab", &cmd, b"XY", name, data, |_| {});
        for piece in [1, 7, 4096] {
            let piece = NonZeroUsize::new(piece).unwrap();
            let got = held(b"ab", &cmd, b"XY", data, piece, |_| {});
            assert_eq!(got, want, "{ctrl} {:?} by {piece}", data.map(<[u8]>::len));
        }
    }
    let seven = NonZeroUsize::new(7).unwrap();
    let ok = held(
        b"",
        &apc("a=T,i=1,t=f,f=32,s=3,v=2", name),
        b"",
        Some(&rgba),
        seven,
        |_| {},
    );
    assert!(ok.starts_with(&*String::from_utf8_lossy(OK)), "{ok}");
    let failed = held(
        b"",
        &apc("a=T,i=1,t=f,f=32,s=3,v=2", name),
        b"",
        None,
        seven,
        |_| {},
    );
    assert!(
        failed.starts_with(&*String::from_utf8_lossy(FILE_ERR)),
        "{failed}"
    );
}

/// **A command fed before the read ends gives the owed read up**: the one
/// file error goes first, as a failed read answers, then the command's
/// own reply, so no command is left unanswered.
#[test]
fn a_command_before_the_read_ends_gives_it_up_with_the_one_error() {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = term(&mut owned);
    t.hold_kitty_files();
    let first = feed(&mut t, &apc("a=T,i=1,t=f,f=32,s=3,v=2", b"/pics/a.raw"));
    assert_eq!(first, b"");
    assert!(t.take_held().is_some());
    let next = feed(&mut t, &apc("a=T,i=2,f=32,s=1,v=1", &[1, 2, 3, 4]));
    assert_eq!(next, [FILE_ERR, b"\x1b_Gi=2;OK\x1b\\"].concat());
    assert_eq!(t.kitty_file_owed(), KittyFileOwed::No);
    t.kitty_file_end(Ok(()));
    assert_eq!(t.reply(), b"", "an end with nothing owed does nothing");
}

/// **A resize while the read is owed places the image at the cursor the
/// resize left**, as a command parsed after the resize places it: the
/// same text, resized before the command reaches a lent reader, leaves
/// the same screen.
#[test]
fn a_resize_while_the_read_is_owed_places_at_the_cursor_the_resize_left() {
    let rgba: Vec<u8> = (0..3 * 2 * 4)
        .map(|i| u8::try_from(i * 5).unwrap())
        .collect();
    let name: &[u8] = b"/pics/a.raw";
    let cmd = apc("a=T,i=1,t=f,f=32,s=3,v=2", name);
    let text = vec![b'a'; 70];
    let narrow = WinSize {
        ws_row: 24,
        ws_col: 40,
    };
    let seven = NonZeroUsize::new(7).unwrap();
    let got = held(&text, &cmd, b"Z", Some(&rgba), seven, |t| {
        assert_eq!(t.resize(&narrow), Resized::Ok);
    });
    let want = {
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
        let mut t = term(&mut owned);
        t.set_kitty_files(Box::leak(Box::new(One {
            name: name.to_vec(),
            data: Some(rgba.clone()),
        })));
        let mut r = feed(&mut t, &text);
        assert_eq!(t.resize(&narrow), Resized::Ok);
        r.extend(feed(&mut t, &[cmd.as_slice(), b"Z"].concat()));
        state(&t, &r)
    };
    assert_eq!(got, want);
}

/// A zlib stream of `data` in three parts: stored, then the strongest
/// level, then the fastest, each part ended by a sync flush -- so stored,
/// fixed or dynamic blocks and the empty stored block a sync flush writes
/// all occur in one stream.
fn zlib_mixed(data: &[u8]) -> Vec<u8> {
    use miniz_oxide::deflate::core::{CompressorOxide, TDEFLFlush, TDEFLStatus, compress};
    let mut c = CompressorOxide::default();
    c.set_format_and_level(miniz_oxide::DataFormat::Zlib, 0);
    let third = data.len() / 3;
    let parts = [
        (&data[..third], 0u8, TDEFLFlush::Sync),
        (&data[third..2 * third], 9, TDEFLFlush::Sync),
        (&data[2 * third..], 1, TDEFLFlush::Finish),
    ];
    let mut out = Vec::new();
    for (part, level, flush) in parts {
        c.set_compression_level_raw(level);
        let mut input = part;
        loop {
            let mut buf = vec![0u8; 1 << 16];
            let (status, read, wrote) = compress(&mut c, input, &mut buf, flush);
            out.extend_from_slice(&buf[..wrote]);
            input = &input[read..];
            if status == TDEFLStatus::Done || (input.is_empty() && status == TDEFLStatus::Okay) {
                break;
            }
            assert!(status != TDEFLStatus::BadParam, "compress");
        }
    }
    out
}

/// A PNG of 8-bit `data` in `color`, its stream cut into `IDAT` chunks of
/// `idat` bytes.
fn png(w: u32, h: u32, color: png::ColorType, data: &[u8], idat: usize) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(color);
        e.set_depth(png::BitDepth::Eight);
        // Every filter a row can carry, so the unfiltering is exercised
        // and not only `Sub`, the crate's default.
        e.set_filter(png::Filter::Adaptive);
        let mut wr = e.write_header().unwrap();
        let mut sw = wr.stream_writer_with_size(idat).unwrap();
        sw.write_all(data).unwrap();
        sw.finish().unwrap();
    }
    out
}

/// Deterministic bytes, a mix of runs and noise so zlib makes every block
/// kind.
fn bytes(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2654435761).max(1);
    (0..n)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            match (i / 37) % 3 {
                0 => u8::try_from(i % 7).unwrap(),
                _ => x.to_le_bytes()[0],
            }
        })
        .collect()
}

/// **Every format, and the bytes `O` and `S` choose, held, leave what a
/// lent reader leaves**, at every split: RGB, a PNG, RGB and a PNG through
/// zlib, `O` and `S` picking the middle of a file, and a zlib stream that
/// does not inflate, refused.
#[test]
fn every_format_held_leaves_what_a_lent_reader_leaves() {
    let rgb = bytes(5 * 3 * 3, 81);
    let p = png(6, 4, png::ColorType::Rgba, &bytes(6 * 4 * 4, 83), 37);
    let zrgb = zlib_mixed(&rgb);
    let zpng = zlib_mixed(&p);
    let padded = [&[9u8; 10][..], &rgb, &[9u8; 7]].concat();
    // Within the bound `o=z` reads to, so every byte is handed over.
    let broken = bytes(40, 5);
    let cases: [(String, &[u8]); 6] = [
        ("f=24,s=5,v=3".to_string(), &rgb),
        ("f=100".to_string(), &p),
        ("f=24,s=5,v=3,o=z".to_string(), &zrgb),
        (format!("f=100,o=z,S={}", p.len()), &zpng),
        (format!("f=24,s=5,v=3,O=10,S={}", rgb.len()), &padded),
        ("f=24,s=5,v=3,o=z".to_string(), &broken),
    ];
    let name: &[u8] = b"/pics/a";
    for (keys, data) in &cases {
        let cmd = apc(&format!("a=T,i=1,t=f,{keys}"), name);
        let want = lent(b"ab", &cmd, b"XY", name, Some(data), |_| {});
        for piece in [1, 7, 4096] {
            let piece = NonZeroUsize::new(piece).unwrap();
            let got = held(b"ab", &cmd, b"XY", Some(data), piece, |_| {});
            assert_eq!(got, want, "{keys} by {piece}");
        }
    }
}

/// **A cell size set while the read is owed sizes the placement the end
/// makes**, as a command parsed after it is sized: the cursor moves past
/// the cells the image covers at the new size.
#[test]
fn a_cell_size_set_while_owed_sizes_the_placement_the_end_makes() {
    let rgba = bytes(30 * 50 * 4, 7);
    let name: &[u8] = b"/pics/big";
    let cmd = apc("a=T,i=1,t=f,f=32,s=30,v=50", name);
    let small = |t: &mut Term<'_>| t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
    let want = lent(b"ab", &cmd, b"Z", name, Some(&rgba), small);
    let unchanged = lent(b"ab", &cmd, b"Z", name, Some(&rgba), |_| {});
    assert_ne!(want, unchanged, "the cell size moves the cursor");
    let seven = NonZeroUsize::new(7).unwrap();
    let got = held(b"ab", &cmd, b"Z", Some(&rgba), seven, small);
    assert_eq!(got, want);
}

/// **A re-transmission over a live placement keeps the old image drawn
/// while its read is owed**: the transmission concludes at the read, so
/// until then the old image and its placement stay, and the end replaces
/// them with the new image as a lent reader's read does.
#[test]
fn a_retransmission_held_keeps_the_old_placement_while_owed() {
    let old = apc("a=T,i=1,f=32,s=3,v=2", &bytes(3 * 2 * 4, 1));
    let new = bytes(3 * 2 * 4, 2);
    let name: &[u8] = b"/pics/new";
    let cmd = apc("a=T,i=1,t=f,f=32,s=3,v=2", name);
    let want = lent(&old, &cmd, b"Z", name, Some(&new), |_| {});
    let seven = NonZeroUsize::new(7).unwrap();
    let got = held(&old, &cmd, b"Z", Some(&new), seven, |t| {
        // The new image is reserved and not whole, so the one whole image
        // placed is the old one.
        let imgs = t.images();
        let whole = imgs
            .placements()
            .filter(|(_, p)| imgs.image(p.image).is_some())
            .count();
        assert_eq!(whole, 1, "the old placement stays drawn");
    });
    assert_eq!(got, want);
}

/// **`RIS` keeps the hold**: a file medium after a reset still stops the
/// read with its command owed, and a failed read answers the one error.
#[test]
fn ris_keeps_the_hold() {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = term(&mut owned);
    t.hold_kitty_files();
    assert_eq!(feed(&mut t, b"\x1bc"), b"");
    let first = feed(&mut t, &apc("a=T,i=1,t=f,f=32,s=3,v=2", b"/pics/a.raw"));
    assert_eq!(first, b"");
    assert_eq!(t.kitty_file_owed(), KittyFileOwed::Yes);
    assert!(t.take_held().is_some());
    t.kitty_file_end(Err(KittyFileFailed));
    assert_eq!(t.reply(), FILE_ERR);
    assert_eq!(t.kitty_file_owed(), KittyFileOwed::No);
}
