//! **A kitty graphics command ends where kitty ends it, and its payload
//! decodes as kitty decodes it.**
//!
//! kitty's graphics protocol is defined by kitty's behaviour, and these are
//! its error handling for its own protocol, taken for `APC G` alone:
//!
//! * the command ends only at `BEL` or `ESC \` (`find_st_terminator`,
//!   `[[source: kitty-vt-parser-c]]`), so an `ESC` that is not `ST`, a
//!   `CAN`, a `SUB`, a C0 control and `9C` are its payload, and nothing in
//!   it runs or prints;
//! * each chunk's payload decodes on its own and stops at its first byte
//!   that is no base64, keeping what it decoded (`base64_decode8`,
//!   `[[source: kitty-base64-h]]`; `[[source: libbase64-dec-tail-c]]`);
//! * a transmission that comes up short is refused, "ENODATA:Insufficient
//!   image data: <have> < <need>" (`[[source: kitty-graphics-c]]`).
//!
//! mpv's `--vo=kitty` writes its status line from another thread, and it
//! lands inside a chunk: `ESC [ 1 K CR ESC [ 0 m <status> ESC [ 0 m`, then
//! the chunk's remaining base64 and its `ESC \`. Read by
//! `[[source: williams]]`' rule, the `ESC` cancelled the command, and the
//! status and the base64 after it printed as a line of text under the
//! picture. Three levels, each fed whole and in every two-read split:
//! the chunk as mpv wrote it, the smallest stream with the same shape, and a
//! generator held to a model of kitty's two functions written out below.
use baud_core::*;
use baud_domain::{DecodeBudget, DecodeProgress, ImageBudget, ImagePx, KittyCounts, Row};

const WS: WinSize = WinSize {
    ws_row: 12,
    ws_col: 60,
};

/// Where every stream starts the cursor: away from the first column, so a
/// `CR` that ran would show.
const AT: &[u8] = b"\x1b[3;5H";

/// What a stream left: the replies, every row's text, the cursor, and the
/// pixels of every image placed.
#[derive(Debug, PartialEq, Eq)]
struct Left {
    replies: Vec<u8>,
    rows: Vec<String>,
    cursor: (usize, usize),
    images: Vec<Vec<u32>>,
    /// What the kitty stream counted: commands dropped past the cap and
    /// chunks cancelled among them.
    counts: KittyCounts,
}

/// `stream` fed in reads cut at `cuts`, every deferred decode run out.
fn run(stream: &[u8], cuts: &[usize]) -> Left {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    let mut replies = Vec::new();
    let mut from = 0;
    for &c in cuts.iter().chain(core::iter::once(&stream.len())) {
        let piece = &stream[from..c];
        from = c;
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
        replies.extend_from_slice(t.reply());
        t.clear_reply();
    }
    let rows = (0..usize::from(WS.ws_row))
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
        .collect();
    let (r, c) = t.cursor();
    let imgs = t.images();
    let images = imgs
        .placements()
        .filter_map(|(_, p)| imgs.image(p.image))
        .map(|v| v.pixels.iter().map(|p| p.bits()).collect())
        .collect();
    Left {
        replies,
        rows,
        cursor: (r.index(), c.index()),
        images,
        counts: t.kitty(),
    }
}

/// `stream` fed whole, and the same in every two-read split.
fn whole_and_split(stream: &[u8]) -> Left {
    let whole = run(stream, &[]);
    for k in 1..stream.len() {
        assert_eq!(run(stream, &[k]), whole, "split at {k}");
    }
    whole
}

/// Nothing written on the screen, the cursor where [`AT`] put it, and
/// nothing cancelled.
fn untouched(l: &Left) {
    assert_eq!(
        (l.counts.too_long, l.counts.cancelled),
        (0, 0),
        "dropped or cancelled"
    );
    assert!(
        l.rows.iter().all(String::is_empty),
        "text drawn: {:?}",
        l.rows
    );
    assert_eq!(l.cursor, (2, 4), "the cursor moved");
}

