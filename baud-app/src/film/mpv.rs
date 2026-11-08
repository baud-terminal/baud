//! **mpv's kitty video, replayed**: `tests/recordings/kitty/mpv-logo-40x6.raw`,
//! what mpv 0.41.0 `--vo=kitty` wrote to a 40x6 pty of 280x96 pixels
//! playing the README's logo (`tests/recordings/README.md`), fed to a
//! terminal set up as the film sets one up -- the Linux driver's
//! `host::set_up_term`, the app's kitty file reader, the config's image
//! budget -- a read at a time, each picture's deferred decode run out, and
//! drawn by the CPU renderer at the end of every upload. Neither mpv nor
//! ffmpeg runs: the oracle is the RGB in the capture's own chunks, decoded
//! by [`capture`] with nothing of baud's.

use super::*;
use baud_domain::{Cols, ImageId, Rows};
use capture::{At, Pixels};

pub(super) mod capture;

/// A read the replay feeds: the tty on macOS hands a `read(2)` 1,024
/// bytes at most, and the capture arrived in 696 reads.
const READ: usize = 1024;

/// **The capture's window**: 40x6 cells of 7x16 pixels, `font_size = 6`
/// at the film's two pixels a point, the 280x96 mpv sized its frames to.
const GRID: &str = "40x6";
const CONFIG: &str = "font_size = 6\n";

/// **Feed `bytes` up to `to`** as the drivers feed a read, each kitty
/// decode run out after it, and what the core owes into `replies`.
fn feed_to(term: &mut Term<'_>, bytes: &[u8], fed: &mut At, to: At, replies: &mut Vec<u8>) {
    while *fed < to {
        let end = At((fed.0 + READ).min(to.0));
        crate::reads::feed(
            term,
            [&bytes[fed.0..end.0]],
            |o| replies.extend_from_slice(o),
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        while crate::reads::decode_step(term).0 == DecodeProgress::More {}
        crate::reads::owed(term, &mut |o: &[u8]| replies.extend_from_slice(o));
        *fed = end;
    }
}

/// The screen's rows as text, each padded to the grid's width.
fn rows(term: &Term<'_>, cols: Cols) -> Vec<String> {
    let cols = cols.index();
    crate::programs::screen(term)
        .into_iter()
        .map(|r| format!("{r:<cols$}"))
        .collect()
}

/// **Row 0 as ECMA-48's `EL 1` leaves it**: each status line mpv wrote
/// before the first cut, in order, on a row the `ED 2` before the first
/// frame blanked. The first is written at the cursor, column
/// [`capture::COL`], where the frame's `CUP` and its `C=1` leave it; each
/// after it follows `ESC [ 1 K` -- "the character positions from the
/// beginning of the line up to and including the active presentation
/// position are put into the erased state" (`[[source: ecma48]]` § 8.3.41,
/// `Ps = 1`) -- and a `CR`, so it is written from column 0 and what the
/// one before had past it stays.
fn status_row(statuses: &[capture::Status], before: At, cols: Cols) -> String {
    let mut row = vec![' '; cols.index()];
    for s in statuses.iter().filter(|s| s.at < before) {
        let from = match s.erased {
            true => {
                row[..=capture::COL].fill(' ');
                0
            }
            false => capture::COL,
        };
        for (i, ch) in s.text.chars().enumerate() {
            row[from + i] = ch;
        }
    }
    row.into_iter().collect()
}

/// **Where a cut's text lands**: written from row 0, column 0 -- the
/// status's `CR` -- wrapping at the last column and scrolling at the last
/// row. Returns the rows it fills whole, as `(row, text)`, and for text
/// that fits, how much of the screen read row after row it is the start
/// of.
fn landed(text: &str, cols: Cols, rows: Rows) -> (Vec<(usize, String)>, Option<String>) {
    let (cols, rows) = (cols.index(), rows.index());
    let chars: Vec<char> = text.chars().collect();
    let lines: Vec<String> = chars.chunks(cols).map(|c| c.iter().collect()).collect();
    match lines.len() <= rows {
        true => (Vec::new(), Some(text.to_string())),
        false => {
            let last = &lines[lines.len() - rows..];
            let full = last
                .iter()
                .enumerate()
                .map(|(r, l)| (r, format!("{l:<cols$}")))
                .collect();
            (full, None)
        }
    }
}

/// How many pixels of the `w` by `h` region at `(x0, y0)` of a frame
/// `fw` wide are not the opaque RGB `rgb`.
fn differing(
    frame: &[u32],
    Pixels(fw): Pixels,
    (x0, y0): (usize, usize),
    (w, h): (usize, usize),
    rgb: &[u8],
) -> usize {
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let o = 3 * (y * w + x);
            let want = 0xFF00_0000
                | u32::from(rgb[o]) << 16
                | u32::from(rgb[o + 1]) << 8
                | u32::from(rgb[o + 2]);
            frame[(y0 + y) * fw + x0 + x] != want
        })
        .count()
}

