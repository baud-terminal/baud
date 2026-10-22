//! **Each command's exit status beside its prompt** (`baud_render::status`):
//! the statuses a shell's `OSC 133 ; D` sent, a mark a status on the row
//! its prompt began, in a strip of the frame's own at its left, following
//! the line through scrolling, a reflow and the history, none on the
//! alternate screen, and nothing at all with the setting off.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{Blend, Damage, Leaving, PxSize, Renderer, Stack, StatusMarks, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 16,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn renderer<'f, 'a>(faces: &'f [Face<'a>], ws: &WinSize) -> Renderer<'f, 'a> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap()
}

/// A shell session: `ls` (0), a failed `cargo` (101), `git` (0), an
/// interrupted `sleep` (130), a line cancelled with a `D` and no `C`, one
/// with `err=` empty after a code of 1 (success), one with `err=CANCEL`,
/// and the prompt the shell is at, unreported. Prompts on rows 0, 2, 4, 6,
/// 8, 9, 11 and 12.
const SESSION: &[u8] =
    b"\x1b]133;A\x07$ \x1b]133;B\x07ls\x1b]133;C\x07\r\nfiles\r\n\x1b]133;D;0\x07\
\x1b]133;A\x07$ \x1b]133;B\x07cargo\x1b]133;C\x07\r\nerror\r\n\x1b]133;D;101\x07\
\x1b]133;A\x07$ \x1b]133;B\x07git\x1b]133;C\x07\r\n M x\r\n\x1b]133;D;0\x07\
\x1b]133;A\x07$ \x1b]133;B\x07sleep\x1b]133;C\x07\r\n^C\r\n\x1b]133;D;130\x07\
\x1b]133;A\x07$ \x1b]133;B\x07half\r\n\x1b]133;D\x07\
\x1b]133;A\x07$ \x1b]133;B\x07odd\x1b]133;C\x07\r\nout\r\n\x1b]133;D;1;err=\x07\
\x1b]133;A\x07$ \x1b]133;B\x07stop\x1b]133;C\x07\r\n\x1b]133;D;;err=CANCEL\x07\
\x1b]133;A\x07$ \x1b]133;B\x07";

/// The theme's colour for palette entry `i`, as the strip draws it.
fn entry(i: usize) -> baud_render::Argb {
    baud_render::dark_palette()[i]
}

/// The marks the renderer placed, each its row (from its record's top) and
/// colour.
fn marks(r: &Renderer<'_, '_>) -> Vec<(i32, baud_render::Argb)> {
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    r.strip_records()
        .iter()
        .map(|i| (i32::from(i.y.0).div_euclid(ch), i.color))
        .collect()
}

/// **A mark for each status the shell sent, and none it did not**: blue
/// success for a code of 0 and for `err=` empty whatever the code, red
/// failure for any other code, grey for a cancelled line, nothing for the
/// prompt still open; each on its prompt's row, in a strip one cell wide.
#[test]
fn a_mark_shows_each_status_the_shell_sent() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces, &WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SESSION);
    r.set_status_marks(StatusMarks::On);
    assert_eq!(r.strip(), r.metrics().cell_w);
    assert_eq!(
        r.whole_size().0.value(),
        r.frame_size().0.value() + r.metrics().cell_w.value()
    );
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    let (blue, red, grey) = (entry(4), entry(1), entry(8));
    assert_eq!(
        marks(&r),
        [
            (0, blue),
            (2, red),
            (4, blue),
            (6, red),
            (8, grey),
            (9, blue),
            (11, grey),
        ]
    );
    for i in r.strip_records() {
        assert!(i.x.0 >= 0, "inside the strip");
        assert!(u32::from(i.w.0) <= r.strip().value(), "inside the strip");
    }
}

/// A plain session of `n` commands, each its prompt, two lines of output
/// and a `D` of 0 but every third, which fails; then an open prompt.
fn history(t: &mut Term<'_>, n: usize) {
    for k in 0..n {
        let code = if k % 3 == 2 { 1 } else { 0 };
        let s = format!(
            "\x1b]133;A\x07$ cmd{k}\x1b]133;B\x07\x1b]133;C\x07\r\nout{k}\r\nmore{k}\r\n\x1b]133;D;{code}\x07"
        );
        feed(t, s.as_bytes());
    }
    feed(t, b"\x1b]133;A\x07$ ");
}

