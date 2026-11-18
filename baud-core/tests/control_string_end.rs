//! **Where an `APC G` ends, under each [`ControlStringEnd`], on the same
//! byte streams.**
//!
//! `Standard`, the default, is the standard's rule for every control
//! string: only `ESC \` ends it, and a bare `ESC` -- one before any byte but
//! `\` -- cancels it, that byte beginning an escape sequence
//! (`[[source: williams]]`, "receiving the ESC character will “cancel” the
//! control string"; `[[source: ecma48]]` 8.3.2, "The control string is
//! closed by the terminating delimiter STRING TERMINATOR (ST)"). `CAN`,
//! `SUB` and a C1 control cancel it too, and a `BEL` is ignored, as the
//! `sos/pm/apc string` state ignores `07`. A cancelled chunk runs nothing
//! and its upload is dropped, counted cancelled, so a retransmission it was
//! part of replaces no image.
//!
//! `Kitty` ends an `APC G` only at `BEL` or `ESC \`, every other byte its
//! payload, and nothing inside it runs or prints.
//!
//! Each stream is fed whole and in every two-read split, and each mode
//! must leave the same terminal at every split.
use baud_core::*;
use baud_domain::{
    ControlStringEnd, DecodeBudget, DecodeProgress, ImageBudget, ImageOwner, ImagePx, KittyCounts,
    KittyId, Row,
};

const MODES: [ControlStringEnd; 2] = [ControlStringEnd::Standard, ControlStringEnd::Kitty];

/// Where every stream starts the cursor: away from the first column, so a
/// `CR` that ran would show.
const AT: &[u8] = b"\x1b[3;5H";

/// What a stream left: the replies, every row's text, the cursor, the
/// screen, the pixels of every kitty image stored, by id, and the kitty graphics
/// stream's counts.
#[derive(Debug, PartialEq, Eq)]
struct Left {
    replies: String,
    rows: Vec<String>,
    cursor: (usize, usize),
    screen: Screen,
    stored: Vec<(u32, Vec<u32>)>,
    counts: KittyCounts,
}

/// `stream` fed under `mode` in reads cut at `cuts`, on a `ws` screen,
/// every deferred decode run out.
fn run(mode: ControlStringEnd, ws: &WinSize, stream: &[u8], cuts: &[usize]) -> Left {
    let mut owned = Owned::new(ws).with_images(ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), ws);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    t.set_control_string_end(mode);
    assert_eq!(t.control_string_end(), mode, "the setting reads back");
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
    let rows = (0..usize::from(ws.ws_row))
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
    let mut stored: Vec<(u32, Vec<u32>)> = (1..=9)
        .filter_map(|id| {
            let image = imgs.kitty_image(KittyId::of(id))?;
            matches!(imgs.owner(image), Some(ImageOwner::Kitty(..))).then_some(())?;
            let v = imgs.image(image)?;
            Some((id, v.pixels.iter().map(|p| p.bits()).collect()))
        })
        .collect();
    stored.sort();
    Left {
        replies: String::from_utf8_lossy(&replies).into_owned(),
        rows,
        cursor: (r.index(), c.index()),
        screen: t.screen(),
        stored,
        counts: t.kitty(),
    }
}

const WS: WinSize = WinSize {
    ws_row: 12,
    ws_col: 60,
};

/// `stream` under `mode`, fed whole and in every two-read split, which
/// must all leave the same.
fn whole_and_split(mode: ControlStringEnd, ws: &WinSize, stream: &[u8]) -> Left {
    let whole = run(mode, ws, stream, &[]);
    for k in 1..stream.len() {
        assert_eq!(run(mode, ws, stream, &[k]), whole, "{mode:?}, split at {k}");
    }
    whole
}

/// A 1x1 RGB image, `f=24`, red: `FF 00 00`.
const RED_B64: &[u8] = b"/wAA";
const RED: u32 = 0xffff_0000;
const BLUE_B64: &[u8] = b"AAD/";
const BLUE: u32 = 0xff00_00ff;

