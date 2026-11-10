//! **A launch, headless, with a real program on its pty.** What the app
//! makes when it starts -- the config read from no file, as a user with none
//! launches; the faces it names; the renderer, set up as both drivers set
//! one (`host::set_up_renderer`); the terminal, set up as the Linux driver
//! sets one (`host::set_up_term`); and the program spawned through the
//! app's own spawn (`Pty::spawn_given`, which is `Pty::spawn` with the
//! environment given) -- and then the reads fed, the keys sent and the
//! frames drawn through the functions the drivers call: `reads::paced` for
//! a read, `typed_text` and `reads::input` for typed text, as the
//! responder's `insertText:` sends it, and `Renderer::render_at` for a
//! frame of the window where it sits, on the CPU, as the CPU path's
//! `present` draws one. No window opens -- held by a test, not this line:
//! [`the_launch_path_names_no_appkit`] -- no GPU is asked for, and nothing
//! of the system's is read but the fonts and the locale CoreFoundation
//! gives; nothing is changed.
//!
//! **Waits are bounded and never sleeps.** The pty's reader thread wakes
//! the test once a batch, as it wakes the run loop, and a wait that has not
//! seen what it waits for in [`WAIT`] fails, saying what the program wrote.

use super::{theme_for, typed_text};
use crate::host::{MAX_GRID, host_geometry};
use crate::pty::{Outbox, Parent, Pty, Stream};
use baud_core::{Owned, Term};
use baud_domain::{Col, Cols, ImagePx, KeyCode, KeyEvent, Modifiers, Row, Rows, WinSize};
use baud_render::{Blend, Damage, PxSize, Renderer, Target};
use macos_liquid_glass::drawable::Appearance;
use std::ffi::OsString;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

/// The longest any one wait runs before the test fails: far past what a
/// shell or yazi takes to answer here, so only a program that never
/// answers reaches it.
pub(super) const WAIT: Duration = Duration::from_secs(30);

/// How many times the program has written something ([`Session::written`]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) struct Times(pub(super) usize);

/// **A launched window, headless**: the renderer, the terminal and the
/// program on its pty, with what the app owes the program.
pub(super) struct Session {
    pub(super) renderer: Renderer<'static, 'static>,
    pub(super) term: Term<'static>,
    /// The faces' files and the font state the terminal was told about,
    /// held for as long as the renderer draws from them.
    _fonts: crate::fonts::Fonts,
    pty: Pty,
    outbox: Outbox,
    woken: Receiver<()>,
    pace: crate::reads::Pace,
    /// Every byte the program wrote, in order: what a failure shows.
    pub(super) seen: Vec<u8>,
    /// What the last pump handed the pty, the core's answers among it,
    /// in the order the outbox took it.
    pub(super) handed: Vec<u8>,
    closed: Stream,
}

impl Session {
    /// The launch, at `ws`, running `cmd` under `/bin/sh -c`, as
    /// `BAUD_CMD` runs one, in the environment `parent` -- what baud's own
    /// would be -- started by `started_by`.
    pub(super) fn start(
        ws: &WinSize,
        cmd: &str,
        parent: Vec<(OsString, OsString)>,
        started_by: Parent,
    ) -> Session {
        let launched = crate::config::Config::launch_at(&[]);
        assert_eq!(launched.config.source, None, "no config file was read");
        let notice = crate::live::Notice::of(&launched.said);
        let cfg = launched.config;
        let (data, mut held) = crate::host::map_faces(&cfg);
        let faces = crate::host::parse_faces(&cfg, &data, &mut held);
        let fonts = crate::fonts::Fonts::new(&cfg, data, faces, held);
        let (mut halves, _) = crate::palette::load_halves(
            cfg.palette_light.as_deref(),
            cfg.palette_dark.as_deref(),
            &crate::palette::Dirs::from_env(None),
        );
        halves.selection = cfg.selection;
        halves.cursor = cfg.cursor;
        let theme = theme_for(Appearance::Dark, &cfg.glass, cfg.theme, cfg.floor, &halves);
        let mut renderer = Renderer::new(
            crate::host::stack(&fonts),
            PxSize::of(cfg.font_size).expect("size"),
            theme,
            Blend::Native,
            Cols::of_index(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .expect("cell metrics");
        crate::host::set_up_renderer(&mut renderer, &cfg, &notice);
        let owned: &'static mut Owned = Box::leak(Box::new(
            Owned::with_geometry(&MAX_GRID, &host_geometry())
                .with_images(cfg.images)
                .with_clipboard(cfg.clipboard.budget)
                .with_notices(crate::notify::BUDGET),
        ));
        let mut term = Term::new(owned.storage(), ws);
        term.set_kitty_files(Box::leak(Box::new(crate::kitty_files::LocalFiles::new())));
        term.set_regis_font(crate::regis_font::glyph);
        term.set_regis_hold(baud_core::RegisHold::On);
        crate::reads::hold_replies(&mut term);
        renderer.fit(&term);
        let term = crate::host::set_up_term(term, &cfg, &renderer, &fonts);
        let (tx, woken) = channel();
        let m = renderer.metrics();
        let pty = Pty::spawn_given(
            ws,
            (m.cell_w, m.cell_h),
            cmd,
            parent,
            started_by,
            move || {
                let _ = tx.send(());
            },
        )
        .expect("forkpty");
        // As each driver does when it opens the window (`say_refused`).
        let mut term = term;
        crate::pty::say_refused(&mut term, &pty);
        Session {
            renderer,
            term,
            _fonts: fonts,
            pty,
            outbox: Outbox::default(),
            woken,
            pace: crate::reads::Pace::new(),
            seen: Vec::new(),
            handed: Vec::new(),
            closed: Stream::Open,
        }
    }

    /// What the program wrote since the last pump, fed as the pump feeds
    /// it, and the answers sent.
    pub(super) fn pump(&mut self) {
        let drained = self.pty.drain();
        if drained.stream == Stream::Closed {
            self.closed = Stream::Closed;
        }
        for c in &drained.chunks {
            self.seen.extend_from_slice(&c.bytes);
        }
        self.handed.clear();
        let (term, outbox, handed) = (&mut self.term, &mut self.outbox, &mut self.handed);
        let _ = crate::reads::paced(
            term,
            &mut self.pace,
            Instant::now(),
            drained.chunks.iter().map(|c| (&c.bytes[..], ())),
            |b| {
                outbox.push(b);
                handed.extend_from_slice(b);
            },
            |_, _| {},
            &mut crate::hostlog::NoLog,
            &mut Vec::new(),
        );
        self.pty.follow(self.term.input_encoding());
        self.flush();
    }

    /// What the core answered, behind what is owed, as much as the kernel
    /// takes: `App::flush_reply`.
    fn flush(&mut self) {
        self.handed.extend_from_slice(self.term.reply());
        self.outbox.push(self.term.reply());
        self.term.clear_reply();
        let _ = self.pty.write(&mut self.outbox);
    }

