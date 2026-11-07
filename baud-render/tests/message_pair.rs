//! **A message's pair and placement, on the CPU path**, which the Linux
//! driver draws with. The instance and cell paths are held to the same
//! pixels by `the_message_pair_holds_on_every_arm`
//! (`baud-present-macos/tests/cells.rs`), which draws all three.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Codepoint, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{
    Argb, Blend, Damage, Field, Highlight, Hints, HoverPaint, LineStyle, LinkHover, LinkLine,
    LinkLook, LinkSpan, MessageAt, MessageColours, MessagePaint, Pair, PxSize, Region, Renderer,
    SelectionPulse, Stack, Target, Theme, TipAnchor, TipLook, message_look,
};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn paint(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32], damage: Damage) {
    let (w, h) = r.pixel_size();
    let mut target = Target::new(px, w, w, h).unwrap();
    r.render(t, &mut target, damage);
}

const WORDS: &str = "Copied 42 characters";

/// A tip under row 3, the words on row 4, so the plain frame under it is the
/// page.
fn tip(hints: Hints) -> TipLook {
    let a = TipAnchor {
        first: (Row::from_index(3), Col::at_index(5)),
        last: (Row::from_index(3), Col::at_index(5)),
    };
    let mut look = message_look(
        WORDS,
        Cols::of_index(80),
        Rows::of_index(24),
        MessageAt::Tip(a),
    )
    .unwrap();
    for f in &mut look.lines {
        f.hints.fill(hints);
    }
    look
}

/// **What a program can do to the colours of what baud draws**: each a
/// sequence that moves the selection's colours, the defaults under them,
/// the palette or the video.
const PROGRAMS: [(&str, &[u8]); 7] = [
    ("nothing", b""),
    ("OSC 17", b"\x1b]17;#102030\x1b\\"),
    ("OSC 19", b"\x1b]19;#a0a0a0\x1b\\"),
    ("OSC 10", b"\x1b]10;#00ff00\x1b\\"),
    ("OSC 11", b"\x1b]11;#ff00ff\x1b\\"),
    ("OSC 4", b"\x1b]4;0;#ff0000;7;#0000ff;15;#ffff00\x1b\\"),
    ("?5", b"\x1b[?5h"),
];

struct Drawn {
    px: Vec<u32>,
    /// The row's width in pixels.
    stride: baud_font::Px,
    cell: (usize, usize),
}

impl Drawn {
    fn cell_px(&self, row: usize, col: usize) -> impl Iterator<Item = u32> + '_ {
        let (cw, ch) = self.cell;
        let stride = usize::try_from(self.stride.value()).unwrap();
        (row * ch..(row + 1) * ch)
            .flat_map(move |y| (col * cw..(col + 1) * cw).map(move |x| self.px[y * stride + x]))
    }
}

fn drawn(program: &[u8], theme: Theme, colours: MessageColours, look: &TipLook) -> Drawn {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25lsome text under it\r\nand more");
    feed(&mut t, program);
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(24),
    )
    .unwrap();
    r.fit(&t);
    r.set_message_colours(colours);
    r.set_tip(Some(look.clone()));
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let mut px = vec![0u32; n];
    paint(&mut r, &mut t, &mut px, Damage::Full);
    let m = r.metrics();
    Drawn {
        px,
        stride: w,
        cell: (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        ),
    }
}

/// **The box's own pixels are the box, whole, and its ink reaches the text
/// and goes no further**: a blank of the padding is the box in every pixel,
/// alpha included, and every pixel of the first letter's cell lies between
/// the two colours, the text's own reached.
fn assert_the_pair(d: &Drawn, look: &TipLook, pair: Pair, what: &str) {
    let line = &look.lines[0];
    let (row, left) = (line.row.index(), line.col.index());
    assert!(
        d.cell_px(row, left).all(|p| p == pair.page.0),
        "{what}: the padding is the box, 0x{:08x}, found 0x{:08x}",
        pair.page.0,
        d.cell_px(row, left).next().unwrap()
    );
    let between = |a: u32, b: u32, v: u32| (a.min(b)..=a.max(b)).contains(&v);
    let mut reached = false;
    for col in left + 1..left + line.text.len() - 1 {
        for p in d.cell_px(row, col) {
            reached |= p == pair.text.0;
            for s in [0, 8, 16] {
                assert!(
                    between(
                        (pair.page.0 >> s) & 0xFF,
                        (pair.text.0 >> s) & 0xFF,
                        (p >> s) & 0xFF
                    ),
                    "{what}: 0x{p:08x} is outside the pair at column {col}"
                );
            }
            assert_eq!(p >> 24, 0xFF, "{what}: opaque, at column {col}");
        }
    }
    assert!(reached, "{what}: the text's own colour was drawn");
}