/// **1. An `APC G` cut by a bare `ESC`**: a whole 1x1 transmission, then
/// `ESC [ 5 ; 7 H` before its `ST`. By default the `ESC` cancels the
/// command: no image is stored, the cancel is counted, and the `CUP` runs.
/// Under the setting the command goes on: the `CUP` is payload, which the
/// decode stops at, the command ends at `ST` with its pixels whole, and
/// the cursor stays where it was.
#[test]
fn an_apc_g_cut_by_a_bare_esc_is_cancelled_by_default_and_goes_on_under_the_setting() {
    let mut s = AT.to_vec();
    s.extend_from_slice(b"\x1b_Ga=t,f=24,s=1,v=1,i=1;");
    s.extend_from_slice(RED_B64);
    s.extend_from_slice(b"\x1b[5;7H\x1b\\");
    let standard = whole_and_split(ControlStringEnd::Standard, &WS, &s);
    assert_eq!(standard.stored, vec![], "the cancelled command stored");
    assert_eq!(
        (standard.counts.cancelled, standard.counts.commands),
        (1, 0),
        "cancelled, and no command run"
    );
    assert_eq!(standard.cursor, (4, 6), "the CUP after the ESC ran");
    assert_eq!(standard.replies, "", "a cancelled command answers nothing");
    assert!(standard.rows.iter().all(String::is_empty), "text drawn");

    let kitty = whole_and_split(ControlStringEnd::Kitty, &WS, &s);
    assert_eq!(kitty.stored, vec![(1, vec![RED])], "the command went on");
    assert_eq!(
        (kitty.counts.cancelled, kitty.counts.commands),
        (0, 1),
        "nothing cancelled, the command run"
    );
    assert_eq!(kitty.cursor, (2, 4), "the CUP is payload");
    assert_eq!(kitty.replies, "\x1b_Gi=1;OK\x1b\\");
}

/// **2. mpv 0.41's exit**, from the capture: its last write is `ESC _ G a
/// = d ;` with no terminator, then `ESC [ ? 25 h`, `ESC [ ? 1003 l` and
/// `ESC [ ? 1049 l`. By default the `ESC` of `?25h` cancels the `a=d`, and
/// the three restore the screen: the primary is up, any-event mouse
/// tracking is off and the cursor shows, as DECRQM reads them straight
/// after. Under the setting the three are the command's payload, and the
/// queries after them are too, until an `ST` ends it: then the alternate
/// screen is still up and `?1003` set. The cursor shows under both, the
/// capture having shown it before its exit.
#[test]
fn mpvs_exit_restores_the_screen_by_default_and_not_under_the_setting() {
    let capture = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/recordings/kitty/mpv-logo-40x6.raw"
    ))
    .expect("the capture");
    let exit = b"\x1b_Ga=d;\x1b[?25h\x1b[?1003l\x1b[?1049l\x1b>";
    assert!(capture.ends_with(exit), "the premise: the exit as captured");
    let ask = b"\x1b[?1049$p\x1b[?1003$p\x1b[?25$p";
    let ws = WinSize {
        ws_row: 6,
        ws_col: 40,
    };
    // The premise: before the exit, the alternate screen is up and
    // `?1003` set, and the cursor shows.
    let before = run(
        ControlStringEnd::Standard,
        &ws,
        &[&capture[..capture.len() - exit.len()], ask].concat(),
        &[],
    );
    assert!(
        before
            .replies
            .ends_with("\x1b[?1049;1$y\x1b[?1003;1$y\x1b[?25;1$y"),
        "the premise: {:?}",
        before.replies
    );
    let s = [&capture[..], ask, b"\x1b\\", ask].concat();
    for read in [s.len(), 1024] {
        let cuts: Vec<usize> = (read..s.len()).step_by(read).collect();
        let standard = run(ControlStringEnd::Standard, &ws, &s, &cuts);
        assert!(
            standard.replies.ends_with(
                "\x1b[?1049;2$y\x1b[?1003;2$y\x1b[?25;1$y\x1b[?1049;2$y\x1b[?1003;2$y\x1b[?25;1$y"
            ),
            "reads of {read}: the exit restored the screen: {:?}",
            standard.replies
        );
        assert_eq!(standard.screen, Screen::Normal, "reads of {read}");
        let kitty = run(ControlStringEnd::Kitty, &ws, &s, &cuts);
        assert!(
            kitty
                .replies
                .ends_with("\x1b[?1049;1$y\x1b[?1003;1$y\x1b[?25;1$y"),
            "reads of {read}: only the queries after the ST answered, the modes as set: {:?}",
            kitty.replies
        );
        assert!(
            !kitty.replies.contains("2$y\x1b[?1003;2$y"),
            "reads of {read}: a query inside the a=d answered: {:?}",
            kitty.replies
        );
        assert_eq!(kitty.screen, Screen::Alternate, "reads of {read}");
    }
}