    /// Pumps until `done` holds of the terminal, or fails after [`WAIT`]
    /// naming `what`.
    pub(super) fn until(&mut self, what: &str, mut done: impl FnMut(&Session) -> bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            self.pump();
            if done(self) {
                return;
            }
            assert!(
                self.closed == Stream::Open,
                "the program ended before {what}: {}",
                self.tail()
            );
            let left = deadline.saturating_duration_since(Instant::now());
            match self.woken.recv_timeout(left) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
                Err(RecvTimeoutError::Timeout) => panic!("never {what}: {}", self.tail()),
            }
        }
    }

    /// The last of what the program wrote, for a failure's message.
    pub(super) fn tail(&self) -> String {
        let from = self.seen.len().saturating_sub(2000);
        format!("{:?}", String::from_utf8_lossy(&self.seen[from..]))
    }

    /// How many times the program has written `needle`.
    pub(super) fn written(&self, needle: &[u8]) -> Times {
        Times(
            self.seen
                .windows(needle.len())
                .filter(|w| *w == needle)
                .count(),
        )
    }

    /// Text typed as the input manager hands it to `insertText:`: each
    /// character the key `typed_text` makes, sent as `App::send` sends one.
    pub(super) fn type_text(&mut self, text: &str) {
        for e in typed_text(text, None) {
            self.key(e);
        }
    }

    /// The window's grid made `ws`, as a relayout makes it: the terminal
    /// resized, the renderer fitted to it, and the child told
    /// (`App::tell_pty`).
    pub(super) fn resize(&mut self, ws: &WinSize) {
        let _ = self.term.resize(ws);
        self.renderer.fit(&self.term);
        let m = self.renderer.metrics();
        self.pty.resize(ws, m.cell_w, m.cell_h);
        self.flush();
    }

    /// Return, as `keyDown:` sends it.
    pub(super) fn enter(&mut self) {
        self.key(KeyEvent::press(KeyCode::Enter, Modifiers::NONE));
    }

    /// A key to the core and its bytes on to the program: `App::send`.
    pub(super) fn key(&mut self, e: KeyEvent) {
        let outbox = &mut self.outbox;
        crate::reads::input(&mut self.term, |t| t.key(e), |b| outbox.push(b));
        self.flush();
    }

    /// The whole frame, drawn on the CPU from nothing, of the window where
    /// it sits: what the CPU path's `present` draws.
    pub(super) fn frame(&mut self) -> Vec<u32> {
        let (w, h) = self.renderer.whole_size();
        let area = baud_font::Extent::new(w, h).cells().expect("an area");
        let mut frame = vec![0u32; area];
        let mut target = Target::new(&mut frame, w, w, h).expect("a target");
        let back = self.term.window_back();
        let _ = self
            .renderer
            .render_at(&mut self.term, back, &mut target, Damage::Full);
        frame
    }

    /// The pixels of the cell at `row`, `col` of `frame`, top left first.
    pub(super) fn cell_pixels(&self, frame: &[u32], (row, col): (Row, Col)) -> Vec<u32> {
        let m = self.renderer.metrics();
        let n = |p: baud_font::Px| usize::try_from(p.value()).expect("a size");
        let (cw, ch, w) = (n(m.cell_w), n(m.cell_h), n(self.renderer.whole_size().0));
        // The grid is drawn the margin and the exit-status strip in from
        // the left, from the top (`Renderer::whole_size`).
        let x0 = n(self.renderer.grid_left());
        (0..ch)
            .flat_map(|y| {
                let at = (row.index() * ch + y) * w + x0 + col.index() * cw;
                frame[at..at + cw].to_vec()
            })
            .collect()
    }

    /// The text of the live screen's row `y`, blanks trimmed: what the
    /// cells hold.
    pub(super) fn row_text(&self, y: Row) -> String {
        self.term
            .view(Rows::ZERO)
            .row(y)
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

    /// Every row of the live screen, as [`Session::row_text`] reads it.
    pub(super) fn screen_text(&self) -> Vec<String> {
        (0..self.term.geometry().1.index())
            .map(|y| self.row_text(Row::from_index(y)))
            .collect()
    }
}

/// **What launchd gives an app the Dock starts**, as
/// `baud-bench/src/host/dock_check.rs` launches one: `HOME`, `USER`,
/// `LOGNAME`, `TMPDIR` and `__CF_USER_TEXT_ENCODING` as this process has
/// them, a system `PATH` and `SHELL` -- and no `LANG` or `LC_*`, which
/// launchd does not set (measured with `launchctl getenv LANG` and two
/// launchd-started apps' environments) -- with launchd's `XPC_FLAGS` (measured
/// in an app `open -n` started; its `XPC_SERVICE_NAME`, which names the app, is
/// left out: no locale rule reads it).
pub(super) fn launchd_environment() -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = [
        "HOME",
        "USER",
        "LOGNAME",
        "TMPDIR",
        "__CF_USER_TEXT_ENCODING",
    ]
    .into_iter()
    .filter_map(|k| std::env::var_os(k).map(|v| (OsString::from(k), v)))
    .collect();
    env.extend(
        [
            ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
            ("SHELL", "/bin/zsh"),
            ("XPC_FLAGS", "0x0"),
        ]
        .into_iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v))),
    );
    env
}

/// **A shell a Dock launch starts draws a typed Hebrew letter, and names a
/// UTF-8 locale.** The launch is the app's: no config file, the spawn with
/// exactly what launchd gives an app ([`launchd_environment`], which names
/// no locale) and launchd as baud's parent, so the shell's locale is the
/// one CoreFoundation's preferences give (`shell_locale`), as on a Dock
/// launch, and the shell starts home. The shell is zsh with no startup
/// file (`-f`), so nothing of the user's is read, and interactive, so its
/// line editor runs: once it reads (bracketed paste on, as it starts each
/// line), yod, U+05D9, is typed through the responder's text path. The
/// screen then holds the letter, drawn with ink in its cell, and no U+FFFD
/// or `<0099>` -- what zsh in the C locale echoes for the letter's two bytes.
/// Then Return (zsh
/// names the letter as a command it cannot find) and `echo $LANG`, whose
/// answer names a `.UTF-8` locale; which one is the user's preference, so
/// only its codeset is checked.
#[test]
fn a_dock_launched_shell_draws_a_typed_hebrew_letter() {
    const READING: &[u8] = b"\x1b[?2004h";
    let ws = WinSize {
        ws_col: 80,
        ws_row: 24,
    };
    let mut s = Session::start(
        &ws,
        "exec /bin/zsh -f -i",
        launchd_environment(),
        Parent::Launchd,
    );
    s.until("zsh reads a line", |s| s.written(READING) >= Times(1));
    s.type_text("\u{05D9}");
    // Echoed: whole, or as the C locale's escape or a decoder's U+FFFD.
    let echoed = |s: &Session| {
        s.screen_text()
            .iter()
            .any(|l| l.contains(['\u{05D9}', '\u{FFFD}']) || l.contains("<0099>"))
    };
    s.until("the letter is echoed", echoed);
    let screen = s.screen_text();
    assert!(
        screen
            .iter()
            .all(|l| !l.contains('\u{FFFD}') && !l.contains("<0099>")),
        "no replacement character and no escape: {screen:?}"
    );
    let row = screen
        .iter()
        .position(|l| l.contains('\u{05D9}'))
        .expect("the letter's row");
    let col = screen[row]
        .chars()
        .position(|c| c == '\u{05D9}')
        .expect("the letter's column");
    let frame = s.frame();
    let cell = s.cell_pixels(&frame, (Row::from_index(row), Col::at_index(col)));
    assert!(
        cell.iter().any(|&p| p != cell[0]),
        "the letter's cell holds ink: row {row}, column {col}"
    );
    // Return runs the letter as a command, which zsh says it cannot find,
    // and the next line reads `$LANG`.
    s.enter();
    s.until("zsh reads the next line", |s| {
        s.written(READING) >= Times(2)
    });
    s.type_text("echo $LANG");
    s.enter();
    s.until("zsh reads the line after", |s| {
        s.written(READING) >= Times(3)
    });
    let screen = s.screen_text();
    let asked = screen
        .iter()
        .position(|l| l.ends_with("echo $LANG"))
        .unwrap_or_else(|| panic!("the command on the screen: {screen:?}"));
    let lang = &screen[asked + 1];
    assert!(
        lang.ends_with(".UTF-8") && !lang.contains(' '),
        "the shell's LANG names a UTF-8 locale: {lang:?} in {screen:?}"
    );
    assert!(
        screen
            .iter()
            .all(|l| !l.contains('\u{FFFD}') && !l.contains("<0099>")),
        "zsh's answer names the letter whole: {screen:?}"
    );
}

