//! **A window narrowed at a shell's prompt draws the listing above it at
//! its words** (`[screen] wrap_words` and `wrap_marker`, both on by
//! default): the frame the CPU renderer draws after each step of a
//! narrowing, in one step and in a drag, and after the window widens back.
//!
//! The screen is a long listing as a long-format `ls` prints it -- an
//! underlined header padded past the width, coloured columns, a long name,
//! a symbolic link whose target is longer than the line -- under the
//! command that printed it, and a three-row prompt whose first row carries a
//! segment at the last column. Written at eighty columns, narrowed to forty.
//!
//! What every frame is held to: each character of the listing is drawn, in
//! reading order -- nothing clipped at the edge, nothing left past it; no
//! word but a hyphenated name wider than the line is split, and a path is
//! cut before its slashes; the lines the display re-wrapped carry the mark
//! in the margin; the prompt is drawn on the cursor's row. Widened back,
//! the frame is the one drawn before the narrowing.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{
    Blend, Damage, Instance, Leaving, PxSize, Renderer, Stack, Target, Theme, WordWrap,
};
use support::font;

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    t.clear_reply();
}

const ROWS: u16 = 30;

fn at(cols: u16) -> WinSize {
    WinSize {
        ws_col: cols,
        ws_row: ROWS,
    }
}

/// The margin each side lends: a Retina display's 8 pt padding.
const BAND: Px = Px::of(16);

/// The prompt: its mark, a first row with a segment at the last column, a
/// directory row, and `$ `, then the input's mark.
fn prompt() -> String {
    "\x1b]133;A\x07zsh\x1b[80G\u{2588}\r\n/tmp/listing\r\n\x1b[32m$\x1b[0m \x1b]133;B\x07".into()
}

/// The symbolic link's target: longer than the narrow line.
const TARGET: &str = "/home/jordan/Library/CloudStorage/Shared-ExampleTeamSpace-Projects";

/// The listing's lines as they read, and the bytes that print them.
fn listing() -> (Vec<String>, String) {
    let u = |s: &str| format!("\x1b[4m{s}\x1b[24m");
    let header = [
        "Permissions",
        "User  ",
        "Group",
        "Size",
        "Date Modified    ",
        &format!("Name{}", " ".repeat(60)),
    ];
    let mut text = vec![header.join(" ")];
    let mut bytes = format!(
        "{}\r\n",
        header.iter().map(|h| u(h)).collect::<Vec<_>>().join(" ")
    );
    for (kind, name, shown) in [
        (
            "drwxr-xr-x",
            "\x1b[1;34mApplications\x1b[0m/".to_string(),
            "Applications/".to_string(),
        ),
        (
            "drwxr-xr-x",
            "\x1b[1;34mpre-nix-darwin-20260803-201530-backup\x1b[0m/".to_string(),
            "pre-nix-darwin-20260803-201530-backup/".to_string(),
        ),
        (
            ".rw-r--r--",
            "a-rather-long-file-name-that-keeps-going-for-a-while.txt".to_string(),
            "a-rather-long-file-name-that-keeps-going-for-a-while.txt".to_string(),
        ),
        (
            "lrwxr-xr-x",
            format!("\x1b[1;36m'Shared - Example Team Space Projects'\x1b[0m -> {TARGET}"),
            format!("'Shared - Example Team Space Projects' -> {TARGET}"),
        ),
    ] {
        bytes.push_str(&format!(
            "\x1b[1;34m{kind}\x1b[0m jordan staff  64 B \x1b[32m26-11-04 21:23:07\x1b[0m {name}\r\n"
        ));
        text.push(format!(
            "{kind} jordan staff  64 B 26-11-04 21:23:07 {shown}"
        ));
    }
    (text, bytes)
}

/// The command typed at the prompt.
const COMMAND: &str = "cd /home/jordan/Library/CloudStorage/listing && ll";

/// `history` lines, a prompt, the command, the listing, the next prompt.
fn session(history: usize) -> String {
    let h: String = (0..history).map(|i| format!("h{i}\r\n")).collect();
    format!(
        "{h}{p}{COMMAND}\r\n\x1b]133;C\x07{l}\x1b]133;D;0\x07{p}",
        p = prompt(),
        l = listing().1
    )
}