/// **3. A status line printed inside a chunk**, the recording
/// `tests/recordings/kitty/mpv-status-in-chunk.raw`: one continuation chunk
/// with `ESC [ 1 K CR ESC [ 0 m <status> ESC [ 0 m` and 764 base64
/// characters before its `ST`, framed by a first chunk that opens a 1000x1
/// `f=24` transmission and a last that closes it. By default the status
/// line's `ESC` cancels the chunk and its upload, and the status line and
/// the base64 after it show as text from the start of the cursor's row; the
/// last chunk, which no upload is open for, answers that its image has no
/// data. Under the setting the status line and the base64 are payload,
/// nothing prints, and the frame comes up short.
#[test]
fn a_status_line_inside_a_chunk_shows_as_text_by_default_and_not_under_the_setting() {
    let cut = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/recordings/kitty/mpv-status-in-chunk.raw"
    ))
    .expect("the chunk");
    assert_eq!(cut.len(), 4176, "the premise: the chunk as recorded");
    let status = "AV: 00:00:01 / 00:05:37 (0%) A-V: -0.004 Cache: 0.7s/865KB";
    let at = cut
        .windows(4)
        .position(|w| w == b"\x1b[1K")
        .expect("the premise: the status line is inside");
    let tail_at = at + b"\x1b[1K\r\x1b[0m".len() + status.len() + b"\x1b[0m".len();
    assert_eq!(&cut[tail_at - 4..tail_at], b"\x1b[0m", "the premise");
    let tail = String::from_utf8_lossy(&cut[tail_at..cut.len() - 2]).into_owned();
    assert_eq!(tail.len(), 764, "the premise: the base64 after it");
    let mut s = AT.to_vec();
    s.extend_from_slice(b"\x1b_Ga=T,f=24,s=1000,v=1,i=1,m=1;\x1b\\");
    s.extend_from_slice(&cut);
    s.extend_from_slice(b"\x1b_Gm=0;\x1b\\");
    let ws = WinSize {
        ws_row: 12,
        ws_col: 120,
    };

    let standard = whole_and_split(ControlStringEnd::Standard, &ws, &s);
    let text: String = standard.rows[2..].concat();
    assert!(
        text.starts_with(&format!("{status}{tail}")),
        "the status line and the base64 after it, as text: {:?}",
        standard.rows
    );
    assert_eq!(standard.counts.cancelled, 1, "the chunk's upload cancelled");
    assert_eq!(standard.stored, vec![], "a frame stored");

    let kitty = whole_and_split(ControlStringEnd::Kitty, &ws, &s);
    assert!(kitty.rows.iter().all(String::is_empty), "text drawn");
    assert_eq!(kitty.cursor, (2, 4), "the cursor moved");
    assert_eq!(kitty.counts.cancelled, 0, "cancelled");
    assert_eq!(
        kitty.replies,
        "\x1b_Gi=1;ENODATA:Insufficient image data: 2499 < 3000\x1b\\"
    );
    assert_eq!(kitty.stored, vec![], "a short frame stored");
}

/// **4. A retransmission cancelled by a bare `ESC` keeps the old image and
/// drops the new one**, by default, at every split: id 1 is red, and its
/// retransmission in blue is cut by `ESC [ 5 ; 7 H` after its whole
/// payload, then the stream asks id 1 for a placement. The cancelled
/// transmission never concluded, so it replaced nothing and the put places
/// red. Under the setting the same bytes conclude at `ST` with the blue
/// pixels whole, and the put places blue. In chunks too: a first chunk of
/// the blue whole, its last chunk cut by a bare `ESC`.
#[test]
fn a_retransmission_cancelled_by_a_bare_esc_keeps_the_old_image() {
    let mut old = b"\x1b_Ga=t,f=24,s=1,v=1,i=1,q=2;".to_vec();
    old.extend_from_slice(RED_B64);
    old.extend_from_slice(b"\x1b\\");
    let mut one = b"\x1b_Ga=t,f=24,s=1,v=1,i=1,q=2;".to_vec();
    one.extend_from_slice(BLUE_B64);
    one.extend_from_slice(b"\x1b[5;7H\x1b\\");
    let mut chunked = b"\x1b_Ga=t,f=24,s=1,v=1,i=1,q=2,m=1;".to_vec();
    chunked.extend_from_slice(BLUE_B64);
    chunked.extend_from_slice(b"\x1b\\\x1b_Gm=0;\x1b[5;7H\x1b\\");
    let put = b"\x1b_Ga=p,i=1,p=1,C=1\x1b\\";
    for retransmit in [one, chunked] {
        // The cancel itself abandons the upload, no later `APC G` needed.
        let cut = whole_and_split(
            ControlStringEnd::Standard,
            &WS,
            &[&old[..], &retransmit].concat(),
        );
        assert_eq!(cut.counts.cancelled, 1, "cancelled at the ESC");
        assert_eq!(cut.stored, vec![(1, vec![RED])], "the old image kept");
        let s = [&old[..], &retransmit, put].concat();
        let standard = whole_and_split(ControlStringEnd::Standard, &WS, &s);
        assert_eq!(standard.counts.cancelled, 1, "the premise: cancelled");
        assert_eq!(standard.stored, vec![(1, vec![RED])], "the old image kept");
        assert_eq!(standard.replies, "\x1b_Gi=1,p=1;OK\x1b\\", "the put");
        assert_eq!(standard.cursor, (4, 6), "the CUP ran");

        let kitty = whole_and_split(ControlStringEnd::Kitty, &WS, &s);
        assert_eq!(kitty.counts.cancelled, 0, "cancelled");
        assert_eq!(kitty.stored, vec![(1, vec![BLUE])], "the new image");
        assert_eq!(kitty.replies, "\x1b_Gi=1,p=1;OK\x1b\\", "the put");
    }
}

