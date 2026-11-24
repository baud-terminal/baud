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
use baud_domain::{Cols, ImageId};
use capture::{At, Pixels};

pub(super) mod capture;

/// A read the replay feeds: the tty on macOS hands a `read(2)` 1,024
/// bytes at most, and the capture arrived in 696 reads.
const READ: usize = 1024;

/// **The capture's window**: 40x6 cells of 7x16 pixels, `font_size = 6`
/// at the film's two pixels a point, the 280x96 mpv sized its frames to.
const GRID: &str = "40x6";
/// **The video films' config: an `APC G` ends only at `BEL` or `ESC \\`**
/// (`[images] kitty_string_end = "kitty"`), so a status line mpv writes
/// into a chunk is the chunk's payload and every upload is a frame, short
/// or whole. By default the status line cancels the chunk and prints as
/// text, `baud-core`'s
/// `a_status_line_inside_a_chunk_shows_as_text_by_default_and_not_under_the_setting`
/// on the recording of one such chunk.
const CONFIG: &str = "font_size = 6\n[images]\nkitty_string_end = \"kitty\"\n";

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

/// How many pixels of the `w` by `h` region at `(x0, y0)` of a frame
/// `fw` wide are not the opaque RGB `rgb`.
fn differing(
    frame: &[u32],
    Pixels(fw): Pixels,
    (Pixels(x0), Pixels(y0)): (Pixels, Pixels),
    (Pixels(w), Pixels(h)): (Pixels, Pixels),
    rgb: &[u8],
) -> Pixels {
    let n = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let o = 3 * (y * w + x);
            let want = 0xFF00_0000
                | u32::from(rgb[o]) << 16
                | u32::from(rgb[o + 1]) << 8
                | u32::from(rgb[o + 2]);
            frame[(y0 + y) * fw + x0 + x] != want
        })
        .count();
    Pixels(n)
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
    let cols = Cols::of_index(usize::from(f.grid.ws_col));
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
    // The status lines mpv wrote inside a chunk are its payload, the
    // setting's reading (`find_st_terminator`), and draw nothing: row 0 is what the
    // ones outside a chunk leave, at every upload.
    let inside: Vec<At> = ups
        .iter()
        .filter_map(|u| u.cut.as_ref().map(|c| c.at))
        .collect();
    let statuses: Vec<capture::Status> = capture::statuses(&bytes, At(bytes.len()))
        .into_iter()
        .filter(|s| !inside.iter().any(|c| s.at.0 == c.0 + b"\x1b[1K\r".len()))
        .collect();
    assert_eq!(
        capture::statuses(&bytes, At(bytes.len())).len() - statuses.len(),
        inside.len(),
        "the premise: each cut's status line is one the capture holds"
    );
    let (mut fed, mut replies) = (At(0), Vec::new());
    let mut before: Option<Vec<String>> = None;
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
            let seal = term.seal_writes();
            stage
                .renderer
                .render_at(&term, seal, back, &mut target, Damage::Full);
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
            // **A cut upload is refused short**: the
            // status line and the base64 after it are the chunk's payload,
            // its data ends at the status's `ESC`, and the upload comes up
            // short at its `m=0`, "ENODATA", unsaid under `q=2`. Nothing is
            // cancelled, nothing is printed, and no frame is made.
            (None, Some(c)) => {
                cuts += 1;
                assert_eq!(
                    counts.images, complete,
                    "upload {k}: a cut upload stored nothing"
                );
                assert_eq!(counts.cancelled, 0, "upload {k}: nothing cancelled");
                assert_eq!(counts.refused, cuts, "upload {k}: refused short, once");
                assert_eq!(
                    store.placements().count() + store.placements_expired,
                    complete,
                    "upload {k}: no placement made or deleted but by the scroll"
                );
                let rows_after = before.as_ref().map(|b| b[1..].to_vec());
                assert_eq!(
                    Some(screen[1..].to_vec()),
                    rows_after,
                    "upload {k}: the cut {:?} drew text",
                    c.status
                );
                let joined: String = screen.concat();
                for (what, text) in [("status", &c.status), ("base64", &c.rest)] {
                    let head: String = text.chars().take(cols.index()).collect();
                    assert!(
                        head.len() > 8 && !joined.contains(&head),
                        "upload {k}: the cut's {what} {head:?} drawn"
                    );
                }
            }
            _ => unreachable!("an upload has its RGB or its cut"),
        }
        before = Some(screen.clone());
        {
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
                    Pixels(usize::try_from(at.0).unwrap()),
                    Pixels(usize::try_from(at.1).unwrap()),
                );
                let (lit, dim) = capture::faint(&frame, fw, xy, up.size, rgb);
                bright += lit;
                assert_eq!(
                    dim, 0,
                    "upload {k}: of {lit} bright pixels, {dim} drawn faint"
                );
                let wrong = differing(&frame, fw, xy, up.size, rgb);
                assert_eq!(
                    wrong,
                    Pixels(0),
                    "upload {k}: pixels not the newest frame's RGB"
                );
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
    // **The end of the stream is where the setting leaves it.** mpv's last write
    // is `ESC _ G a = d ;` with no terminator, so the `?1003l` and
    // `?1049l` after it are that command's payload under the setting
    // (`mpvs_unterminated_exit_leaves_the_modes_set_as_kitty_does`): the
    // alternate screen stays up and the `a=d` deletes nothing.
    feed_to(&mut term, &bytes, &mut fed, At(bytes.len()), &mut replies);
    assert_eq!(
        term.screen(),
        baud_core::Screen::Alternate,
        "the exit's ?1049l is payload"
    );
    let store = term.images();
    assert_eq!(
        store.placements().count() + store.placements_expired,
        complete,
        "the open a=d deleted nothing"
    );
    assert_eq!(
        term.kitty().images,
        complete,
        "a frame stored after the last"
    );
    stored.sort();
    stored.dedup();
    assert_eq!(stored.len(), complete, "one image a frame");
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
            let seal = term.seal_writes();
            stage
                .renderer
                .render_at(&term, seal, back, &mut target, Damage::Full);
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
        let xy = (
            Pixels(usize::try_from(p.x).unwrap()),
            Pixels(usize::try_from(p.y).unwrap()),
        );
        let wrong = differing(&frame, fw, xy, ups[0].size, rgb);
        assert_eq!(wrong, Pixels(0), "upload {k}: not the newest frame's RGB");
        drawn += 1;
    }
    assert!(
        term.images().images_evicted >= 8,
        "the premise: the quota deleted frames, {} of them",
        term.images().images_evicted
    );
    // Every upload: the last one's status line no longer prints and
    // scrolls the frames away, the setting's reading of a chunk it cuts.
    assert_eq!(drawn, 13, "the premise: every upload drawn");
    assert_eq!(
        repeats,
        vec![1],
        "the premise: every frame differs from the one before but the second"
    );
}