fn renderer<'f, 'a>(faces: &'f [Face<'a>], ws: &WinSize) -> Renderer<'f, 'a> {
    let mut r = Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    r.set_margins(BAND);
    r
}

/// A frame as the window shows it: each row as drawn, blanks trimmed, and
/// the rows with a mark in the left margin.
struct Frame {
    rows: Vec<String>,
    marked: Vec<usize>,
}

fn text(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|p| char::from_u32(p.value()))
                .unwrap_or(' ')
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// The frame drawn now, on the CPU, as the app's present draws one.
fn frame(r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Frame {
    r.fit(t);
    r.place_marks(t, Rows::ZERO, Leaving::None, Px::of(0));
    let (w, h) = r.whole_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    {
        let mut target = Target::new(&mut px, w, w, h).unwrap();
        r.render(t, &mut target, Damage::Full);
    }
    let rows = match r.bidi().display() {
        Some(d) => {
            let vp = d.window(t);
            (0..vp.rows().index())
                .map(|y| {
                    let row = Row::from_index(y);
                    let mut out = vec![PackedCell::EMPTY; vp.cols().index()];
                    r.bidi().permute(row, vp.row(row), &mut out);
                    text(&out)
                })
                .collect()
        }
        None => {
            let vp = t.view(Rows::ZERO);
            (0..vp.rows().index())
                .map(|y| text(vp.row(Row::from_index(y))))
                .collect()
        }
    };
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let band = i32::try_from(r.margins().value()).unwrap();
    let marked = r
        .margin_records()
        .iter()
        .filter(|i: &&Instance| i32::from(i.x.0) + i32::from(i.w.0) <= band)
        .map(|i| usize::try_from(i32::from(i.y.0).div_euclid(ch)).unwrap())
        .collect();
    Frame { rows, marked }
}

/// Every character but the spaces, in order.
fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The rows the listing is drawn on: from the header's first to the row
/// before the prompt after it.
fn listing_rows(f: &Frame) -> Vec<String> {
    let from = f
        .rows
        .iter()
        .position(|l| l.starts_with("Permissions"))
        .unwrap_or_else(|| panic!("the header is drawn: {:#?}", f.rows));
    let to = from
        + f.rows[from..]
            .iter()
            .position(|l| l.starts_with("zsh"))
            .unwrap_or_else(|| panic!("the prompt after the listing: {:#?}", f.rows));
    f.rows[from..to].to_vec()
}

/// What every frame at `cols` is held to.
fn holds(f: &Frame, t: &Term<'_>, cols: usize, step: &str) {
    let (lines, _) = listing();
    let drawn = listing_rows(f);
    let show = || format!("{step}:\n{}", f.rows.join("\n"));
    assert_eq!(
        squash(&drawn.join("")),
        squash(&lines.join("")),
        "every character of the listing drawn, in order -- {}",
        show()
    );
    assert!(
        drawn.iter().all(|l| l.chars().count() <= cols),
        "no row past the edge -- {}",
        show()
    );
    // No word split: each word of the listing but a hyphenated name and
    // the path is on one row.
    for word in lines.iter().flat_map(|l| l.split_whitespace()) {
        if word.contains('-') || word.contains('/') {
            continue;
        }
        assert!(
            drawn
                .iter()
                .any(|l| l.split_whitespace().any(|w| w == word)),
            "{word:?} whole on a row -- {}",
            show()
        );
    }
    // The cursor's row is the prompt's last, `$`.
    let cy = t.cursor().0.index();
    assert_eq!(
        f.rows[cy],
        "$",
        "the prompt on the cursor's row -- {}",
        show()
    );
}

/// The narrow frame's paths and marks: the link's target cut before its
/// slashes, every row it continues on with one starting with it; a mark beside
/// each row that continues a listing line, and none beside a line's first.
fn cut_and_marked(f: &Frame, cols: usize, step: &str) {
    let drawn = listing_rows(f);
    let from = f
        .rows
        .iter()
        .position(|l| l.starts_with("Permissions"))
        .unwrap();
    let show = || format!("{step}:\n{}", f.rows.join("\n"));
    let at = drawn
        .iter()
        .position(|l| l.contains("/home"))
        .unwrap_or_else(|| panic!("the target -- {}", show()));
    // Every row the target goes on to that holds a slash starts with one,
    // and no row of it ends with one: a hyphen inside a segment is a break
    // of its own (UAX #14's HY).
    let rest: Vec<&String> = drawn[at + 1..].iter().collect();
    assert!(
        rest.first().is_some_and(|l| l.starts_with('/'))
            && rest.iter().all(|l| !l.contains('/') || l.starts_with('/'))
            && drawn[at..].iter().all(|l| !l.ends_with('/')),
        "the target cut before its slashes at {cols} -- {}",
        show()
    );
    let firsts = ["Permissions", "drwxr", ".rw-", "lrwxr"];
    for (k, l) in drawn.iter().enumerate() {
        let first = firsts.iter().any(|p| l.starts_with(p));
        assert_eq!(
            f.marked.contains(&(from + k)),
            !first,
            "row {} {l:?}: marked as a continued line -- {}",
            from + k,
            show()
        );
    }
}

/// **One step to forty columns, with history above**: the listing drawn at
/// its words, the target cut before its slashes, the marks beside every
/// continued line; widened back, the frame drawn before.
#[test]
fn a_listing_narrowed_at_a_prompt_is_drawn_at_its_words_in_one_step() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(at(80));
    let mut t = Term::new(owned.storage(), &at(80));
    let mut r = renderer(&faces, &at(80));
    assert_eq!(r.word_wrap(), WordWrap::On, "on by default");
    feed(&mut t, &session(60));
    let wide = frame(&mut r, &mut t);
    holds(&wide, &t, 80, "at eighty");
    let _ = t.resize(&at(40));
    let narrow = frame(&mut r, &mut t);
    holds(&narrow, &t, 40, "at forty");
    cut_and_marked(&narrow, 40, "at forty");
    let _ = t.resize(&at(80));
    let back = frame(&mut r, &mut t);
    assert_eq!(back.rows, wide.rows, "back at eighty, the frame before");
}