/// **Nothing a program sets reaches baud's pair on the CPU path**: under
/// `OSC 17`, `19`, `10`, `11`, `4` and `?5`, on dark and light, and over a
/// dimmed glass page, the box is `#F4A32C` and the text `#15171B`, in every
/// pixel. The control: a field in the selection's colours moves under
/// `OSC 17`, so a sequence that reached the pair would show here.
/// Perturbations: read `Hints::MESSAGE` as the selection in `Drawing::of`;
/// resolve the pair with `base.of`, which goes through the rendition.
#[test]
fn nothing_a_program_sets_reaches_the_pair() {
    let look = tip(Hints::MESSAGE);
    let mut glass = Theme::dark();
    glass.bg_alpha = Chan::from_u8(0x38);
    for (name, theme) in [
        ("dark", Theme::dark()),
        ("light", Theme::light()),
        ("glass", glass),
    ] {
        for (program, bytes) in PROGRAMS {
            let d = drawn(bytes, theme.clone(), MessageColours::default(), &look);
            assert_the_pair(&d, &look, Pair::BAUD, &format!("{name}, {program}"));
        }
    }
    // The control.
    let plain = tip(Hints::SELECTION);
    let quiet = drawn(b"", Theme::dark(), MessageColours::default(), &plain);
    let moved = drawn(
        b"\x1b]17;#102030\x1b\\",
        Theme::dark(),
        MessageColours::default(),
        &plain,
    );
    let (row, left) = (plain.lines[0].row.index(), plain.lines[0].col.index());
    assert!(
        quiet.cell_px(row, left).ne(moved.cell_px(row, left)),
        "the control: OSC 17 moves a field in the selection's colours"
    );
}