/// **A `BEL` inside an `APC G`**: by default it is ignored, as the
/// `sos/pm/apc string` state ignores `07`, so the command's payload runs
/// on past it to `ST` and the image is whole. Under the setting the `BEL`
/// ends the command, which comes up short, and what follows prints.
#[test]
fn a_bel_inside_an_apc_g_is_ignored_by_default_and_ends_it_under_the_setting() {
    let mut s = AT.to_vec();
    s.extend_from_slice(b"\x1b_Ga=t,f=24,s=1,v=1,i=1;/w\x07AA\x1b\\");
    let standard = whole_and_split(ControlStringEnd::Standard, &WS, &s);
    assert_eq!(standard.stored, vec![(1, vec![RED])], "the BEL ignored");
    assert_eq!(standard.replies, "\x1b_Gi=1;OK\x1b\\");
    assert!(standard.rows.iter().all(String::is_empty), "text drawn");

    let kitty = whole_and_split(ControlStringEnd::Kitty, &WS, &s);
    assert_eq!(kitty.stored, vec![], "a short image stored");
    assert_eq!(
        kitty.replies,
        "\x1b_Gi=1;ENODATA:Insufficient image data: 1 < 3\x1b\\"
    );
    assert_eq!(kitty.rows[2], "    AA", "what follows the BEL prints");
}

/// **`CAN` and `SUB` inside an `APC G`**: by default each cancels it, as
/// it cancels any control string, and the cancel is counted; under the
/// setting each is payload, which the decode stops at.
#[test]
fn can_and_sub_cancel_an_apc_g_by_default_and_are_payload_under_the_setting() {
    for cancel in [0x18u8, 0x1a] {
        let mut s = AT.to_vec();
        s.extend_from_slice(b"\x1b_Ga=t,f=24,s=1,v=1,i=1;");
        s.extend_from_slice(RED_B64);
        s.push(cancel);
        s.extend_from_slice(b"\x1b\\");
        let standard = whole_and_split(ControlStringEnd::Standard, &WS, &s);
        assert_eq!(standard.counts.cancelled, 1, "{cancel:#04x}: cancelled");
        assert_eq!(standard.stored, vec![], "{cancel:#04x}: stored");
        let kitty = whole_and_split(ControlStringEnd::Kitty, &WS, &s);
        assert_eq!(kitty.counts.cancelled, 0, "{cancel:#04x}: cancelled");
        assert_eq!(kitty.stored, vec![(1, vec![RED])], "{cancel:#04x}");
    }
}

/// **Every other control string is read the same under both**: an `APC`
/// that is not a kitty graphics command, cut by a bare `ESC`, is cancelled under either,
/// and the `CUP` after it runs.
#[test]
fn an_apc_that_is_not_kittys_is_cancelled_by_a_bare_esc_under_both() {
    let mut s = AT.to_vec();
    s.extend_from_slice(b"\x1b_Hello\x1b[5;7H\x1b\\");
    for mode in MODES {
        let l = whole_and_split(mode, &WS, &s);
        assert_eq!(l.cursor, (4, 6), "{mode:?}");
        assert!(l.rows.iter().all(String::is_empty), "{mode:?}: text drawn");
    }
}

/// **The setting is the embedder's**: `Standard` until it is set, and no
/// reset changes it.
#[test]
fn the_setting_is_standard_until_set_and_no_reset_changes_it() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    assert_eq!(t.control_string_end(), ControlStringEnd::Standard);
    t.set_control_string_end(ControlStringEnd::Kitty);
    for reset in [&b"\x1bc"[..], b"\x1b[!p"] {
        t.write_at(BytePos::ZERO, reset);
        t.feed(BytePos::ZERO, ReadLen::from_index(reset.len()));
        assert_eq!(t.control_string_end(), ControlStringEnd::Kitty, "{reset:?}");
    }
}