/// **mpv's in-stream video draws exactly what mpv sent, and its two
/// interrupted frames and its status line are what the standards make of
/// them.** At the end of each of the capture's thirteen uploads:
///
/// - **every complete frame draws its own RGB**, all 13,248 pixels, at
///   the cell mpv put it in: `f=24` is opaque (`[[source:
///   kitty-graphics]]`, "RGB and RGBA data"). Store `f=24` with alpha 0
///   and the page shows through: all 13,248 pixels red at the first
///   upload.
/// - **no bright pixel draws faint**, the symptom a faint picture shows:
///   every source pixel brighter than luminance 200 (`capture::BRIGHT`)
///   draws brighter than 200, checked before the exact RGB so its break
///   names it. The same alpha-0 break: red at the first upload.
/// - **frames pile up, newest on top, and the quota deletes none**: mpv
///   sends no id and no delete between frames, so each `a=T` is a new
///   image with a placement of its own at the same cell, and kitty's
///   storage quota ("Image persistence and storage quotas") is far above
///   eleven 138x96 frames at the app's budget, so `images_evicted` stays
///   0. The newest is drawn: images that tie on `z` and name no id draw
///   in the order they were stored, as kitty draws them, which
///   [`under_a_two_frame_quota_every_upload_shows_its_own_frame`] holds
///   where the quota deletes frames. Reverse the draw order and the oldest
///   frame is on top: red at the third upload, the first whose frame
///   differs from the first's (the first two are the same picture), which
///   the test requires happen.
/// - **an `ESC` inside a chunk cancels its upload** (`[[source:
///   williams]]`), counted in `KittyCounts::cancelled`, and what the chunk
///   had left -- mpv's status line and the base64 after it -- prints as
///   text where `CR` put it, derived from the capture; the frame before
///   stays on top. The chunks mpv sent after it continue an upload that
///   no longer exists, so they are a new command with no size, refused as
///   empty (`ImageRefusals::empty`) and answered nothing, since it names
///   no image. Stop counting the cancel and the count is red at the first
///   cut.
/// - **the status line keeps what `EL 1` leaves**: mpv moves the cursor
///   to the frame's cell before it redraws its status with `ESC [ 1 K CR`,
///   so the erase stops at column 9 and a tail of the longer line before
///   survives, e.g. `V: 00:00:00 / 00:00:01 (8%)0:01 (0%)`. Row 0 is held
///   to [`status_row`] at every upload before the first cut, and a tail
///   must survive at one of them. Make `EL 1` erase the whole line and
///   the tail is gone: red at the fourth upload.
/// - **nothing is left at the end**: the last cut's text, 81 lines, scrolls
///   every frame off the alternate screen, where nothing is kept above
///   the top, so each placement expires and its image, which no id names,
///   goes with it; the quit's `a=d` is cut by the `ESC` of `ESC [ ? 25 h`
///   and deletes nothing. Every image the uploads reserved is freed --
///   eleven with their placements, two by their cancel -- and none the
///   frames were stored as is live.
#[test]
fn mpvs_kitty_video_draws_what_mpv_sent() {
    let bytes = std::fs::read(capture::PATH).expect("the capture");
    let ups = capture::uploads(&bytes);
    let cut_at: Vec<usize> = (0..ups.len()).filter(|&k| ups[k].cut.is_some()).collect();
    assert_eq!(
        (ups.len(), cut_at),
        (13, vec![10, 12]),
        "the capture's uploads"
    );
    let f = Film {
        child: Child::Command("true"),
        env: Vec::new(),
        grid: grid_of(GRID),
        config: CONFIG,
        source: None,
        period: Duration::from_millis(40),
        limit: Duration::from_secs(10),
    };
    let (cols, nrows) = (
        Cols::of_index(usize::from(f.grid.ws_col)),
        Rows::of_index(usize::from(f.grid.ws_row)),
    );
    let mut stage = Stage::new(&f);
    let m = stage.renderer.metrics();
    assert_eq!(
        (m.cell_w.value() * 40, m.cell_h.value() * 6),
        (280, 96),
        "the premise: the capture's window in pixels"
    );
    let mut owned = Owned::new(&f.grid).with_images(stage.cfg.images);
    let mut term = stage.term(Term::new(owned.storage(), &f.grid));
    let (gw, gh) = stage.renderer.whole_size();
    let slots = |p: Px| usize::try_from(p.value()).expect("a size");
    let fw = slots(gw);
    let mut frame = vec![0u32; fw * slots(gh)];
    let fw = Pixels(fw);
    let first_cut = ups.iter().find_map(|u| u.cut.as_ref()).expect("a cut").at;
    let statuses = capture::statuses(&bytes, first_cut);
    let (mut fed, mut replies) = (At(0), Vec::new());
    let (mut complete, mut cuts, mut tails, mut over_first) = (0usize, 0usize, 0usize, 0usize);
    let mut bright = 0usize;
    let mut shown: Option<&[u8]> = None;
    let mut first: Option<&[u8]> = None;
    let mut origin: Option<(i16, i16)> = None;
    let mut stored: Vec<ImageId> = Vec::new();
    for (k, up) in ups.iter().enumerate() {
        feed_to(&mut term, &bytes, &mut fed, up.end, &mut replies);
        {
            let back = term.window_back();
            let mut target = Target::new(&mut frame, gw, gw, gh).expect("the frame holds the grid");
            stage
                .renderer
                .render_at(&mut term, back, &mut target, Damage::Full);
        }
        let places = stage.renderer.image_placements().to_vec();
        let counts = term.kitty();
        let store = term.images();
        let screen = rows(&term, cols);
        match (&up.rgb, &up.cut) {
            (Some(rgb), None) => {
                complete += 1;
                assert_eq!(counts.images, complete, "upload {k}: stored");
                assert_eq!(
                    store.placements().count(),
                    complete,
                    "upload {k}: every frame placed"
                );
                assert_eq!(
                    store.images_evicted, 0,
                    "upload {k}: the quota deleted a frame"
                );
                stored.extend(store.placements().map(|(_, p)| p.image));
                if first.is_some_and(|f| f != rgb.as_slice()) {
                    over_first += 1;
                }
                first.get_or_insert(rgb.as_slice());
                shown = Some(rgb.as_slice());
            }
            (None, Some(c)) => {
                cuts += 1;
                assert_eq!(
                    counts.images, complete,
                    "upload {k}: a cut upload stored nothing"
                );
                assert_eq!(counts.cancelled, cuts, "upload {k}: the cancel is counted");
                assert_eq!(
                    (counts.refused, store.images_refused, store.refusals.empty),
                    (cuts, cuts, cuts),
                    "upload {k}: the chunks after the cut, refused as a command with no size"
                );
                assert_eq!(
                    store.placements().count() + store.placements_expired,
                    complete,
                    "upload {k}: no placement made or deleted but by the scroll"
                );
                let text = format!("{}{}", c.status, c.rest);
                match landed(&text, cols, nrows) {
                    (_, Some(text)) => {
                        let joined: String = screen.concat();
                        assert_eq!(
                            &joined[..text.len()],
                            text,
                            "upload {k}: the status and the chunk's rest, as text from row 0"
                        );
                    }
                    (full, None) => {
                        for (r, line) in full {
                            assert_eq!(screen[r], line, "upload {k}: row {r} of the cut's text");
                        }
                    }
                }
            }
            _ => unreachable!("an upload has its RGB or its cut"),
        }
        if up.end < first_cut {
            let want = status_row(&statuses, up.end, cols);
            let last = statuses.iter().rev().find(|s| s.at < up.end);
            if last.is_some_and(|s| want.trim_end().len() > s.text.len()) {
                tails += 1;
            }
            assert_eq!(screen[0], want, "upload {k}: row 0 as EL 1 leaves it");
        }
        match (places.is_empty(), shown) {
            (false, Some(rgb)) => {
                let (Pixels(s), Pixels(v)) = up.size;
                let at = (places[0].x, places[0].y);
                for p in &places {
                    assert_eq!(
                        ((p.x, p.y), usize::from(p.w), usize::from(p.h)),
                        (at, s, v),
                        "upload {k}: every frame at one cell, whole"
                    );
                }
                assert_eq!(
                    *origin.get_or_insert(at),
                    at,
                    "upload {k}: the frames' cell"
                );
                let xy = (
                    usize::try_from(at.0).unwrap(),
                    usize::try_from(at.1).unwrap(),
                );
                let (lit, dim) = capture::faint(&frame, fw, xy, (s, v), rgb);
                bright += lit;
                assert_eq!(
                    dim, 0,
                    "upload {k}: of {lit} bright pixels, {dim} drawn faint"
                );
                let wrong = differing(&frame, fw, xy, (s, v), rgb);
                assert_eq!(wrong, 0, "upload {k}: pixels not the newest frame's RGB");
            }
            (true, _) => assert_eq!(store.placements().count(), 0, "upload {k}: drawn none"),
            (false, None) => panic!("upload {k}: a frame drawn before any was sent"),
        }
    }
    assert!(
        over_first > 0,
        "the premise: a frame unlike the first is drawn on it"
    );
    assert!(bright > 0, "the premise: the frames have bright pixels");
    assert!(
        tails > 0,
        "the premise: a status line's tail survives an EL 1"
    );
    feed_to(&mut term, &bytes, &mut fed, At(bytes.len()), &mut replies);
    let store = term.images();
    assert_eq!(
        term.kitty().images,
        complete,
        "a frame stored after the last"
    );
    assert_eq!(
        store.placements().count(),
        0,
        "a placement outlived the stream"
    );
    assert_eq!(
        (
            store.placements_expired,
            store.images_dropped,
            store.images_evicted
        ),
        (complete, ups.len(), 0),
        "every frame expired with its rows, every reserve freed"
    );
    stored.sort();
    stored.dedup();
    assert_eq!(stored.len(), complete, "one image a frame");
    for id in stored {
        assert!(store.image(id).is_none(), "image {id:?} is still live");
    }
    assert!(
        replies.is_empty(),
        "answered {replies:?}: q=2, and no id to name"
    );
}