/// **`"none"` is the selection's colours in force, and under 4.5:1 they
/// give way to baud's at draw**: a program's readable pair is drawn, its
/// unreadable one is not, whichever half it moved. The fallback is the
/// fixed pair, opaque over glass too. In force means as drawn: with the
/// defaults made equal under reverse video, the swap hands the page the
/// text's floored colour, as it does every selected cell, so the selection
/// reads and `"none"` draws it, pixel for pixel as a field in the
/// selection's colours. Perturbation: drop the check in
/// `MessageColours::in_force`, and the grey pair draws.
#[test]
fn none_falls_back_to_the_fixed_pair_at_draw() {
    let look = tip(Hints::MESSAGE);
    let none = MessageColours {
        page: MessagePaint::Selection,
        text: MessagePaint::Selection,
    };
    let rgb = |r, g, b| Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b));
    let reads = drawn(
        b"\x1b]17;#000040\x1b\\\x1b]19;#ffff00\x1b\\",
        Theme::dark(),
        none,
        &look,
    );
    assert_the_pair(
        &reads,
        &look,
        Pair {
            page: rgb(0, 0, 0x40),
            text: rgb(0xFF, 0xFF, 0),
        },
        "a pair that reads",
    );
    let unreadable: [&[u8]; 2] = [
        b"\x1b]17;#808080\x1b\\\x1b]19;#909090\x1b\\",
        // Only the page moved, under the theme's reverse-video text.
        b"\x1b]19;#101010\x1b\\\x1b]17;#141414\x1b\\",
    ];
    for bytes in unreadable {
        let d = drawn(bytes, Theme::dark(), none, &look);
        assert_the_pair(&d, &look, Pair::BAUD, "under 4.5:1");
    }
    // The defaults made equal under reverse video: the selection as drawn
    // is the floored text on the page, which reads, so it is drawn.
    let equal: &[u8] = b"\x1b]10;#777777\x1b\\\x1b]11;#777777\x1b\\";
    let d = drawn(equal, Theme::dark(), none, &look);
    let selection = drawn(
        equal,
        Theme::dark(),
        MessageColours::default(),
        &tip(Hints::SELECTION),
    );
    let line = &look.lines[0];
    let (row, left) = (line.row.index(), line.col.index());
    assert!(
        d.cell_px(row, left).all(|p| p != Pair::BAUD.page.0),
        "equal defaults: the selection in force reads, so not baud's"
    );
    for col in left..left + line.text.len() {
        assert!(
            d.cell_px(row, col).eq(selection.cell_px(row, col)),
            "equal defaults: column {col} is the selection's colours as drawn"
        );
    }
    // And the pair as drawn reads, by `Pair::reads`, the measure `in_force`
    // takes: the padding is the page, and the letters' pixel farthest from
    // it is the text.
    let page = d.cell_px(row, left).next().unwrap();
    let apart = |p: u32| {
        [0, 8, 16]
            .iter()
            .map(|s| ((p >> s) & 0xFF).abs_diff((page >> s) & 0xFF))
            .sum::<u32>()
    };
    let text = (left + 1..left + line.text.len() - 1)
        .flat_map(|col| d.cell_px(row, col))
        .max_by_key(|&p| apart(p))
        .unwrap();
    assert!(
        Pair {
            page: Argb(page),
            text: Argb(text),
        }
        .reads(),
        "equal defaults: the drawn pair 0x{text:08x} on 0x{page:08x} reads at 4.5:1"
    );
    // Under `?5`, the selection's own two colours made equal: as drawn they
    // still do not read, so the line falls back.
    let d = drawn(
        b"\x1b[?5h\x1b]17;#555555\x1b\\\x1b]19;#555555\x1b\\",
        Theme::dark(),
        none,
        &look,
    );
    assert_the_pair(&d, &look, Pair::BAUD, "?5, the selection's two equal");
    let mut glass = Theme::dark();
    glass.bg_alpha = Chan::from_u8(0x38);
    let d = drawn(unreadable[0], glass, none, &look);
    assert_the_pair(&d, &look, Pair::BAUD, "under 4.5:1 over glass");
}

/// **A message is drawn and taken away without a trace**: its frame covers
/// only its own cells, an incremental frame is the full one, and with none
/// the frame is the plain one, on the end frame that costs nothing else.
/// Perturbation: drop `tipped` from the convert's damage.
#[test]
fn a_message_is_drawn_and_cleared_without_a_trace() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[?25lsome text\r\nand more\r\nand more\r\nselected",
    );
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(24),
    )
    .unwrap();
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let mut plain = vec![0u32; n];
    paint(&mut r, &mut t, &mut plain, Damage::Full);
    let mut shown = plain.clone();
    let look = tip(Hints::MESSAGE);
    r.set_tip(Some(look.clone()));
    paint(&mut r, &mut t, &mut shown, Damage::Incremental);
    let mut full = vec![0u32; n];
    paint(&mut r, &mut t, &mut full, Damage::Full);
    assert!(full == shown, "an incremental frame is the full one");
    let m = r.metrics();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let stride = usize::try_from(w.value()).unwrap();
    let area = look.area;
    for (i, (&a, &b)) in plain.iter().zip(&shown).enumerate() {
        let (x, y) = (i % stride, i / stride);
        let inside = area.holds(Row::from_index(y / ch), Col::at_index(x / cw));
        assert!(inside || a == b, "outside its cells: pixel {x},{y} moved");
    }
    r.set_tip(None);
    paint(&mut r, &mut t, &mut shown, Damage::Incremental);
    assert!(shown == plain, "nothing of it is left behind");
}

/// The checksum a program reads of the whole screen, as the core answers.
fn checksum(t: &mut Term<'_>) -> Vec<u8> {
    t.clear_reply();
    feed(t, b"\x1b[7;1;1;1;24;80*y");
    t.reply().to_vec()
}