/// **A drag to forty in steps of three, and back**: every step's frame
/// held to the same, the last narrow one cut and marked.
#[test]
fn a_listing_narrowed_at_a_prompt_is_drawn_at_its_words_through_a_drag() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(at(80));
    let mut t = Term::new(owned.storage(), &at(80));
    let mut r = renderer(&faces, &at(80));
    feed(&mut t, &session(60));
    let wide = frame(&mut r, &mut t);
    for c in (40..80).rev().step_by(3).chain([40]) {
        let _ = t.resize(&at(c));
        let f = frame(&mut r, &mut t);
        holds(&f, &t, usize::from(c), &format!("in at {c}"));
    }
    let f = frame(&mut r, &mut t);
    cut_and_marked(&f, 40, "dragged to forty");
    for c in (40..=80).step_by(3).chain([80]) {
        let _ = t.resize(&at(c));
        let f = frame(&mut r, &mut t);
        holds(&f, &t, usize::from(c), &format!("out at {c}"));
    }
    assert_eq!(frame(&mut r, &mut t).rows, wide.rows, "the frame before");
}

/// **A fresh window, nothing above the first prompt**: the case a new
/// window is in when its first command lists a directory. Every character
/// of the listing is drawn, in order, nothing past the edge, at every step;
/// with no history above, a line the words would need one more row for
/// keeps the terminal's cut rather than push a row where nothing scrolls to
/// it, so the words are not asked of this frame.
#[test]
fn a_fresh_windows_listing_narrowed_at_a_prompt_loses_nothing() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(at(80));
    let mut t = Term::new(owned.storage(), &at(80));
    let mut r = renderer(&faces, &at(80));
    feed(&mut t, &session(0));
    let wide = frame(&mut r, &mut t);
    for c in (40..80).rev().step_by(3).chain([40]) {
        let _ = t.resize(&at(c));
        let f = frame(&mut r, &mut t);
        let (lines, _) = listing();
        let drawn = listing_rows(&f);
        assert_eq!(
            squash(&drawn.join("")),
            squash(&lines.join("")),
            "at {c}: every character drawn, in order:\n{}",
            f.rows.join("\n")
        );
        assert!(drawn.iter().all(|l| l.chars().count() <= usize::from(c)));
        assert_eq!(f.rows[t.cursor().0.index()], "$", "at {c}");
    }
    let _ = t.resize(&at(80));
    assert_eq!(frame(&mut r, &mut t).rows, wide.rows, "the frame before");
}