/// The rows of the window `back` lines up whose text begins with a
/// prompt's `$ cmd`, top first.
fn prompt_rows(t: &Term<'_>, back: usize) -> Vec<i32> {
    let v = t.view(Rows::of_count(back));
    (0..v.rows().index())
        .filter(|&y| {
            let row = v.row(Row::from_index(y));
            let text: String = row
                .iter()
                .take(5)
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or(' ')
                })
                .collect();
            text == "$ cmd"
        })
        .map(|y| i32::try_from(y).unwrap())
        .collect()
}

/// **A mark stays on its prompt's line**: at every depth of the window
/// over the history, and after a narrowing reflow re-cuts the lines, each
/// ended command's mark is on the row its `$ cmd` text is on.
#[test]
fn a_mark_follows_its_line_through_the_history_and_a_reflow() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    history(&mut t, 20);
    let check = |t: &Term<'_>, ws: &WinSize| {
        let mut r = renderer(&faces, ws);
        r.set_status_marks(StatusMarks::On);
        let mut depths = 0;
        for back in 0..=t.scrollback().len() {
            r.place_marks(t, Rows::of_count(back), Leaving::None, Px::of(0));
            let rows: Vec<i32> = marks(&r).into_iter().map(|(y, _)| y).collect();
            // The open prompt, `$ ` with no command, has no mark and is
            // not counted.
            assert_eq!(rows, prompt_rows(t, back), "back {back}");
            depths += 1;
        }
        assert!(depths > 20, "the history was walked ({depths})");
    };
    check(&t, &WS);
    let narrow = WinSize {
        ws_col: 6,
        ws_row: 16,
    };
    let _ = t.resize(&narrow);
    check(&t, &narrow);
}

/// **A full-screen program's rows have no marks**: the prompts are the
/// primary's, and on the alternate screen nothing is drawn in the strip.
#[test]
fn the_alternate_screen_has_no_marks() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces, &WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SESSION);
    r.set_status_marks(StatusMarks::On);
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    assert!(!r.strip_records().is_empty(), "positive control");
    feed(&mut t, b"\x1b[?1049h");
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    assert!(r.strip_records().is_empty());
    feed(&mut t, b"\x1b[?1049l");
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    assert_eq!(r.strip_records().len(), 7, "back on the primary");
}

/// **A smooth scroll's leaving line keeps its mark**, a row above the
/// window, and every mark is lowered by the pan, as the cell path draws
/// the grid.
#[test]
fn a_smooth_scroll_carries_the_marks_and_the_leaving_lines() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces, &WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    history(&mut t, 20);
    r.set_status_marks(StatusMarks::On);
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    let still: Vec<i32> = r.strip_records().iter().map(|i| i32::from(i.y.0)).collect();
    let pan = Px::of(5);
    r.place_marks(&t, Rows::ZERO, Leaving::Drawn, pan);
    let slid: Vec<i32> = r.strip_records().iter().map(|i| i32::from(i.y.0)).collect();
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    // The live screen's top line is `more` of a command whose prompt is
    // two lines up; the line above it is that command's `out`, no prompt.
    let above = prompt_rows(&t, 1).first() == Some(&0);
    let lowered: Vec<i32> = still.iter().map(|y| y + 5).collect();
    match above {
        true => {
            assert_eq!(slid[0], -ch + 5);
            assert_eq!(&slid[1..], &lowered[..]);
        }
        false => assert_eq!(slid, lowered),
    }
    // A window whose leaving line is a prompt's: one line further back.
    let back = (1..40)
        .find(|&b| prompt_rows(&t, b + 1).first() == Some(&0))
        .unwrap();
    r.place_marks(&t, Rows::of_count(back), Leaving::Drawn, pan);
    let tops: Vec<i32> = r.strip_records().iter().map(|i| i32::from(i.y.0)).collect();
    assert_eq!(
        tops[0],
        -ch + 5,
        "the leaving line's mark, above the window"
    );
}

/// A canvas the size of the renderer's whole frame, strip included.
fn whole(r: &Renderer<'_, '_>) -> (Vec<u32>, usize, usize) {
    let (w, h) = r.whole_size();
    let (w, h) = (
        usize::try_from(w.value()).unwrap(),
        usize::try_from(h.value()).unwrap(),
    );
    (vec![0; w * h], w, h)
}