/// **No trail is drawn over the history the wheel scrolls back to**, end to
/// end: a real program's output moves the cursor, every trail starts, and
/// the wheel, through the scroll handler's route and its history step
/// (`wheel::route`, `scroll_history`), scrolls the window back three lines
/// while the trail is in flight. Each frame is the one `present` builds:
/// the trail `frame_trail` gives at the display link's target, handed to
/// the renderer and drawn on the CPU, and compared with the same frame
/// drawn with no trail. The control: before the wheel, the trail's frame
/// differs from the trail-less one. Then, for every frame while back, the
/// two are the same pixels, and no trail reaches the renderer at all, so no
/// path draws one. The trails are each named one and the default
/// a launch with no config draws (`TrailPick::effective` at full motion,
/// so the system's Reduce Motion is not read).
#[test]
fn no_frame_draws_a_trail_over_the_history_the_wheel_scrolls_to() {
    use crate::config::{CursorTrail, SystemMotion};
    use crate::wheel::{Delta, Route, Travel};
    let ws = WinSize {
        ws_col: 60,
        ws_row: 12,
    };
    let cmd = "/bin/stty -echo; i=0; while [ $i -lt 40 ]; do echo \"line $i\"; \
               i=$((i+1)); done; printf ready; read x; printf '\\033[3;30H'; read y";
    let parent = vec![
        (OsString::from("LANG"), OsString::from("en_US.UTF-8")),
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
    ];
    let default = crate::config::Config::launch_at(&[])
        .config
        .cursor_trail
        .effective(SystemMotion::Full);
    for kind in [
        default,
        CursorTrail::Ghost,
        CursorTrail::Quad,
        CursorTrail::Sweep,
        CursorTrail::Fire,
        CursorTrail::Plasma,
        CursorTrail::Lightning,
        CursorTrail::Ice,
        CursorTrail::Ufo,
        CursorTrail::Orb,
        CursorTrail::Liquid,
    ] {
        let mut s = Session::start(&ws, cmd, parent.clone(), Parent::Other);
        s.until("the program is ready", |s| {
            s.screen_text().iter().any(|l| l == "ready")
        });
        let mut chase = crate::trail::Chase::new(kind.style());
        let (mut trail, mut last) = (None, None);
        let mut carried = crate::carried::Carried::new();
        // A frame at `t` seconds: its trail drawn, and the same frame
        // drawn without one.
        let mut frame = |s: &mut Session, t: f64| {
            let look = super::frame_trail(
                (kind, crate::config::TrailScroll::Ride),
                &s.term,
                &s.renderer,
                1.0,
                t,
                (&mut chase, &mut trail, &mut last),
                (&mut carried, Rows::ZERO),
            );
            s.renderer.set_trail(look);
            let with = s.frame();
            s.renderer.set_trail(None);
            let without = s.frame();
            (look.is_some(), with == without)
        };
        let _ = frame(&mut s, 1.0);
        s.enter();
        s.until("the cursor moves", |s| {
            s.term.cursor() == (Row::from_index(2), baud_domain::Col::at_index(29))
        });
        let _ = frame(&mut s, 1.005);
        let (drawn, same) = frame(&mut s, 1.020);
        assert!(drawn && !same, "{kind:?}: the control, a trail drawn live");
        let lines = Travel::default().lines(Delta::Lines(3.0));
        assert_eq!(
            crate::wheel::route(&s.term, baud_domain::MouseMods::NONE),
            Route::History,
            "{kind:?}: the wheel scrolls the history"
        );
        assert_eq!(
            super::scroll_history(&mut s.term, lines),
            Some(Rows::of_count(3)),
            "{kind:?}: three lines back"
        );
        for n in 0..30 {
            let t = 1.030 + 0.016 * f64::from(n);
            let (drawn, same) = frame(&mut s, t);
            assert!(same, "{kind:?}: a trail pixel over the history at {t:.3} s");
            assert!(
                !drawn,
                "{kind:?}: a trail handed to the renderer at {t:.3} s"
            );
        }
    }
}

/// A PNG of `w` by `h` RGB pixels `px` gives, written whole: zlib's stored
/// blocks, so no compressor is needed and the file is what its pixels are.
fn write_png(
    path: &std::path::Path,
    (w, h): (ImagePx, ImagePx),
    mut px: impl FnMut(u32, u32) -> [u8; 3],
) {
    let (w, h) = (u32::from(w.raw()), u32::from(h.raw()));
    // PNG's CRC-32 (ISO 3309), over a chunk's type and data.
    let crc = |data: &[u8]| {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    };
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend_from_slice(&px(x, y));
        }
    }
    let mut z = vec![0x78, 0x01];
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, c) in chunks.iter().enumerate() {
        z.push(u8::from(i + 1 == chunks.len()));
        let n = u16::try_from(c.len()).expect("a stored block");
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(c);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |t: &[u8], d: &[u8]| {
        out.extend_from_slice(&u32::try_from(d.len()).expect("a chunk").to_be_bytes());
        let mut td = t.to_vec();
        td.extend_from_slice(d);
        out.extend_from_slice(&td);
        out.extend_from_slice(&crc(&td).to_be_bytes());
    };
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    std::fs::write(path, out).expect("the PNG");
}

/// A PDF of `pages` pages, each a 300 by 400 point page filled with a
/// rectangle of its own colour: the smallest file a PDF previewer renders.
fn write_pdf(path: &std::path::Path, pages: &[[f32; 3]]) {
    let mut objs: Vec<String> = vec![
        "<</Type/Catalog/Pages 2 0 R>>".into(),
        format!(
            "<</Type/Pages/Kids[{}]/Count {}>>",
            (0..pages.len())
                .map(|k| format!("{} 0 R", 3 + 2 * k))
                .collect::<Vec<_>>()
                .join(" "),
            pages.len()
        ),
    ];
    for (k, [r, g, b]) in pages.iter().enumerate() {
        let draw = format!("{r} {g} {b} rg 20 20 260 360 re f");
        objs.push(format!(
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 400]/Contents {} 0 R>>",
            4 + 2 * k
        ));
        objs.push(format!(
            "<</Length {}>>\nstream\n{draw}\nendstream",
            draw.len()
        ));
    }
    let mut out = String::from("%PDF-1.4\n");
    let mut at = Vec::new();
    for (k, o) in objs.iter().enumerate() {
        at.push(out.len());
        out.push_str(&format!("{} 0 obj\n{o}\nendobj\n", k + 1));
    }
    let xref = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objs.len() + 1
    ));
    for a in at {
        out.push_str(&format!("{a:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n",
        objs.len() + 1
    ));
    std::fs::write(path, out).expect("the PDF");
}