/// **Under a quota of two frames every upload shows its own frame**: the
/// capture replayed with an image budget of two of mpv's frames, so from
/// the third upload on each new frame deletes the oldest for space
/// (`[[source: kitty-graphics]]`, "Image persistence and storage quotas":
/// "delete older images to make space for the new one"), and its
/// placement takes the slot the deleted one's held. The frames tie on `z`
/// and name no id, where the text leaves the order undefined ("If the
/// images have the same z-index and the same id, then the behavior is
/// undefined"); kitty draws the image stored later over the one stored
/// before (`[[source: kitty-graphics-c]]`, the render sort's `lt`: by
/// `z_index`, then `image_id`, the image's internal id in the order
/// images were made, then `ref_id`), and so does baud. At the end of each
/// complete upload the frame drawn is that upload's RGB, and at a cut one
/// the frame before it.
///
/// The break: today's slot order. Draw ties in the order the placement
/// table holds them and the frame placed into a freed low slot goes
/// under the one before it: red at the third upload, which draws the
/// second's frame.
#[test]
fn under_a_two_frame_quota_every_upload_shows_its_own_frame() {
    let bytes = std::fs::read(capture::PATH).expect("the capture");
    let ups = capture::uploads(&bytes);
    let f = Film {
        child: Child::Command("true"),
        env: Vec::new(),
        grid: grid_of(GRID),
        config: CONFIG,
        source: None,
        period: Duration::from_millis(40),
        limit: Duration::from_secs(10),
    };
    let mut stage = Stage::new(&f);
    let (Pixels(s), Pixels(v)) = ups[0].size;
    let budget = baud_domain::ImageBudget::of(2 * s * v + 64);
    let mut owned = Owned::new(&f.grid).with_images(budget);
    let mut term = stage.term(Term::new(owned.storage(), &f.grid));
    let (gw, gh) = stage.renderer.whole_size();
    let slots = |p: Px| usize::try_from(p.value()).expect("a size");
    let fw = slots(gw);
    let mut frame = vec![0u32; fw * slots(gh)];
    let fw = Pixels(fw);
    let (mut fed, mut replies) = (At(0), Vec::new());
    let mut shown: Option<&[u8]> = None;
    // The complete uploads whose frame is the one before's, which a wrong
    // order could not be told from.
    let mut repeats: Vec<usize> = Vec::new();
    let mut drawn = 0usize;
    for (k, up) in ups.iter().enumerate() {
        feed_to(&mut term, &bytes, &mut fed, up.end, &mut replies);
        {
            let back = term.window_back();
            let mut target = Target::new(&mut frame, gw, gw, gh).expect("the frame holds the grid");
            stage
                .renderer
                .render_at(&mut term, back, &mut target, Damage::Full);
        }
        if let Some(rgb) = &up.rgb {
            if shown == Some(rgb.as_slice()) {
                repeats.push(k);
            }
            shown = Some(rgb.as_slice());
        }
        let places = stage.renderer.image_placements().to_vec();
        let (Some(p), Some(rgb)) = (places.first(), shown) else {
            continue;
        };
        let xy = (usize::try_from(p.x).unwrap(), usize::try_from(p.y).unwrap());
        let wrong = differing(&frame, fw, xy, (s, v), rgb);
        assert_eq!(wrong, 0, "upload {k}: not the newest frame's RGB");
        drawn += 1;
    }
    assert!(
        term.images().images_evicted >= 8,
        "the premise: the quota deleted frames, {} of them",
        term.images().images_evicted
    );
    assert_eq!(drawn, 12, "the premise: every upload but the last drawn");
    assert_eq!(
        repeats,
        vec![1],
        "the premise: every frame differs from the one before but the second"
    );
}