/// **A program cannot read a message back**: `DECRQCRA` answers the same
/// with one drawn over the bytes it reads, on the tip's row and on the bar's.
/// It is an overlay of the frame's copy, and the core's cells never hold it.
/// The control: the same text written as cells moves the answer, so the
/// checksum can see that row. Perturbation: write the words through
/// `Term::feed`, which the checksum then reads.
#[test]
fn decrqcra_answers_the_same_with_a_message_up() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25lsome text\r\nand more");
    let before = checksum(&mut t);
    assert!(!before.is_empty(), "the control: the query answers");
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(24),
    )
    .unwrap();
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let mut px = vec![0u32; n];
    let bar = message_look(
        WORDS,
        Cols::of_index(80),
        Rows::of_index(24),
        MessageAt::Bar,
    )
    .unwrap();
    for look in [tip(Hints::MESSAGE), bar] {
        r.set_tip(Some(look));
        paint(&mut r, &mut t, &mut px, Damage::Full);
        assert_eq!(checksum(&mut t), before, "the same answer with it up");
    }
    r.set_tip(None);
    paint(&mut r, &mut t, &mut px, Damage::Full);
    assert_eq!(checksum(&mut t), before, "and with it gone");
    feed(&mut t, b"\x1b[5;7HCopied");
    assert_ne!(
        checksum(&mut t),
        before,
        "the control: the words as cells would move it"
    );
}

fn anchor(first: (usize, usize), last: (usize, usize)) -> TipAnchor {
    TipAnchor {
        first: (Row::from_index(first.0), Col::at_index(first.1)),
        last: (Row::from_index(last.0), Col::at_index(last.1)),
    }
}

fn placed(a: TipAnchor, cols: usize, rows: usize) -> (usize, usize) {
    let l = message_look(
        WORDS,
        Cols::of_index(cols),
        Rows::of_index(rows),
        MessageAt::Tip(a),
    )
    .unwrap();
    (l.lines[0].row.index(), l.lines[0].col.index())
}

/// **The row under the selection, above it on the last row, from its left,
/// moved in to fit.** A stream's row under it starts at the left edge when
/// it runs across rows, and at its start on one row; a block at its left.
/// A selection of every row sits on the last row. Perturbations: read
/// `room` as the width; place above from the last row.
#[test]
fn a_message_is_placed_by_the_selection() {
    assert_eq!(placed(anchor((3, 5), (3, 5)), 80, 24), (4, 5), "under it");
    assert_eq!(
        placed(anchor((23, 5), (23, 5)), 80, 24),
        (22, 5),
        "above it, on the last row"
    );
    assert_eq!(
        placed(anchor((20, 12), (23, 0)), 80, 24),
        (19, 12),
        "above its first row, at that row's start"
    );
    let width = WORDS.len() + 2;
    assert_eq!(
        placed(anchor((3, 70), (3, 70)), 80, 24),
        (4, 80 - width),
        "moved in to fit"
    );
    assert_eq!(
        placed(anchor((0, 5), (23, 0)), 80, 24),
        (23, 5),
        "every row selected: on the last, at the selection's start"
    );
    let narrow = message_look(
        WORDS,
        Cols::of_index(12),
        Rows::of_index(24),
        MessageAt::Tip(anchor((3, 5), (3, 5))),
    )
    .unwrap();
    assert_eq!(narrow.lines.len(), 2, "wrapped at a narrow width");
    assert!(
        narrow
            .lines
            .iter()
            .all(|f| f.col.index() + f.text.len() <= 12)
    );
    assert!(
        message_look(WORDS, Cols::of_index(2), Rows::of_index(24), MessageAt::Bar).is_none(),
        "no room at all"
    );
    let short = message_look(
        "Copied 1 character",
        Cols::of_index(12),
        Rows::of_index(1),
        MessageAt::Bar,
    )
    .unwrap();
    assert_eq!(short.lines.len(), 1, "the first line that fits");
}

/// **A highlight names the rows it lies on**: a block its left on both, a
/// stream its start on its first row and the edge on the rest, and one with
/// no cell on the view none. Perturbation: read a stream's last row from
/// its start.
#[test]
fn an_anchor_is_where_the_highlight_lies() {
    let h = |from: (usize, usize), to: (usize, usize), region| {
        Highlight::new(
            (Row::from_index(from.0), Col::at_index(from.1)),
            (Row::from_index(to.0), Col::at_index(to.1)),
            region,
        )
    };
    assert_eq!(
        TipAnchor::of(&h((2, 7), (2, 9), Region::Stream)),
        Some(anchor((2, 7), (2, 7)))
    );
    assert_eq!(
        TipAnchor::of(&h((2, 7), (4, 3), Region::Stream)),
        Some(anchor((2, 7), (4, 0)))
    );
    assert_eq!(
        TipAnchor::of(&h((2, 7), (4, 3), Region::Block)),
        Some(anchor((2, 3), (4, 3)))
    );
    assert_eq!(TipAnchor::of(&h((4, 0), (2, 0), Region::Stream)), None);
}