// ---- The reference model: kitty's two functions, transcribed. ----

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `find_st_terminator`: from `at`, the first `BEL`, or the first `\`
/// whose byte before it is `ESC`. Returns where the body ends and where the
/// stream goes on.
fn find_st_terminator(s: &[u8], at: usize) -> Option<(usize, usize)> {
    let mut i = at;
    while i < s.len() {
        match s[i] {
            0x07 => return Some((i, i + 1)),
            b'\\' if i > at && s[i - 1] == 0x1b => return Some((i - 1, i + 1)),
            _ => i += 1,
        }
    }
    None
}

/// libbase64's tail decode, as `base64_decode8` uses it: an octet as soon as
/// a character completes one, and a stop at the first byte outside the
/// table, `=` included, keeping what was written.
fn dec_tail(p: &[u8]) -> Vec<u8> {
    let (mut bits, mut n, mut out) = (0u32, 0u32, Vec::new());
    for &c in p {
        let Some(v) = ALPHABET.iter().position(|&a| a == c) else {
            break;
        };
        bits = (bits << 6) | u32::try_from(v).unwrap();
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push(u8::try_from((bits >> n) & 0xff).unwrap());
        }
    }
    out
}

/// kitty's outcome for a stream of [`AT`] and then `APC G` commands, one
/// `f=24` transmission of `w` by 1 pixels with id 1 and `q=0`, chunked: the
/// reply, and the pixels if it was stored.
fn kitty(stream: &[u8], w: usize) -> (Vec<u8>, Option<Vec<u32>>) {
    let mut at = AT.len();
    let mut data = Vec::new();
    while let Some(k) = stream[at..].windows(3).position(|x| x == b"\x1b_G") {
        let body = at + k + 2;
        let (end, next) = find_st_terminator(stream, body).expect("terminated");
        let b = &stream[body..end];
        let semi = b.iter().position(|&x| x == b';').unwrap_or(b.len());
        data.extend(dec_tail(b.get(semi + 1..).unwrap_or(&[])));
        at = next;
    }
    let need = 3 * w;
    if data.len() < need {
        let text = format!(
            "\x1b_Gi=1;ENODATA:Insufficient image data: {} < {need}\x1b\\",
            data.len()
        );
        return (text.into_bytes(), None);
    }
    let px = data[..need]
        .chunks(3)
        .map(|p| 0xff00_0000 | u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]))
        .collect();
    (b"\x1b_Gi=1;OK\x1b\\".to_vec(), Some(px))
}

/// RFC 4648 base64 of `data`, unpadded when it ends short.
fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let n = q
            .iter()
            .enumerate()
            .fold(0u32, |w, (i, &b)| w | u32::from(b) << (16 - 8 * i));
        for k in 0..=q.len() {
            out.push(ALPHABET[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]);
        }
    }
    out
}

/// **L1: the chunk mpv wrote with its status line inside**, in its framing
/// as it arrived, the pixels generated
/// (`tests/recordings/kitty/mpv-status-in-chunk.raw`), after a first chunk
/// that opens a 1000x1 `f=24` transmission and before a last that closes
/// it. The status line and the 764 base64 characters after it are gone
/// from the screen; the chunk's data ends at the status line's `ESC`, 2,499
/// octets, so the frame is refused short and the reply says by how much.
#[test]
fn mpvs_status_line_inside_a_chunk_is_payload_and_the_frame_comes_up_short() {
    let cut = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/recordings/kitty/mpv-status-in-chunk.raw"
    ))
    .expect("the chunk");
    assert_eq!(cut.len(), 4176, "the premise: the chunk as recorded");
    assert!(
        cut.windows(4).any(|w| w == b"\x1b[1K"),
        "the premise: the status line is inside"
    );
    let mut s = AT.to_vec();
    s.extend_from_slice(b"\x1b_Ga=T,f=24,s=1000,v=1,i=1,m=1;\x1b\\");
    s.extend_from_slice(&cut);
    s.extend_from_slice(b"\x1b_Gm=0;\x1b\\");
    let l = whole_and_split(&s);
    untouched(&l);
    assert_eq!(
        String::from_utf8_lossy(&l.replies),
        "\x1b_Gi=1;ENODATA:Insufficient image data: 2499 < 3000\x1b\\"
    );
    assert!(l.images.is_empty(), "a short frame drawn");
    assert_eq!(kitty(&s, 1000), (l.replies.clone(), None), "the model");
}

/// **L2: the smallest stream of that shape**: a 2x1 `f=24` command whose
/// payload is `AAAA`, then `ESC [ 1 K CR`, then `AAAA` and `ST`. The `ESC`
/// is payload and ends the data at three octets of the six, so the reply is
/// ENODATA with 3, and the `EL` and the `CR` never run.
#[test]
fn an_escape_sequence_inside_a_kitty_command_neither_runs_nor_prints() {
    let mut s = AT.to_vec();
    s.extend_from_slice(b"\x1b_Gf=24,s=2,v=1,i=1;AAAA\x1b[1K\rAAAA\x1b\\");
    let l = whole_and_split(&s);
    untouched(&l);
    assert_eq!(
        String::from_utf8_lossy(&l.replies),
        "\x1b_Gi=1;ENODATA:Insufficient image data: 3 < 6\x1b\\"
    );
    assert_eq!(kitty(&s, 2), (l.replies.clone(), None), "the model");
}

/// The bytes the generator writes into a payload: the shapes kitty keeps as
/// payload and Williams' parser does not -- a CSI, `CR`, `LF`, `CAN`,
/// `SUB`, `9C`, `DEL`, a GR byte, `ESC ESC`, `ESC DEL` and `ESC X` -- and
/// base64 of four, three, two and one characters, which leave a chunk's
/// last quantum short.
const SPLICE: [&[u8]; 15] = [
    b"\x1b[1K",
    b"\r",
    b"\n",
    b"\x18",
    b"\x1a",
    b"\x9c",
    b"\x7f",
    b"\xc1",
    b"\x1b\x1b",
    b"\x1b\x7f",
    b"\x1bX",
    b"text",
    b"ABC",
    b"AB",
    b"A",
];

proptest::proptest! {
    /// **L3: any chunked `f=24` transmission, anything of [`SPLICE`] written
    /// into its chunks, ended by `ESC \`, `BEL` or `ESC ESC \`, is what the
    /// model of kitty makes of it**: the same reply, the same pixels if
    /// stored, nothing on the screen and the cursor unmoved (`C=1`, so a
    /// stored frame's placement moves it neither), whole and in two reads
    /// cut anywhere. Chunks are of any length, so a chunk may end inside a
    /// quantum, whose whole octets kitty keeps and whose rest it drops,
    /// each chunk decoding on its own.
    #[test]
    fn any_kitty_stream_with_bytes_written_into_it_is_what_kitty_makes_of_it(
        w in 1usize..40,
        seed in 0u32..1000,
        chunk in 1usize..120,
        splices in proptest::collection::vec((0usize..10_000, 0usize..15), 0..4),
        ends in proptest::collection::vec(0u8..3, 1..40),
        cut in 0usize..10_000,
    ) {
        let data: Vec<u8> = (0..3 * w)
            .map(|i| u8::try_from((u32::try_from(i).unwrap() * 37 + seed) % 251).unwrap())
            .collect();
        let enc = b64(&data);
        let mut payloads: Vec<Vec<u8>> = enc.chunks(chunk).map(<[u8]>::to_vec).collect();
        for (at, k) in splices {
            let m = payloads.len();
            let p = &mut payloads[at % m];
            let at = at % (p.len() + 1);
            let tail = p.split_off(at);
            p.extend_from_slice(SPLICE[k]);
            p.extend_from_slice(&tail);
        }
        let mut s = AT.to_vec();
        let n = payloads.len();
        for (i, p) in payloads.into_iter().enumerate() {
            let keys = match (i, i + 1 == n) {
                (0, last) => format!("a=T,f=24,s={w},v=1,i=1,C=1,m={}", u8::from(!last)),
                (_, true) => "m=0".to_string(),
                _ => "m=1".to_string(),
            };
            s.extend_from_slice(b"\x1b_G");
            s.extend_from_slice(keys.as_bytes());
            s.push(b';');
            s.extend_from_slice(&p);
            s.extend_from_slice(match ends[i % ends.len()] {
                0 => &b"\x1b\\"[..],
                1 => b"\x07",
                _ => b"\x1b\x1b\\",
            });
        }
        let (reply, px) = kitty(&s, w);
        let whole = run(&s, &[]);
        proptest::prop_assert!(whole.rows.iter().all(String::is_empty), "{:?}", whole.rows);
        proptest::prop_assert_eq!(whole.cursor, (2, 4));
        proptest::prop_assert_eq!((whole.counts.too_long, whole.counts.cancelled), (0, 0));
        proptest::prop_assert_eq!(
            String::from_utf8_lossy(&whole.replies),
            String::from_utf8_lossy(&reply)
        );
        proptest::prop_assert_eq!(&whole.images, &px.into_iter().collect::<Vec<_>>());
        let cut = cut % (s.len() + 1);
        proptest::prop_assert_eq!(run(&s, &[cut]), whole, "cut at {}", cut);
    }
}

/// **A kitty command whose body passes 262,144 bytes is dropped, and what
/// follows up to its terminator is ignored**: kitty's
/// `MAX_ESCAPE_CODE_LENGTH`, a quarter of its 1 MiB read buffer
/// (`[[source: kitty-vt-parser-c]]`), counted from the `G`. A body of
/// exactly that many bytes is the command's, and answered; one byte more
/// and nothing is stored or said, the drop is counted once, and the text
/// after the `ST` is drawn where the cursor was. kitty returns to the
/// ground there instead, and draws the rest of the command as text, and
/// only when a read ends without a terminator; baud keeps ignoring it up to
/// the terminator, at every split. A raw continuation chunk past the cap
/// is ignored and its upload goes on, kitty's result: a 2x1 picture's
/// first chunk, a chunk of 262,144 `A`s, and a last chunk store the first
/// and last chunks' pixels, as if the middle one had never come. So is a
/// PNG's, whose bytes roll back too: a 1x1 PNG's first 12 bytes, the chunk
/// of `A`s, whose zeros take it past 24 bytes to a header that is refused,
/// and the rest, store the PNG's pixel. An `o=z` continuation drops its
/// upload whole, the inflate's state not rolling back, where kitty ignores
/// that chunk alone: a stated difference.
#[test]
fn a_kitty_command_past_its_cap_is_dropped_up_to_its_terminator() {
    use baud_domain::KITTY_BODY_MAX;
    let keys = b"a=T,f=24,s=1,v=1,i=1,C=1;";
    let single = |extra: usize, end: &[u8]| {
        let mut s = AT.to_vec();
        s.extend_from_slice(b"\x1b_G");
        s.extend_from_slice(keys);
        s.extend(std::iter::repeat_n(
            b'A',
            KITTY_BODY_MAX - 1 - keys.len() + extra,
        ));
        s.extend_from_slice(end);
        s.push(b'X');
        s
    };
    let middle = |first: &[u8], last: &[u8]| {
        let mut s = AT.to_vec();
        s.extend_from_slice(first);
        s.extend_from_slice(b"\x1b_Gm=1;");
        s.extend(std::iter::repeat_n(b'A', KITTY_BODY_MAX));
        s.extend_from_slice(b"\x1b\\");
        s.extend_from_slice(last);
        s.push(b'X');
        s
    };
    // `AQID` and `BAUG` are 01 02 03 and 04 05 06.
    let cont = middle(
        b"\x1b_Ga=T,f=24,s=2,v=1,i=1,C=1,m=1;AQID\x1b\\",
        b"\x1b_Gm=0;BAUG\x1b\\",
    );
    // A stored zlib block of 01 02 03 and its Adler-32, in two chunks.
    let mut zlib = b"\x1b_Ga=T,f=24,o=z,s=1,v=1,i=1,C=1,m=1;".to_vec();
    zlib.extend(b64(&[0x78, 0x01, 0x01, 0x03, 0x00, 0xfc, 0xff]));
    zlib.extend_from_slice(b"\x1b\\");
    let mut zlast = b"\x1b_Gm=0;".to_vec();
    zlast.extend(b64(&[1, 2, 3, 0x00, 0x0d, 0x00, 0x07]));
    zlast.extend_from_slice(b"\x1b\\");
    let zcont = middle(&zlib, &zlast);
    let png = {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, 1, 1);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.write_header()
            .and_then(|mut w| w.write_image_data(&[1, 2, 3]))
            .expect("a PNG");
        out
    };
    let mut pfirst = b"\x1b_Ga=T,f=100,i=1,C=1,m=1;".to_vec();
    pfirst.extend(b64(&png[..12]));
    pfirst.extend_from_slice(b"\x1b\\");
    let mut plast = b"\x1b_Gm=0;".to_vec();
    plast.extend(b64(&png[12..]));
    plast.extend_from_slice(b"\x1b\\");
    let pcont = middle(&pfirst, &plast);
    let ok = "\x1b_Gi=1;OK\x1b\\";
    for (name, s, reply, images, dropped) in [
        ("at the cap, ST", single(0, b"\x1b\\"), ok, 1, 0),
        ("at the cap, BEL", single(0, b"\x07"), ok, 1, 0),
        ("past it, ST", single(1, b"\x1b\\"), "", 0, 1),
        ("past it, BEL", single(1, b"\x07"), "", 0, 1),
        (
            "past it by a read's worth, ST",
            single(4096, b"\x1b\\"),
            "",
            0,
            1,
        ),
        ("a raw continuation past it", cont, ok, 1, 1),
        ("a PNG continuation past it", pcont, ok, 1, 1),
        ("an o=z continuation past it", zcont, "", 0, 1),
    ] {
        let whole = run(&s, &[]);
        assert_eq!(String::from_utf8_lossy(&whole.replies), reply, "{name}");
        assert_eq!(whole.images.len(), images, "{name}");
        assert_eq!(whole.counts.too_long, dropped, "{name}");
        assert_eq!(whole.rows[2], "    X", "{name}: the text after it");
        let pixels = match name {
            "a raw continuation past it" => vec![vec![0xff01_0203, 0xff04_0506]],
            "a PNG continuation past it" => vec![vec![0xff01_0203]],
            _ => whole.images.clone(),
        };
        assert_eq!(
            whole.images, pixels,
            "{name}: the first and last chunks' pixels"
        );
        let n = s.len();
        for cut in [1, 40, n / 2, n - 4100, n - 6, n - 3, n - 2, n - 1] {
            assert_eq!(run(&s, &[cut]), whole, "{name}: cut at {cut}");
        }
    }
}

/// **mpv 0.41.0's exit leaves the terminal where kitty leaves it**: its
/// last write is `ESC _ G a = d ;` with no terminator
/// (`tests/recordings/kitty/mpv-logo-40x6.raw`), so the `?25h`, `?1003l`,
/// `?1049l` and `ESC >` after it are that command's payload, as kitty
/// keeps a string open until its `BEL` or `ESC \`
/// (accumulate_st_terminated_esc_code, `[[source: kitty-vt-parser-c]]`).
/// kitty 0.41.1, fed the capture headless, answers no DECRQM while the
/// string is open and, after a later `BEL` or `ESC \`, reports `?1049` and
/// `?1003` both still set, on the alternate screen; so does baud, whole
/// and in the tty's 1,024-byte reads. mpv 8e566099cb ends that `a=d` with
/// `ESC \`.
#[test]
fn mpvs_unterminated_exit_leaves_the_modes_set_as_kitty_does() {
    let capture = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/recordings/kitty/mpv-logo-40x6.raw"
    ))
    .expect("the capture");
    assert!(
        capture.ends_with(b"\x1b_Ga=d;\x1b[?25h\x1b[?1003l\x1b[?1049l\x1b>"),
        "the premise: the quit's a=d has no terminator"
    );
    let ask = b"\x1b[?1049$p\x1b[?1003$p";
    for end in [&b"\x1b\\"[..], b"\x07"] {
        let mut s = capture.clone();
        s.extend_from_slice(ask);
        s.extend_from_slice(end);
        s.extend_from_slice(ask);
        for read in [s.len(), 1024] {
            let ws = WinSize {
                ws_row: 6,
                ws_col: 40,
            };
            let mut owned = Owned::new(&ws).with_images(ImageBudget::of(1 << 20));
            let mut t = Term::new(owned.storage(), &ws);
            let mut replies = Vec::new();
            for piece in s.chunks(read) {
                t.write_at(BytePos::ZERO, piece);
                t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
                while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
                replies.extend_from_slice(t.reply());
                t.clear_reply();
            }
            assert_eq!(
                String::from_utf8_lossy(&replies),
                "\x1b[?1049;1$y\x1b[?1003;1$y",
                "{end:?}, reads of {read}: only the queries after it answered, both set"
            );
            assert_eq!(t.screen(), Screen::Alternate, "{end:?}, reads of {read}");
        }
    }
}
