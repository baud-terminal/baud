//! **What mpv sent, read from its capture alone**: the oracle the replays
//! of `tests/recordings/kitty/mpv-logo-40x6.raw` are held to, parsed here
//! from the bytes with nothing of baud's, so a decoder or a parser baud
//! shares cannot agree with itself. `baud-app`'s film test and
//! `baud-present-macos/tests/cells.rs` both read it through this file.
//!
//! mpv 0.41.0's `--vo=kitty` sends a frame as `ESC [ 0 ; 10 f` and then
//! `ESC _ G a=T,f=24,s=…,v=…,C=1,q=2,m=1 ; <base64> ESC \`, the rest of
//! its RGB in `ESC _ G m=1 ; …` chunks and the last with `m=0`
//! (`[[source: kitty-graphics]]`, "RGB and RGBA data", "Display
//! images on screen"). Its status thread writes `ESC [ 1 K CR` and the
//! status line between them, and twice in this capture it did so inside
//! a chunk: the `ESC` ends the chunk with no `ST`, which cancels the
//! upload (`[[source: williams]]`: an `ESC` cancels a control string),
//! and what the chunk had left prints as text.

/// The capture.
pub const PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/kitty/mpv-logo-40x6.raw"
);

/// **A byte of the capture**, by its offset from the first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct At(pub usize);

/// **A count of pixels**: an upload's `s` or `v`, or a frame's width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pixels(pub usize);

/// **One pixel's red, green and blue**, as the capture's `f=24` and a
/// frame's ARGB word below its alpha hold them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub [u8; 3]);

/// One `a=T` upload and the chunks after it, to the one with `m=0`.
pub struct Upload {
    /// Just past the terminator of its last chunk: where a replay stops to
    /// look at it.
    pub end: At,
    /// Its `s` and `v`.
    pub size: (Pixels, Pixels),
    /// The RGB its chunks carried, three bytes a pixel; `None` for one a
    /// status line cut.
    pub rgb: Option<Vec<u8>>,
    /// The status line that cut it, if one did.
    pub cut: Option<Cut>,
}

/// **The text an `ESC` inside a chunk began**, as mpv wrote it:
/// `ESC [ 1 K`, `CR`, `ESC [ 0 m`, the status, `ESC [ 0 m`, and then the
/// rest of the chunk's base64, up to the `ESC \` that was the chunk's.
pub struct Cut {
    /// The `ESC` that cut the chunk.
    pub at: At,
    pub status: String,
    pub rest: String,
}

/// **A status line mpv wrote outside a chunk**: where it starts in the
/// capture, its text, and whether `ESC [ 1 K CR` came first, which only
/// the first one lacks -- that one is written where the cursor is.
pub struct Status {
    pub at: At,
    pub text: String,
    pub erased: bool,
}

/// **Where mpv puts every frame**: `CUP` to row 1, column 10, so the
/// cursor is at column 9 counted from 0 when the upload's `C=1` leaves it
/// there ([`COL`]).
pub const PLACE: &[u8] = b"\x1b[0;10f";

/// The cursor's column after [`PLACE`], from 0.
pub const COL: usize = 9;

fn find(hay: &[u8], needle: &[u8], from: At) -> Option<At> {
    hay.get(from.0..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| At(from.0 + i))
}

/// The value of `key` in a chunk's control data.
fn key<'c>(ctrl: &'c [u8], key: &[u8]) -> Option<&'c [u8]> {
    ctrl.split(|&b| b == b',').find_map(|kv| {
        let eq = kv.iter().position(|&b| b == b'=')?;
        (&kv[..eq] == key).then(|| &kv[eq + 1..])
    })
}

fn number(v: Option<&[u8]>) -> Pixels {
    Pixels(
        std::str::from_utf8(v.expect("the key"))
            .expect("ASCII")
            .parse()
            .expect("a number"),
    )
}

/// **A source pixel this bright must draw this bright**: luminance on
/// 0-255, Rec. ITU-R BT.709's weights. A picture drawn faint is the
/// symptom the check names.
pub const BRIGHT: f64 = 200.0;

impl Rgb {
    /// Its luminance on 0-255, BT.709's weights.
    pub fn luma(self) -> f64 {
        let [r, g, b] = self.0;
        0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)
    }
}

/// **The pixels of an upload that draw faint**: of the `w` by `h` region
/// at `(x0, y0)` of a frame `fw` wide, how many source pixels are
/// brighter than [`BRIGHT`], and how many of those draw at or below it.
pub fn faint(
    frame: &[u32],
    Pixels(fw): Pixels,
    (x0, y0): (usize, usize),
    (w, h): (usize, usize),
    rgb: &[u8],
) -> (usize, usize) {
    let (mut bright, mut faint) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let o = 3 * (y * w + x);
            if Rgb([rgb[o], rgb[o + 1], rgb[o + 2]]).luma() <= BRIGHT {
                continue;
            }
            bright += 1;
            let [_, r, g, b] = frame[(y0 + y) * fw + x0 + x].to_be_bytes();
            faint += usize::from(Rgb([r, g, b]).luma() <= BRIGHT);
        }
    }
    (bright, faint)
}