/// **The bar is the main display's last row from the left edge, in the
/// pair**: where the link preview's bar goes with no link under it.
/// Perturbation: place `MessageAt::Bar` at the top.
#[test]
fn a_bar_message_sits_on_the_last_row() {
    let l = message_look(
        WORDS,
        Cols::of_index(80),
        Rows::of_index(24),
        MessageAt::Bar,
    )
    .unwrap();
    assert_eq!(l.lines.len(), 1);
    assert_eq!(l.lines[0].row, Row::from_index(23));
    assert_eq!(l.lines[0].col, Col::ZERO);
    assert!(l.lines[0].hints.iter().all(|h| *h == Hints::MESSAGE));
}

/// **A pulse recolours the selection, and no field of baud's**: the selected
/// cells take the pair, a field in the selection's colours keeps its own, and
/// with the pulse over the frame is what it was. Perturbations: read
/// `r.hilite()` in `composition_parts`, which flashes the field; drop the
/// pulse's arm in `refresh_screen`, which leaves the selection as it was.
#[test]
fn a_pulse_recolours_the_selection_and_no_field() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25lsome text\r\nand more");
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(24),
    )
    .unwrap();
    r.fit(&t);
    r.set_highlight(Some(Highlight::new(
        (Row::ZERO, Col::ZERO),
        (Row::ZERO, Col::at_index(3)),
        Region::Stream,
    )));
    let field = tip(Hints::SELECTION);
    r.set_tip(Some(field.clone()));
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let mut before = vec![0u32; n];
    paint(&mut r, &mut t, &mut before, Damage::Full);
    let m = r.metrics();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let stride = usize::try_from(w.value()).unwrap();
    let cell = |px: &[u32], row: usize, col: usize| -> Vec<u32> {
        (row * ch..(row + 1) * ch)
            .flat_map(|y| px[y * stride + col * cw..y * stride + (col + 1) * cw].to_vec())
            .collect()
    };
    r.set_selection_pulse(SelectionPulse::On);
    let mut during = vec![0u32; n];
    paint(&mut r, &mut t, &mut during, Damage::Full);
    assert!(
        cell(&during, 0, 0).contains(&Pair::BAUD.page.0),
        "the selected cell takes the pair's box"
    );
    let (row, left) = (field.lines[0].row.index(), field.lines[0].col.index());
    assert_eq!(
        cell(&during, row, left),
        cell(&before, row, left),
        "the field keeps the selection's own colours"
    );
    r.set_selection_pulse(SelectionPulse::Off);
    let mut after = vec![0u32; n];
    paint(&mut r, &mut t, &mut after, Damage::Full);
    assert!(
        after == before,
        "with the pulse over the frame is as it was"
    );
}

/// The bar with `address`, on the bottom row in the pair, as `baud-app`'s
/// `link::preview` builds it.
fn address(address: &str, hints: Hints) -> LinkLook {
    let text: Vec<Codepoint> = format!(" {address} ")
        .chars()
        .map(Codepoint::from_char)
        .collect();
    let n = text.len();
    LinkLook {
        spans: Vec::new(),
        bar: Field {
            row: Row::from_index(23),
            col: Col::ZERO,
            text,
            hints: vec![hints; n],
            caret: Col::at_index(n),
        },
    }
}

/// The frame with `set` having set the renderer up, under `program`.
fn framed(
    program: &[u8],
    theme: Theme,
    set: impl FnOnce(&mut Renderer<'_, '_>, &Term<'_>),
) -> Drawn {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"some text under it\r\nand more");
    feed(&mut t, program);
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        theme,
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(24),
    )
    .unwrap();
    r.fit(&t);
    set(&mut r, &t);
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let mut px = vec![0u32; n];
    paint(&mut r, &mut t, &mut px, Damage::Full);
    let m = r.metrics();
    Drawn {
        px,
        stride: w,
        cell: (
            usize::try_from(m.cell_w.value()).unwrap(),
            usize::try_from(m.cell_h.value()).unwrap(),
        ),
    }
}