/// Whether `prog` is a file in one of `path`'s directories.
fn on_path(path: &std::ffi::OsStr, prog: &str) -> bool {
    std::env::split_paths(path).any(|d| d.join(prog).is_file())
}

/// **The previews a kitty graphics program has finished sending** in
/// `seen`: each transmission -- `a=T`, which places as it stores, or `a=t`,
/// placed after by `a=p` -- counted when its last chunk (`m=0`, or a
/// command with no `m=1`) has arrived to its `ST`; and the pixel size the
/// last one declared, `s` by `v`.
fn kitty_previews(seen: &[u8]) -> (usize, Option<(u32, u32)>) {
    let text = String::from_utf8_lossy(seen);
    let (mut done, mut open, mut size) = (0, false, None);
    for rest in text.split("\x1b_G").skip(1) {
        let keys = &rest[..rest.find(';').or_else(|| rest.find('\x1b')).unwrap_or(0)];
        let has = |k: &str| keys.split(',').any(|kv| kv == k);
        let value = |k: &str| {
            keys.split(',')
                .find_map(|kv| kv.strip_prefix(k).and_then(|v| v.strip_prefix('=')))
                .and_then(|v| v.parse::<u32>().ok())
        };
        if has("a=T") || has("a=t") {
            size = value("s").zip(value("v"));
            match (has("m=1"), rest.contains("\x1b\\")) {
                (true, _) => open = true,
                (false, true) => done += 1,
                (false, false) => {}
            }
        } else if open && has("m=0") && rest.contains("\x1b\\") {
            open = false;
            done += 1;
        }
    }
    (done, size)
}