fn paint(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    px: &mut [u32],
    (w, h): (usize, usize),
    damage: Damage,
) {
    let (ww, hh) = (
        Px::of(u32::try_from(w).unwrap()),
        Px::of(u32::try_from(h).unwrap()),
    );
    let mut target = Target::new(px, ww, ww, hh).unwrap();
    r.render_at(t, Rows::ZERO, &mut target, damage);
}

/// **On the CPU path the strip is the clear colour and the marks; the grid
/// is the frame it was, a strip to the right.** With the setting off the
/// frame is the base's to the pixel. A mark's pixels change when its status
/// changes, and the strip is repainted for it on an incremental frame.
#[test]
fn the_cpu_path_paints_the_strip_beside_the_grid() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SESSION);
    // The base: a renderer that never heard of the marks.
    let mut base = renderer(&faces, &WS);
    let (mut b, bw, bh) = whole(&base);
    paint(&mut base, &mut t, &mut b, (bw, bh), Damage::Full);
    // Off: the same frame, pixel for pixel.
    let mut off = renderer(&faces, &WS);
    off.set_status_marks(StatusMarks::Off);
    off.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    let (mut o, ow, oh) = whole(&off);
    assert_eq!((ow, oh), (bw, bh));
    paint(&mut off, &mut t, &mut o, (ow, oh), Damage::Full);
    assert!(o == b, "off draws the base's frame");
    // On.
    let mut on = renderer(&faces, &WS);
    on.set_status_marks(StatusMarks::On);
    on.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    let (mut p, w, h) = whole(&on);
    let s = usize::try_from(on.strip().value()).unwrap();
    assert_eq!(w, bw + s);
    paint(&mut on, &mut t, &mut p, (w, h), Damage::Full);
    for y in 0..h {
        assert_eq!(
            &p[y * w + s..y * w + w],
            &b[y * bw..y * bw + bw],
            "grid row {y}"
        );
    }
    let clear = on.clear_color().0;
    let ch = usize::try_from(on.metrics().cell_h.value()).unwrap();
    let strip_row = |p: &[u32], row: usize| -> Vec<u32> {
        (row * ch..(row + 1) * ch)
            .flat_map(|y| p[y * w..y * w + s].to_vec())
            .collect()
    };
    for row in 0..16 {
        let px = strip_row(&p, row);
        let marked = [0, 2, 4, 6, 8, 9, 11].contains(&row);
        assert_eq!(px.iter().any(|&c| c != clear), marked, "row {row}");
    }
    // The open prompt's command ends in failure: its row gains a mark on
    // an incremental frame.
    assert!(strip_row(&p, 12).iter().all(|&c| c == clear));
    feed(&mut t, b"x\x1b]133;C\x07\r\n\x1b]133;D;2\x07");
    on.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    paint(&mut on, &mut t, &mut p, (w, h), Damage::Incremental);
    assert_eq!(strip_row(&p, 12), strip_row(&p, 2), "a failure as row 2's");
}

/// **The shape carries the state; the colour only repeats it**: with the
/// palette's success and failure entries made one colour (`OSC 4`), a
/// success mark's pixels and a failure mark's still differ, and a cancelled
/// one's from both.
#[test]
fn the_marks_differ_without_their_colours() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SESSION);
    feed(
        &mut t,
        b"\x1b]4;1;rgb:80/80/80;4;rgb:80/80/80;8;rgb:80/80/80\x07",
    );
    let mut r = renderer(&faces, &WS);
    r.set_status_marks(StatusMarks::On);
    r.place_marks(&t, Rows::ZERO, Leaving::None, Px::of(0));
    let colours: Vec<_> = r.strip_records().iter().map(|i| i.color).collect();
    assert!(
        colours.windows(2).all(|c| c[0] == c[1]),
        "one colour: {colours:?}"
    );
    let (mut p, w, h) = whole(&r);
    paint(&mut r, &mut t, &mut p, (w, h), Damage::Full);
    let s = usize::try_from(r.strip().value()).unwrap();
    let ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
    let strip_row = |row: usize| -> Vec<u32> {
        (row * ch..(row + 1) * ch)
            .flat_map(|y| p[y * w..y * w + s].to_vec())
            .collect()
    };
    let (success, failure, cancelled) = (strip_row(0), strip_row(2), strip_row(8));
    assert_ne!(success, failure);
    assert_ne!(success, cancelled);
    assert_ne!(failure, cancelled);
    assert_eq!(
        success,
        strip_row(4),
        "positive control: one shape, one picture"
    );
}