/// **The deciding test of the bar and the notice, on the CPU path**: a program
/// sets `OSC 17`, `19`, `10`, `11`, `4` and `?5` to match, and what the bar
/// and the config's notice row draw is the pair in every pixel, an opaque box
/// and its own text. The control: a bar in the selection's colours moves with
/// the same sequences, which is what both were before. Perturbations: build
/// the notice with `Hints::SELECTION` in `Renderer::notice_field`; read
/// `Hints::MESSAGE` as the selection in `Drawing::of`.
#[test]
fn the_address_and_the_notice_take_no_colour_from_a_program() {
    const MATCH: &[u8] = b"\x1b]17;#101010\x1b\\\x1b]19;#101010\x1b\\\x1b]10;#101010\x1b\\\x1b]11;#101010\x1b\\\x1b]4;0;#101010;15;#101010\x1b\\";
    let mut programs: Vec<(&str, &[u8])> = PROGRAMS.to_vec();
    programs.push(("all of them to match", MATCH));
    let bar = address("https://example.org/docs", Hints::MESSAGE);
    let width = bar.bar.text.len();
    let line = TipLook {
        lines: vec![bar.bar.clone()],
        ..tip(Hints::MESSAGE)
    };
    let notice: Vec<Codepoint> = "baud: a setting was not read"
        .chars()
        .map(Codepoint::from_char)
        .collect();
    for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
        for (program, bytes) in &programs {
            let d = framed(bytes, theme.clone(), |r, _| r.set_link(Some(bar.clone())));
            assert_the_pair(
                &d,
                &line,
                Pair::BAUD,
                &format!("{name}, {program}, the bar"),
            );
            let d = framed(bytes, theme.clone(), |r, _| {
                r.set_notice(Some(notice.clone()))
            });
            let row = 24;
            assert!(
                d.cell_px(row, 60).all(|p| p == Pair::BAUD.page.0),
                "{name}, {program}: the notice's box, whole"
            );
            assert!(
                (0..notice.len())
                    .flat_map(|c| d.cell_px(row, c).collect::<Vec<_>>())
                    .any(|p| p == Pair::BAUD.text.0),
                "{name}, {program}: the notice's text"
            );
        }
    }
    let plain = address("https://example.org/docs", Hints::SELECTION);
    let quiet = framed(b"", Theme::dark(), |r, _| r.set_link(Some(plain.clone())));
    let moved = framed(MATCH, Theme::dark(), |r, _| r.set_link(Some(plain.clone())));
    assert!(
        quiet
            .cell_px(23, width - 1)
            .ne(moved.cell_px(23, width - 1)),
        "the control: a program moves the selection's colours"
    );
}

/// **A visible cursor cannot cover a message**: a block cursor moved by a
/// program onto a letter of the bar or of a tip, under its own `OSC 12` in the
/// box's colour, draws nothing over it: the frame is the one with the cursor
/// hidden. The control: with no message the same cursor moves the pixels.
/// Perturbation: drop `clear_of_messages` from `convert_with`.
#[test]
fn a_visible_cursor_cannot_cover_a_message() {
    let bar = address("https://example.org/docs", Hints::MESSAGE);
    let tipped = tip(Hints::MESSAGE);
    let (row, col) = (tipped.lines[0].row.index(), tipped.lines[0].col.index() + 2);
    let onto_tip = format!("\x1b[2 q\x1b]12;#F4A32C\x1b\\\x1b[{};{}H", row + 1, col + 1);
    let onto_bar = "\x1b[2 q\x1b]12;#F4A32C\x1b\\\x1b[24;4H".to_string();
    let same = |a: &Drawn, b: &Drawn| a.px == b.px;
    let hide = |s: &str| format!("{s}\x1b[?25l");
    let (shown, hidden) = (
        framed(onto_bar.as_bytes(), Theme::dark(), |r, _| {
            r.set_link(Some(bar.clone()));
        }),
        framed(hide(&onto_bar).as_bytes(), Theme::dark(), |r, _| {
            r.set_link(Some(bar.clone()));
        }),
    );
    assert!(same(&shown, &hidden), "the bar: as with no cursor");
    let (shown, hidden) = (
        framed(onto_tip.as_bytes(), Theme::dark(), |r, _| {
            r.set_tip(Some(tipped.clone()));
        }),
        framed(hide(&onto_tip).as_bytes(), Theme::dark(), |r, _| {
            r.set_tip(Some(tipped.clone()));
        }),
    );
    assert!(same(&shown, &hidden), "the tip: as with no cursor");
    let (shown, hidden) = (
        framed(onto_bar.as_bytes(), Theme::dark(), |_, _| {}),
        framed(hide(&onto_bar).as_bytes(), Theme::dark(), |_, _| {}),
    );
    assert!(!same(&shown, &hidden), "the control: the cursor draws");
}