/// **Every yazi preview arrives as a picture the terminal keeps**, with real
/// yazi on the app's pty over a folder of generated files: PNGs and JPEGs
/// small, wide, tall and photo-sized -- the photo's preview, about 620 KiB as
/// yazi sends it, past the 256 KiB the image body limit (`body_kb`) once held
/// -- and a two-page PDF where `pdftoppm`, yazi's
/// PDF previewer, is installed. yazi asks what the terminal is, the core
/// answers as the app sends it, and yazi picks kitty's graphics protocol:
/// baud answers its probe `OK` and claims no Sixel by default. Each file
/// is reached as a user reaches it, `j` typed through the responder's text
/// path, and its preview -- an `a=T`, or an `a=t` that `a=p` places, sent in
/// chunks -- is waited for whole.
/// Then the core has refused nothing, places one picture, the one yazi
/// declared, and its pixels are not one colour. On the PDF, `J` seeks
/// to its second page, whose preview arrives the same way and is not the
/// first's.
///
/// yazi is kept from the user's files: its home, config, cache and
/// temporary directories are a fresh folder's. Where yazi is not installed
/// the test says so and checks nothing.
#[test]
fn every_yazi_preview_arrives_as_a_picture_the_terminal_keeps() {
    let path = std::env::var_os("PATH").unwrap_or_default();
    if !on_path(&path, "yazi") {
        eprintln!("yazi is not installed: no preview to check");
        return;
    }
    let dir = std::env::temp_dir().join(format!("baud-yazi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let files = dir.join("files");
    for d in ["files", "home", "config", "tmp", "src"] {
        std::fs::create_dir_all(dir.join(d)).expect("a folder");
    }
    // A seeded generator: noise is what makes a picture's body long.
    let mut seed = 20_261_031u32;
    let mut noise = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed.to_be_bytes()[0]
    };
    let shade = |v: u32, of: u32| u8::try_from(v * 255 / of).unwrap_or(255);
    write_png(
        &files.join("a-small.png"),
        (ImagePx::of(96), ImagePx::of(64)),
        |x, y| [shade(x, 96), shade(y, 64), 128],
    );
    write_png(
        &files.join("b-wide.png"),
        (ImagePx::of(2000), ImagePx::of(400)),
        |x, y| [shade(x, 2000), noise(), shade(y, 400)],
    );
    write_png(
        &files.join("c-photo.png"),
        (ImagePx::of(1200), ImagePx::of(1600)),
        |x, y| {
            let n = noise();
            [shade(x, 1200) ^ (n & 0x1f), shade(y, 1600), n]
        },
    );
    write_png(
        &dir.join("src/flat.png"),
        (ImagePx::of(800), ImagePx::of(600)),
        |x, y| [shade(x, 800), 90, shade(y, 600)],
    );
    write_png(
        &files.join("f-tall.png"),
        (ImagePx::of(300), ImagePx::of(2000)),
        |x, y| [noise(), shade(y, 2000), shade(x, 300)],
    );
    for (from, to) in [
        ("files/c-photo.png", "d-photo.jpg"),
        ("src/flat.png", "e-flat.jpg"),
    ] {
        let made = std::process::Command::new("/usr/bin/sips")
            .args(["-s", "format", "jpeg"])
            .arg(dir.join(from))
            .arg("--out")
            .arg(files.join(to))
            .output()
            .expect("sips");
        assert!(made.status.success(), "sips made {to}");
    }
    let pdf = on_path(&path, "pdftoppm");
    match pdf {
        true => write_pdf(
            &files.join("g-pages.pdf"),
            &[[0.2, 0.4, 0.8], [0.8, 0.3, 0.2]],
        ),
        false => eprintln!("pdftoppm is not installed: yazi previews no PDF, so none is checked"),
    }
    let names: Vec<String> = {
        let mut n: Vec<String> = std::fs::read_dir(&files)
            .expect("the files")
            .map(|e| {
                e.expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        n.sort();
        n
    };
    let big = std::fs::metadata(files.join("c-photo.png"))
        .expect("the photo")
        .len();
    assert!(big > 1 << 18, "the photo is past 256 KiB: {big}");
    let ws = WinSize {
        ws_col: 117,
        ws_row: 40,
    };
    let at = |k: &str, p: &std::path::Path| (OsString::from(k), p.as_os_str().to_owned());
    let parent = vec![
        at("HOME", &dir.join("home")),
        at("YAZI_CONFIG_HOME", &dir.join("config")),
        at("XDG_CONFIG_HOME", &dir.join("config")),
        at("XDG_CACHE_HOME", &dir.join("tmp")),
        at("XDG_STATE_HOME", &dir.join("tmp")),
        at("XDG_DATA_HOME", &dir.join("tmp")),
        at("TMPDIR", &dir.join("tmp")),
        (OsString::from("LANG"), OsString::from("en_US.UTF-8")),
        (OsString::from("PATH"), path.clone()),
    ];
    let cmd = format!("cd '{}' && exec yazi", files.display());
    let mut s = Session::start(&ws, &cmd, parent, Parent::Other);
    let mut previews = 0;
    let mut check = |s: &mut Session, what: &str| {
        previews += 1;
        let want = previews;
        // Sent whole, and then placed or refused: a refusal is said below.
        s.until(&format!("the preview of {what}"), |s| {
            let imgs = s.term.images();
            kitty_previews(&s.seen).0 >= want
                && (imgs.placements().next().is_some() || imgs.images_refused > 0)
        });
        let imgs = s.term.images();
        assert_eq!(imgs.refusals, baud_domain::ImageRefusals::NONE, "{what}");
        assert_eq!(imgs.images_refused, 0, "{what}: nothing refused");
        // One placement where the picture is placed whole, one a cell where
        // a client places it cell by cell: every one this picture.
        let placed: Vec<_> = imgs.placements().map(|(_, p)| p.image).collect();
        assert!(
            placed.iter().all(|&i| i == placed[0]),
            "{what}: one picture placed: {placed:?}"
        );
        let v = imgs.image(placed[0]).expect("placed means kept");
        let declared = kitty_previews(&s.seen).1.expect("a size declared");
        let got = (u32::from(v.width.raw()), u32::from(v.height.raw()));
        assert_eq!(got, declared, "{what}: the picture yazi sent");
        assert!(
            v.pixels.iter().any(|&p| p != v.pixels[0]),
            "{what}: the picture is not one colour"
        );
        v.pixels.to_vec()
    };
    let mut last = Vec::new();
    for (k, name) in names.iter().enumerate() {
        if k > 0 {
            s.type_text("j");
        }
        last = check(&mut s, name);
    }
    if pdf {
        s.type_text("J");
        let second = check(&mut s, "the PDF's second page");
        assert!(second != last, "the second page is not the first");
    }
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

impl Session {
    /// The lines the renderer's last layout drew, in drawn order, where it
    /// re-broke a paragraph at its words (`baud_render::Display`); `None`
    /// where it drew every row as the terminal cut it.
    pub(super) fn drawn_lines(&self) -> Option<Vec<String>> {
        let orders = self.renderer.bidi();
        let vp = orders.display()?.window();
        Some(
            (0..vp.rows().index())
                .map(|y| {
                    let r = Row::from_index(y);
                    let mut out = vec![baud_domain::PackedCell::EMPTY; vp.cols().index()];
                    orders.permute(r, vp.row(r), &mut out);
                    out.iter()
                        .map(|c| {
                            c.content
                                .as_codepoint()
                                .and_then(|p| char::from_u32(p.value()))
                                .unwrap_or(' ')
                        })
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                })
                .collect(),
        )
    }
}

/// The paragraph the word-wrap tests print: no word longer than a line,
/// and long enough to wrap three times at forty columns.
const PARAGRAPH: &str = "Every finished paragraph is drawn at its words, so a word \
                         that does not fit at the end of a line moves whole to the next";

/// Whether `lines`, read as words, are [`PARAGRAPH`]'s, in order and each
/// whole: a word split across two lines reads as two that are not its.
fn words_whole(lines: &[String]) -> bool {
    let read = lines.join(" ");
    read.split_whitespace().eq(PARAGRAPH.split_whitespace())
}

/// The screen's rows that hold [`PARAGRAPH`]'s words, top to bottom.
fn paragraph_rows(lines: &[String]) -> Vec<String> {
    let first = lines.iter().position(|l| l.starts_with("Every finished"));
    let Some(first) = first else {
        return Vec::new();
    };
    lines[first..]
        .iter()
        .take_while(|l| !l.is_empty() && !l.contains('%'))
        .cloned()
        .collect()
}

/// zsh with no startup file, at forty columns, its prompt `% `, with
/// thirty lines of history behind its second prompt.
fn zsh_at_forty() -> Session {
    zsh_at(&WinSize {
        ws_col: 40,
        ws_row: 12,
    })
}

/// [`zsh_at_forty`] at `ws`.
fn zsh_at(ws: &WinSize) -> Session {
    let parent = vec![
        (OsString::from("LANG"), OsString::from("en_US.UTF-8")),
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
    ];
    let mut s = Session::start(ws, "PS1='%% ' exec /bin/zsh -f -i", parent, Parent::Other);
    s.until("zsh reads a line", |s| {
        s.written(b"\x1b[?2004h") >= Times(1)
    });
    // A screenful of history first: at the top of what the history keeps,
    // a re-broken paragraph keeps the terminal's cut rather than push a
    // line where nothing scrolls to it.
    s.type_text("print -l {1..30}");
    s.enter();
    s.until("zsh reads the next line", |s| {
        s.written(b"\x1b[?2004h") >= Times(2)
    });
    s
}

/// **With no config, a marked shell's paragraph is drawn at its words.**
/// zsh, with no startup file, is given the marks a shell integration sends
/// -- `OSC 133 ; A` as each prompt starts, `C` as a command runs, `D` as it
/// ends -- and prints a paragraph longer than the line. The terminal's own
/// cut splits a word (the control); the frame the renderer then draws with
/// the config's default, read from no file, breaks every line at a space,
/// each word whole, and the words in order are the paragraph's. Narrowed
/// to thirty columns while zsh waits at its prompt, the paragraph above it
/// is finished output, which the narrowing re-cuts, and the frame draws it
/// at its words again.
#[test]
fn with_no_config_a_marked_shells_paragraph_is_drawn_at_its_words() {
    let mut s = zsh_at_forty();
    s.type_text(
        "precmd() { print -n '\\e]133;D\\a\\e]133;A\\a' }; \
         preexec() { print -n '\\e]133;C\\a' }",
    );
    s.enter();
    s.until("the marked prompt", |s| {
        s.written(b"\x1b]133;A\x07") >= Times(1)
    });
    s.until("zsh reads the next line", |s| {
        s.written(b"\x1b[?2004h") >= Times(3)
    });
    s.type_text(&format!("print -r -- '{PARAGRAPH}'"));
    s.enter();
    s.until("zsh reads the line after", |s| {
        s.written(b"\x1b[?2004h") >= Times(4)
    });
    let stored = paragraph_rows(&s.screen_text());
    assert!(
        !words_whole(&stored),
        "the control: the terminal's cut splits a word: {stored:?}"
    );
    let _ = s.frame();
    let drawn = s
        .drawn_lines()
        .unwrap_or_else(|| panic!("the paragraph is re-broken: {:?}", s.screen_text()));
    let lines = paragraph_rows(&drawn);
    assert!(words_whole(&lines), "every word whole, in order: {lines:?}");
    // Narrower, while zsh waits at its marked prompt: the paragraph is
    // re-cut, and drawn at its words at thirty.
    s.resize(&WinSize {
        ws_col: 30,
        ws_row: 12,
    });
    let stored = paragraph_rows(&s.screen_text());
    assert!(!words_whole(&stored), "the control at thirty: {stored:?}");
    let _ = s.frame();
    let drawn = s
        .drawn_lines()
        .unwrap_or_else(|| panic!("re-broken at thirty: {:?}", s.screen_text()));
    let lines = paragraph_rows(&drawn);
    assert!(
        words_whole(&lines),
        "every word whole at thirty, in order: {lines:?}"
    );
}

/// The lines a long-format listing prints, as they read, and the bytes
/// that print them: an underlined header padded past eighty columns,
/// coloured columns, a long name, and a symbolic link whose target is
/// longer than forty.
fn listing() -> (Vec<String>, Vec<u8>) {
    let header = [
        "Permissions",
        "User  ",
        "Group",
        "Size",
        "Date Modified    ",
        &format!("Name{}", " ".repeat(60)),
    ];
    let mut text = vec![header.join(" ")];
    let mut bytes = header
        .iter()
        .map(|h| format!("\x1b[4m{h}\x1b[24m"))
        .collect::<Vec<_>>()
        .join(" ");
    bytes.push_str("\r\n");
    for (kind, name) in [
        ("drwxr-xr-x", "Applications/"),
        ("drwxr-xr-x", "pre-nix-darwin-20260803-201530-backup/"),
        (
            ".rw-r--r--",
            "a-rather-long-file-name-that-keeps-going-for-a-while.txt",
        ),
        (
            "lrwxr-xr-x",
            "'Shared - Example Team Space Projects' -> \
             /home/jordan/Library/CloudStorage/Shared-ExampleTeamSpace-Projects",
        ),
    ] {
        bytes.push_str(&format!(
            "\x1b[1;34m{kind}\x1b[0m jordan staff  64 B \x1b[32m26-11-04 21:23:07\x1b[0m \x1b[1;36m{name}\x1b[0m\r\n"
        ));
        text.push(format!(
            "{kind} jordan staff  64 B 26-11-04 21:23:07 {name}"
        ));
    }
    (text, bytes.into_bytes())
}

/// The rows the listing is drawn on in `rows`: from the header's first to
/// the row before the prompt after it.
fn listing_rows(rows: &[String]) -> Vec<String> {
    let Some(from) = rows.iter().position(|l| l.starts_with("Permissions")) else {
        return Vec::new();
    };
    rows[from..]
        .iter()
        .take_while(|l| !l.starts_with('%'))
        .cloned()
        .collect()
}

/// Every character but the spaces, in order.
fn squash(lines: &[String]) -> String {
    lines
        .concat()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// **A listing narrowed at zsh's prompt, through the functions a window's
/// resize runs**: zsh with the marks a shell integration sends prints a
/// long-format listing at eighty columns, and the window is dragged to
/// forty in steps of three and back, each step as `relayout` takes it --
/// the terminal resized, the renderer fitted, the child told its size,
/// which signals zsh to redraw its prompt -- and the frame drawn on the
/// CPU after each. Every frame draws every character of the listing in
/// order, nothing past the edge; at forty, once zsh has redrawn, the
/// listing is drawn at its words with a mark beside each continued line;
/// back at eighty the listing's rows are the ones drawn before; and zsh
/// still answers, its stream open.
#[test]
fn a_listing_narrowed_at_zshs_prompt_is_drawn_at_its_words_and_zsh_answers() {
    let wide = WinSize {
        ws_col: 80,
        ws_row: 30,
    };
    let mut s = zsh_at(&wide);
    s.renderer.set_margins(baud_font::Px::of(16));
    s.type_text(
        "precmd() { print -n '\\e]133;D\\a\\e]133;A\\a' }; \
         preexec() { print -n '\\e]133;C\\a' }",
    );
    s.enter();
    s.until("zsh reads the next line", |s| {
        s.written(b"\x1b[?2004h") >= Times(3)
    });
    let file = std::env::temp_dir().join(format!("baud-listing-{}", std::process::id()));
    let (lines, bytes) = listing();
    std::fs::write(&file, &bytes).expect("the listing's bytes");
    s.type_text(&format!("cat {}", file.display()));
    s.enter();
    s.until("zsh reads the line after", |s| {
        s.written(b"\x1b[?2004h") >= Times(4)
    });
    let _ = std::fs::remove_file(&file);
    let _ = s.frame();
    let before = listing_rows(&s.drawn_lines().unwrap_or_else(|| s.screen_text()));
    assert_eq!(squash(&before), squash(&lines), "the control: {before:#?}");
    let each = |s: &mut Session, c: u16| {
        s.resize(&WinSize {
            ws_col: c,
            ws_row: 30,
        });
        s.pump();
        let _ = s.frame();
        let rows = s.drawn_lines().unwrap_or_else(|| s.screen_text());
        let drawn = listing_rows(&rows);
        assert_eq!(
            squash(&drawn),
            squash(&lines),
            "at {c}: every character of the listing drawn, in order:\n{}",
            rows.join("\n")
        );
        assert!(
            drawn.iter().all(|l| l.chars().count() <= usize::from(c)),
            "at {c}: no row past the edge:\n{}",
            rows.join("\n")
        );
    };
    for c in (40..80).rev().step_by(3).chain([40]) {
        each(&mut s, c);
    }
    // zsh has redrawn at forty: a line typed and run brings its next
    // prompt, so every byte it wrote for the drag has been fed.
    s.type_text("print synced");
    s.enter();
    s.until("zsh runs the line at forty", |s| {
        s.written(b"\x1b[?2004h") >= Times(5)
    });
    // The marks as `App::present` places them, before the frame.
    let back = s.term.window_back();
    s.renderer.place_marks(
        &s.term,
        back,
        baud_render::Leaving::None,
        baud_font::Px::of(0),
    );
    let _ = s.frame();
    let rows = s
        .drawn_lines()
        .unwrap_or_else(|| panic!("re-broken at forty: {:#?}", s.screen_text()));
    let drawn = listing_rows(&rows);
    for word in lines.iter().flat_map(|l| l.split_whitespace()) {
        if word.contains('-') || word.contains('/') {
            continue;
        }
        assert!(
            drawn
                .iter()
                .any(|l| l.split_whitespace().any(|w| w == word)),
            "{word:?} whole on a row at forty:\n{}",
            rows.join("\n")
        );
    }
    let from = rows
        .iter()
        .position(|l| l.starts_with("Permissions"))
        .expect("the header");
    let ch = i32::try_from(s.renderer.metrics().cell_h.value()).expect("a height");
    let marked: Vec<usize> = s
        .renderer
        .margin_records()
        .iter()
        .map(|i| usize::try_from(i32::from(i.y.0).div_euclid(ch)).expect("a row"))
        .collect();
    let firsts = ["Permissions", "drwxr", ".rw-", "lrwxr"];
    for (k, l) in drawn.iter().enumerate() {
        let first = firsts.iter().any(|p| l.starts_with(p));
        assert_eq!(
            marked.contains(&(from + k)),
            !first,
            "row {} {l:?}: marked as a continued line:\n{}",
            from + k,
            rows.join("\n")
        );
    }
    for c in (40..=80).step_by(3).chain([80]) {
        each(&mut s, c);
    }
    s.type_text("print alive");
    s.enter();
    s.until("zsh answers at eighty", |s| {
        s.written(b"\x1b[?2004h") >= Times(6)
    });
    assert_eq!(s.closed, Stream::Open, "zsh's stream is open");
    let _ = s.frame();
    let rows = s.drawn_lines().unwrap_or_else(|| s.screen_text());
    assert_eq!(
        listing_rows(&rows),
        before,
        "back at eighty, the listing's rows as drawn before"
    );
}

/// **With no config and no marks, the paragraph is drawn at its words
/// once it is history**, and the live screen keeps the terminal's cut:
/// without a shell's `OSC 133` marks nothing tells the renderer which rows
/// a program is done with (`baud_render::rebreak`). zsh with no startup
/// file prints the paragraph, and the frame draws it as stored. Twenty
/// more lines move it into the history, the wheel scrolls the window back
/// to it through the scroll handler's history step (`scroll_history`), and
/// the frame then breaks every line at a space.
#[test]
fn with_no_config_an_unmarked_paragraph_is_drawn_at_its_words_in_the_history() {
    let mut s = zsh_at_forty();
    s.type_text(&format!("print -r -- '{PARAGRAPH}'"));
    s.enter();
    s.until("zsh reads the next line", |s| {
        s.written(b"\x1b[?2004h") >= Times(3)
    });
    let _ = s.frame();
    assert_eq!(
        s.drawn_lines(),
        None,
        "the live screen as the terminal cut it"
    );
    let stored = paragraph_rows(&s.screen_text());
    assert!(!words_whole(&stored), "the control: {stored:?}");
    s.type_text("print -l {1..20}");
    s.enter();
    s.until("zsh reads the line after", |s| {
        s.written(b"\x1b[?2004h") >= Times(4)
    });
    // The paragraph's three rows sit 22 to 24 lines above the prompt's;
    // fifteen lines back holds them and the twelve rows of the window.
    let lines = crate::wheel::Travel::default().lines(crate::wheel::Delta::Lines(15.0));
    assert_eq!(
        super::scroll_history(&mut s.term, lines),
        Some(Rows::of_count(15))
    );
    let _ = s.frame();
    let drawn = s
        .drawn_lines()
        .unwrap_or_else(|| panic!("the history is re-broken: {:?}", s.screen_text()));
    let lines = paragraph_rows(&drawn);
    assert!(words_whole(&lines), "every word whole, in order: {lines:?}");
}

/// The parent a test's shell has: a scratch `HOME` of `tag`'s, `HISTFILE`
/// at `/dev/null`, this `PATH`, a UTF-8 locale and `/bin/sh` as `SHELL`,
/// so nothing of the user's is read.
fn neutral_parent(tag: &str) -> (std::path::PathBuf, Vec<(OsString, OsString)>) {
    let home = std::env::temp_dir().join(format!("baud-launch-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("a scratch home");
    let os = |s: &str| OsString::from(s);
    let env = vec![
        (os("HOME"), home.clone().into_os_string()),
        (os("HISTFILE"), os("/dev/null")),
        (os("PATH"), std::env::var_os("PATH").unwrap_or_default()),
        (os("LANG"), os("en_US.UTF-8")),
        (os("SHELL"), os("/bin/sh")),
    ];
    (home, env)
}

/// **The bytes the shell wrote before the window wait in the channel,
/// and a turn draws them**: `bash --noprofile --norc` prints `hi` and
/// waits; once its read is in the channel, the frame drawn before a turn
/// is blank there and the frame after it draws the `h`. This holds the
/// harness's turn (`Session::pump`), not `Opening::show`, and passes
/// without the opening turn; show's order is held by
/// `the_first_frame_follows_the_opening_turn`.
#[test]
fn the_first_frame_holds_what_the_shell_wrote_before_it() {
    let ws = WinSize {
        ws_col: 40,
        ws_row: 6,
    };
    let (home, parent) = neutral_parent("first-frame");
    let mut s = Session::start(
        &ws,
        r#"exec bash --noprofile --norc -c "printf 'hi\n'; exec cat""#,
        parent,
        Parent::Other,
    );
    s.woken
        .recv_timeout(WAIT)
        .expect("the shell's read woke the reader");
    let at = (Row::from_index(0), Col::at_index(0));
    let blank = s.frame();
    assert_eq!(s.row_text(at.0), "", "nothing fed before the turn");
    s.pump();
    assert_eq!(s.row_text(at.0), "hi", "{}", s.tail());
    let first = s.frame();
    assert_ne!(
        s.cell_pixels(&first, at),
        s.cell_pixels(&blank, at),
        "the first frame draws the h"
    );
    let _ = std::fs::remove_dir_all(home);
}

/// **A read after a drain earns its own wake**: the reader wakes once a
/// batch until the main thread drains (`Pty::drain` clears the flag), so a
/// read that lands before the window can be woken is drained by the first
/// wake after it, and the next read wakes again. `bash --noprofile
/// --norc` prints `a`, waits for a line, then prints `b`.
#[test]
fn a_read_after_a_drain_earns_its_own_wake() {
    let ws = WinSize {
        ws_col: 40,
        ws_row: 6,
    };
    let (home, parent) = neutral_parent("wake");
    let mut s = Session::start(
        &ws,
        r#"exec bash --noprofile --norc -c "stty -echo; printf a; read x; printf b; exec cat""#,
        parent,
        Parent::Other,
    );
    s.woken
        .recv_timeout(WAIT)
        .expect("the first read woke the reader");
    s.until("a drawn", |s| s.row_text(Row::from_index(0)) == "a");
    while s.woken.try_recv().is_ok() {}
    s.outbox.push(b"\n");
    s.flush();
    s.woken
        .recv_timeout(WAIT)
        .unwrap_or_else(|_| panic!("the read after the drain woke nothing: {}", s.tail()));
    s.until("b drawn", |s| s.row_text(Row::from_index(0)) == "ab");
    let _ = std::fs::remove_dir_all(home);
}

/// **A kitty query read on a wake is answered on that wake**: a program on
/// the launch's pty -- the app's own spawn, reader thread and waker --
/// sends Chawan's `a=q` with its terminal raw, then reads eleven bytes and
/// prints them in hex. The pump that fed the query is the pump that handed
/// its `OK` to the pty, with nothing left in the outbox, and the program
/// prints those bytes. The pump is `App::pump`'s feed and flush, and the
/// macOS pump calls `flush_reply` after `reads::paced` in the same call,
/// with no return between them and no decode step, which only the display
/// link's tick takes; the Linux turn feeds each read as it arrives and
/// flushes before it waits again. The control: a program that sends no
/// query is handed nothing.
#[test]
fn a_kitty_query_read_on_a_wake_is_answered_on_that_wake() {
    let ws = WinSize {
        ws_col: 40,
        ws_row: 12,
    };
    let parent = vec![(OsString::from("PATH"), OsString::from("/usr/bin:/bin"))];
    let query = "printf '\\033_Gi=1,s=1,v=1,a=q;AAAAAA\\033\\\\'";
    let cmd = format!(
        "stty raw -echo; {query}; dd bs=1 count=11 2>/dev/null | od -An -tx1; \
         printf 'read\\n'; exec cat"
    );
    let mut s = Session::start(&ws, &cmd, parent.clone(), Parent::Other);
    s.until("the query fed", |s| s.term.kitty().commands >= 1);
    assert_eq!(
        s.handed, b"\x1b_Gi=1;OK\x1b\\",
        "answered on the pump that fed it"
    );
    assert!(s.outbox.is_empty(), "and written whole");
    s.until("the program reads the answer", |s| {
        s.written(b"read") >= Times(1)
    });
    let hex: String = String::from_utf8_lossy(&s.seen)
        .split_whitespace()
        .filter(|w| w.len() == 2 && w.bytes().all(|b| b.is_ascii_hexdigit()))
        .collect();
    assert_eq!(hex, "1b5f47693d313b4f4b1b5c", "the program read it");
    let mut quiet = Session::start(&ws, "printf 'none\\n'; exec cat", parent, Parent::Other);
    quiet.until("the control's line", |s| s.written(b"none") >= Times(1));
    assert!(quiet.handed.is_empty(), "the control: nothing handed");

    // The macOS pump: the feed, then the flush, in the one call.
    let src = include_str!("../app.rs");
    let pump = src
        .split_once(concat!("    fn pump(&mut self, ", "trigger: Trigger) {"))
        .and_then(|(_, rest)| rest.split_once("\n    }\n"))
        .map(|(b, _)| b)
        .expect("the pump");
    let fed = pump.find("crate::reads::paced(").expect("the feed");
    let flushed = pump
        .find("self.app.flush_reply(Asked::Unasked);")
        .expect("the flush");
    assert!(fed < flushed, "the flush after the feed");
    // Code lines only: a comment there speaks of the early returns after.
    let code: Vec<&str> = pump[fed..flushed]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect();
    assert!(code.len() > 100, "the control: the span is read");
    assert!(
        !code.iter().any(|l| l.contains("return")),
        "no way out between them"
    );
    assert!(
        !code.iter().any(|l| l.contains("decode_step")),
        "no decode step between them"
    );
    // The Linux turn: each read fed as it arrives, then the flush, before
    // the loop waits again; the only ways out end the program.
    let linux = include_str!("../linux.rs");
    let turn = linux
        .split_once("    'run: loop {")
        .and_then(|(_, rest)| rest.split_once("\n    }\n"))
        .map(|(b, _)| b)
        .expect("the Linux loop");
    let fed = turn
        .find("d.feed(&bytes, (writers, linked));")
        .expect("the Linux feed");
    let flushed = turn.find("d.flush_reply();").expect("the Linux flush");
    assert!(fed < flushed, "the Linux flush after the feed");
    let exits: Vec<&str> = turn[fed..flushed]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .filter(|l| l.contains("continue") || l.contains("return") || l.contains("break"))
        .collect();
    assert!(
        exits.iter().all(|l| l.contains("break 'run")),
        "only ways out that end the program: {exits:?}"
    );
}

/// **A launch on this path opens no window**: no code the launch tests
/// reach names a main-thread marker, NSApplication, NSWindow or
/// `baud_present_macos`, nor calls pty.rs's Accessibility read,
/// `prefers_steady_caret` (`crate::no_appkit`, which computes the reach:
/// from this file, app.rs fn by fn). One reach is allowed, by name and line
/// ([`ALLOWED`]).
#[test]
fn the_launch_path_names_no_appkit() {
    use crate::no_appkit::{Follow, Part, Walk, hits, reach};
    let src = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
    let file = |p: &[&str]| Part::File(p.iter().map(|s| s.to_string()).collect());
    let app = |f: &str| Part::Fn(vec!["app".to_string()], f.to_string());
    let reached = reach(&Walk {
        src,
        start: vec![file(&["app", "launch_tests"])],
        by_fn: &[(&["app"], Follow::FreeFns)],
        sinks: &[],
        marked: crate::no_appkit::Marked::Cut,
    });
    // The control: the walk follows `crate::`, `super::` and `use`, to
    // modules three deep and to app.rs's fns.
    for part in [
        file(&["host"]),
        file(&["pty"]),
        file(&["winops"]),
        file(&["probe"]),
        file(&["notify"]),
        app("theme_for"),
        app("frame_trail"),
    ] {
        assert!(
            reached.parts.contains_key(&part),
            "the walk reaches {part:?}"
        );
    }
    let steady = concat!("prefers_steady", "_caret(");
    assert_eq!(
        hits(&reached, &[steady], &ALLOWED),
        Vec::<String>::new(),
        "code the launch reaches names AppKit"
    );
    // The control: a use in code is caught, the same words in a comment
    // are not, and a module's tests block is cut.
    let marker = format!(
        "fn f() {{ let m = {}::new(); }}\n",
        crate::no_appkit::FORBIDDEN[0]
    );
    let caught = |text: &str| {
        crate::no_appkit::without_tests(&crate::no_appkit::code(text))
            .lines()
            .any(|l| crate::no_appkit::FORBIDDEN.iter().any(|f| l.contains(f)))
    };
    assert!(caught(&marker), "the control: code is caught");
    assert!(!caught(&format!("// {marker}")), "a comment is not");
    assert!(
        !caught(&format!("mod tests {{\n{marker}}}\n")),
        "a tests block is cut"
    );
    // An item `#[cfg(test)]` marks is cut; a marked field is no item, and
    // the code after it stays.
    assert!(
        !caught(&format!("#[cfg(test)]\n{marker}")),
        "a marked fn is cut"
    );
    assert!(
        caught(&format!(
            "struct S {{\n    #[cfg(test)]\n    f: u8,\n}}\n{marker}"
        )),
        "a marked field cuts nothing after it"
    );
    assert!(
        caught(&format!("mod tests {{\n    let s = \"{{\";\n}}\n{marker}")),
        "a brace in a string ends no block early, nor hides the code after it"
    );
}

/// **The reaches allowed**: the module, the code on the line, and why it
/// opens nothing.
pub(crate) const ALLOWED: [(&[&str], &str, &str); 1] = [crate::no_appkit::LINK_WORKER];

/// Refuses this thread's pty opens with `ENXIO`, macOS's answer at its
/// limit, over counts of 511 in use of 511, until dropped.
struct Refusing;

impl Refusing {
    fn at_the_limit() -> Refusing {
        use crate::pty::PtyCount;
        crate::pty::set_refusing(
            Some(crate::sys::Errno(crate::sys::ENXIO)),
            Some((Some(PtyCount(511)), Some(PtyCount(511)))),
        );
        Refusing
    }
}

impl Drop for Refusing {
    fn drop(&mut self) {
        crate::pty::set_refusing(None, None);
    }
}

/// **A window the system gives no terminal opens over why, and stays.**
/// At macOS's limit on ptys the open fails with `ENXIO`; the window opens
/// all the same, its terminal says the limit, the count, and how to close
/// terminals or raise `kern.tty.ptmx_max`, and it is still open after many
/// pumps: nothing ends it but the user. The open is tried once: a refusal
/// is not raced, and retrying it would fail again in microseconds.
#[test]
fn a_window_refused_its_terminal_opens_over_why_and_stays() {
    let ws = WinSize {
        ws_col: 80,
        ws_row: 24,
    };
    let refusing = Refusing::at_the_limit();
    let mut s = Session::start(&ws, "exit 0", launchd_environment(), Parent::Launchd);
    let tried = crate::pty::refused_opens();
    drop(refusing);
    assert_eq!(tried, crate::pty::Tries(1), "one open, no retry");
    for _ in 0..64 {
        s.pump();
    }
    assert_eq!(s.closed, Stream::Open, "the window stays");
    assert_eq!(
        s.screen_text()[..5],
        [
            "baud could not open a terminal: macOS allows 511 at once, and 511 are open.",
            "Close terminal windows or tabs you no longer need, then open a new window.",
            "Or raise the limit, up to 999: sudo sysctl kern.tty.ptmx_max=999",
            "To keep it after a restart, add kern.tty.ptmx_max=999 to /etc/sysctl.conf.",
            "",
        ]
    );
    assert_eq!(s.pty.child(), None, "no child to signal");
    assert!(s.pty.reaps().is_none(), "no child to reap");
    assert!(s.pty.watch(|| {}).is_none(), "no process watch");
}