/// **RFC 4648 base64**, padding optional: written here, not taken from
/// the decoder under test.
pub fn base64(text: &[u8]) -> Vec<u8> {
    let six = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("{c:#x} is not base64"),
        }
    };
    let text: Vec<u8> = text.iter().copied().filter(|&c| c != b'=').collect();
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    for q in text.chunks(4) {
        let n = q
            .iter()
            .enumerate()
            .fold(0u32, |n, (k, &c)| n | six(c) << (18 - 6 * k));
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..q.len()]);
    }
    out
}

/// **The text an `ESC` at `esc` began**, split as [`Cut`] says.
fn cut(bytes: &[u8], esc: At) -> Cut {
    const LEAD: &[u8] = b"\x1b[1K\r\x1b[0m";
    assert_eq!(
        bytes.get(esc.0..esc.0 + LEAD.len()),
        Some(LEAD),
        "the premise: a status line cut the chunk at {}",
        esc.0
    );
    let from = esc.0 + LEAD.len();
    let At(status_end) = find(bytes, b"\x1b[0m", At(from)).expect("the status's end");
    let At(st) = find(bytes, b"\x1b\\", At(status_end)).expect("the chunk's own ST");
    let text = |r: &[u8]| String::from_utf8(r.to_vec()).expect("ASCII");
    let rest = &bytes[status_end + 4..st];
    assert!(
        !rest.contains(&0x1b),
        "the premise: nothing but base64 between the status and the ST"
    );
    Cut {
        at: esc,
        status: text(&bytes[from..status_end]),
        rest: text(rest),
    }
}

/// Every upload in the capture, in order.
pub fn uploads(bytes: &[u8]) -> Vec<Upload> {
    let mut out = Vec::new();
    let mut open: Option<(Upload, Vec<u8>)> = None;
    let mut from = 0;
    while let Some(At(at)) = find(bytes, b"\x1b_G", At(from)) {
        let At(esc) = find(bytes, b"\x1b", At(at + 3)).expect("every chunk ends");
        let body = &bytes[at + 3..esc];
        let terminated = bytes.get(esc + 1) == Some(&b'\\');
        let semi = body.iter().position(|&b| b == b';').unwrap_or(body.len());
        let (ctrl, payload) = (&body[..semi], body.get(semi + 1..).unwrap_or(&[]));
        if key(ctrl, b"a") == Some(b"T") {
            assert!(
                open.is_none(),
                "the premise: an upload ends before the next"
            );
            assert!(
                bytes[..at].ends_with(PLACE),
                "the premise: every frame is put at the cursor {PLACE:?} leaves"
            );
            let size = (number(key(ctrl, b"s")), number(key(ctrl, b"v")));
            let up = Upload {
                end: At(0),
                size,
                rgb: None,
                cut: None,
            };
            open = Some((up, Vec::new()));
        }
        if let Some((up, data)) = open.as_mut() {
            if up.cut.is_none() {
                data.extend_from_slice(payload);
            }
            if !terminated && up.cut.is_none() {
                up.cut = Some(cut(bytes, At(esc)));
            }
            if key(ctrl, b"m") != Some(b"1") {
                assert!(terminated, "the premise: an upload's last chunk has its ST");
                let (mut up, data) = open.take().expect("open");
                up.end = At(esc + 2);
                if up.cut.is_none() {
                    let rgb = base64(&data);
                    let (Pixels(s), Pixels(v)) = up.size;
                    assert_eq!(rgb.len(), 3 * s * v, "f=24's size");
                    up.rgb = Some(rgb);
                }
                out.push(up);
            }
        }
        from = esc + 1;
    }
    assert!(open.is_none(), "the premise: the last upload ended");
    out
}

/// **Every status line outside a chunk**, before `before`: each
/// `ESC [ 0 m V: … ESC [ 0 m` at the top level.
pub fn statuses(bytes: &[u8], before: At) -> Vec<Status> {
    const ERASED: &[u8] = b"\x1b[1K\r";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(At(at)) = find(bytes, b"\x1b[0mV: ", At(from)).filter(|&a| a < before) {
        let start = at + 4;
        let At(end) = find(bytes, b"\x1b[0m", At(start)).expect("the status's end");
        out.push(Status {
            at: At(at),
            text: String::from_utf8(bytes[start..end].to_vec()).expect("ASCII"),
            erased: at >= ERASED.len() && &bytes[at - ERASED.len()..at] == ERASED,
        });
        from = end;
    }
    out
}