/// **A program cannot read a link's line, its look or its bar back**:
/// `DECRQCRA` answers the same with a line at rest, each look and the bar up,
/// on the rows they lie on. The control: the same text underlined in the cells
/// moves the answer. Perturbation: write the line through `SGR 4`.
#[test]
fn decrqcra_answers_the_same_with_a_line_up() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25lsee https://example.org/x today");
    let before = checksum(&mut t);
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(24),
    )
    .unwrap();
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let cell = t.view(Rows::ZERO).row(Row::ZERO)[4];
    let word = t.style_in(baud_domain::RowStyles::Live, cell);
    let line = LinkLine {
        row: Row::ZERO,
        from: Col::at_index(4),
        to: Col::at_index(24),
        word,
    };
    r.set_link_lines(&[line], LineStyle::Solid);
    for paint_kind in [HoverPaint::Box, HoverPaint::Line, HoverPaint::Text] {
        r.set_link_hover(Some(&LinkHover {
            spans: vec![LinkSpan {
                row: Row::ZERO,
                from: Col::at_index(4),
                to: Col::at_index(24),
            }],
            paint: paint_kind,
        }));
        r.set_link(Some(address("https://example.org/x", Hints::MESSAGE)));
        paint(&mut r, &mut t, &mut px, Damage::Full);
        assert_eq!(checksum(&mut t), before, "the same with {paint_kind:?} up");
    }
    feed(&mut t, b"\x1b[1;5H\x1b[4mhttps\x1b[m");
    assert_ne!(checksum(&mut t), before, "the control: as cells it moves");
}

/// **A line at rest is in the text's own colour, and goes without a trace**:
/// red text has a red line under it, an incremental frame is the full one, and
/// with none the frame is the plain one. Perturbations: draw it in the default
/// foreground, as the preview's underline; drop `lines_changed` from the
/// convert's damage.
#[test]
fn a_line_at_rest_is_the_texts_colour_and_leaves_no_trace() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&mono).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[?25lsee \x1b[38;2;255;0;0mhttps://example.org/x\x1b[m",
    );
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(80),
        Rows::of_index(24),
    )
    .unwrap();
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let mut plain = vec![0u32; n];
    paint(&mut r, &mut t, &mut plain, Damage::Full);
    let cell = t.view(Rows::ZERO).row(Row::ZERO)[4];
    let word = t.style_in(baud_domain::RowStyles::Live, cell);
    r.set_link_lines(
        &[LinkLine {
            row: Row::ZERO,
            from: Col::at_index(4),
            to: Col::at_index(24),
            word,
        }],
        LineStyle::Solid,
    );
    let mut shown = plain.clone();
    paint(&mut r, &mut t, &mut shown, Damage::Incremental);
    let mut full = vec![0u32; n];
    paint(&mut r, &mut t, &mut full, Damage::Full);
    assert!(full == shown, "an incremental frame is the full one");
    let m = r.metrics();
    let (cw, stride) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(w.value()).unwrap(),
    );
    let y = usize::try_from(m.underline.value()).unwrap();
    assert!(
        (4 * cw..25 * cw).all(|x| shown[y * stride + x] & 0x00FF_FFFF == 0x00FF_0000),
        "the line is the text's own red, across its cells"
    );
    r.set_link_lines(&[], LineStyle::Solid);
    paint(&mut r, &mut t, &mut shown, Damage::Incremental);
    assert!(shown == plain, "nothing of it is left behind");
}