/// **mpv 0.41.0's exit, as it wrote it, turns off what it turned on**, by
/// default: the capture fed in the tty's reads, then zsh's prompt with its
/// marks. mpv's delete is left open (`ESC _ G a = d ;`), and the `ESC` of
/// the resets after it cancels it, so its exit shows the cursor, turns
/// any-event tracking off and brings the primary back: each holds when the
/// prompt is drawn. The premise: the capture turned `?1003` on.
#[test]
fn after_mpvs_exit_the_mouse_is_off_and_the_primary_is_back() {
    let bytes = std::fs::read(capture::PATH).expect("the capture");
    assert!(
        bytes.windows(8).any(|w| w == b"\x1b[?1003h"),
        "the premise: mpv turned any-event tracking on"
    );
    let zsh: &[u8] = b"\x1b]133;D;0;aid=4242\x07\x1b]133;A;aid=4242\x07~/baud \xe2\x9d\xaf ";
    let grid = grid_of(GRID);
    let mut owned = Owned::new(&grid);
    let mut term = Term::new(owned.storage(), &grid);
    for chunk in bytes.chunks(READ).chain(core::iter::once(zsh)) {
        crate::reads::feed(
            &mut term,
            [chunk],
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        while crate::reads::decode_step(&mut term).0 == DecodeProgress::More {}
    }
    assert_eq!(
        (term.mouse_tracking(), term.screen(), term.cursor_visible()),
        (
            baud_core::MouseTracking::Off,
            baud_core::Screen::Normal,
            true
        )
    );
}

/// **The demo's prompts after mpv's exit**: each shell's mark is a
/// BEL-ended `OSC 133 ; A`, after the `OSC 133 ; D` of the command before
/// it -- zsh's, baud's integration's, `ESC ] 133 ; A ; aid=PID BEL`
/// (`baud-app/src/shell/zshenv.zsh`), and the container's bash's,
/// `ESC ] 133 ; A ; BEL` (`nix/demo-container/profile`).
///
/// **After the capture's quit, mpv 0.41.0's**, whose `ESC _ G a = d ;`
/// has no terminator: by default the `ESC` of `ESC [ ? 25 h` cancels the
/// `a=d`, the restores run, and the primary is up with a prompt opened
/// there; cut straight after the `a=d`, the prompt's own `ESC` cancels it,
/// and its marks are read on the alternate screen nothing has left, as
/// under the setting.
/// Under `[images] kitty_string_end = "kitty"` a kitty command ends only at
/// `BEL` or `ESC \`, so `?25h`, `?1003l` and `?1049l` are its payload
/// (`mpvs_unterminated_exit_leaves_the_modes_set_as_kitty_does`,
/// `baud-core/tests/kitty_string_end.rs`), and so is the prompt's first
/// mark up to its `BEL`, which ends it. The mark after it opens a prompt
/// on the alternate screen, which nothing has left, and the primary opens
/// none, whole and cut.
///
/// **After the pinned mpv's quit**, 8e566099cb, which ends that `a=d`
/// with `ESC \` (the stream below, the capture with that `ST` after its
/// `a=d`), the restores run: the primary is up, `?1049` off, and either
/// prompt opens there. The control: a prompt with no mark opens none.
/// Perturbation: feed the shell's prompt as plain text and no prompt
/// opens.
#[test]
fn the_demos_prompts_after_mpvs_exit_are_bel_ended_osc_133_marks() {
    let zshenv = include_str!("../shell/zshenv.zsh");
    for spelled in [
        "__baud_bel=$'\\a'",
        "${__baud_osc}D;$st;aid=$$;baud_token=$__baud_token${__baud_bel}",
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
    let fed_under = |end: baud_domain::ControlStringEnd, parts: &[&[u8]]| {
        let mut owned = Owned::new(&grid);
        let mut term = Term::new(owned.storage(), &grid);
        term.set_control_string_end(end);
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
    let fed = |parts: &[&[u8]]| fed_under(baud_domain::ControlStringEnd::Standard, parts);
    let ended: &[&[u8]] = &[&bytes[..cut], b"\x1b\\", &bytes[cut..]];
    for prompt in [zsh, bash] {
        let text = String::from_utf8_lossy(prompt);
        assert_eq!(
            fed(&[&bytes, prompt]),
            (baud_core::Screen::Normal, true, baud_core::Zone::Prompt),
            "after mpv 0.41.0's exit, whole: {text}"
        );
        assert_eq!(
            fed(&[&bytes[..cut], prompt]),
            (baud_core::Screen::Alternate, false, baud_core::Zone::Prompt),
            "after mpv 0.41.0's exit, cut after its a=d: {text}"
        );
        for (what, exit) in [("whole", &bytes[..]), ("cut after its a=d", &bytes[..cut])] {
            assert_eq!(
                fed_under(baud_domain::ControlStringEnd::Kitty, &[exit, prompt]),
                (baud_core::Screen::Alternate, false, baud_core::Zone::Prompt),
                "after mpv 0.41.0's exit, {what}, under the setting: {text}"
            );
        }
        let mut parts = ended.to_vec();
        parts.push(prompt);
        assert_eq!(
            fed(&parts),
            (baud_core::Screen::Normal, true, baud_core::Zone::Prompt),
            "after the pinned mpv's exit: {}",
            String::from_utf8_lossy(prompt)
        );
    }
    let mut plain = ended.to_vec();
    plain.push(b"~/baud \xe2\x9d\xaf ");
    let (screen, opened, _) = fed(&plain);
    assert_eq!(screen, baud_core::Screen::Normal, "the control's premise");
    assert!(!opened, "the control: a prompt with no mark opens none");
}