/// **The demo's prompts after mpv's exit**: each shell's mark is a
/// BEL-ended `OSC 133 ; A`, after the `OSC 133 ; D` of the command before
/// it -- zsh's, baud's integration's, `ESC ] 133 ; A ; aid=PID BEL`
/// (`baud-app/src/shell/zshenv.zsh`), and the container's bash's,
/// `ESC ] 133 ; A ; BEL` (`nix/demo-container/profile`) -- and after the
/// capture's quit, whose `a=d` the `ESC` of `ESC [ ? 25 h` cuts, either
/// one, fed whole, finds the primary screen up, `?1049` off, and opens a
/// prompt there. Then the case with no `ESC` after the quit's `a=d`, the
/// capture cut there: the prompt's own first byte ends the open string, so
/// the mark that follows is read as one, here on the alternate screen
/// nothing has left. The control: a prompt with no mark opens none.
/// Perturbation: feed the shell's prompt as plain text and no prompt
/// opens.
#[test]
fn the_demos_prompts_after_mpvs_exit_are_bel_ended_osc_133_marks() {
    let zshenv = include_str!("../shell/zshenv.zsh");
    for spelled in [
        "__baud_bel=$'\\a'",
        "${__baud_osc}D;$st;aid=$$${__baud_bel}",
        "${__baud_osc}A;aid=$$${__baud_bel}",
    ] {
        assert!(zshenv.contains(spelled), "zsh's mark: {spelled}");
    }
    let profile = include_str!("../../../nix/demo-container/profile");
    assert!(
        profile.contains("printf '\\033]133;D;%s\\a\\033]133;A;\\a'"),
        "bash's mark in the container"
    );
    let zsh: &[u8] = b"\x1b]133;D;0;aid=4242\x07\x1b]133;A;aid=4242\x07~/baud \xe2\x9d\xaf ";
    let bash: &[u8] = b"\x1b]133;D;0\x07\x1b]133;A;\x07~ \xe2\x9d\xaf ";
    let bytes = std::fs::read(capture::PATH).expect("the capture");
    let quit = b"\x1b_Ga=d;";
    let cut = bytes
        .windows(quit.len())
        .rposition(|w| w == quit)
        .map(|i| i + quit.len())
        .expect("the quit's a=d");
    assert_eq!(
        &bytes[cut..cut + 6],
        b"\x1b[?25h",
        "the premise: an ESC follows the quit"
    );
    let grid = grid_of(GRID);
    let fed = |parts: &[&[u8]]| {
        let mut owned = Owned::new(&grid);
        let mut term = Term::new(owned.storage(), &grid);
        let before = term.prompts_opened();
        for p in parts {
            for chunk in p.chunks(1024) {
                crate::reads::feed(
                    &mut term,
                    [chunk],
                    |_| {},
                    |_| {},
                    &mut crate::hostlog::NoLog,
                );
            }
        }
        (term.screen(), term.prompts_opened() != before, term.zone())
    };
    for prompt in [zsh, bash] {
        assert_eq!(
            fed(&[&bytes, prompt]),
            (baud_core::Screen::Normal, true, baud_core::Zone::Prompt),
            "after mpv's whole exit: {}",
            String::from_utf8_lossy(prompt)
        );
        let (screen, _, zone) = fed(&[&bytes[..cut], prompt]);
        assert_eq!(
            (screen, zone),
            (baud_core::Screen::Alternate, baud_core::Zone::Prompt),
            "the string left open, then the prompt: {}",
            String::from_utf8_lossy(prompt)
        );
    }
    let (_, opened, _) = fed(&[&bytes, b"~/baud \xe2\x9d\xaf "]);
    assert!(!opened, "the control: a prompt with no mark opens none");
}
