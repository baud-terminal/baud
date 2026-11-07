//! **The take's journal**: what a filmed take of the macOS window showed,
//! and when, for the sound laid under the film. Off unless `BAUD_TAKE`
//! names a file; then each row is one thing that reached the screen, a
//! line of seven tab-separated fields:
//!
//! ```text
//! host_ns  event  detail  x_px  y_px  x2_px  y2_px
//! ```
//!
//! `host_ns` is mach host time in nanoseconds, the clock every
//! [`HostTime`] is on (`latency.rs` cites each API's base), so the demo's
//! `--record` subtracts the recorder's first frame from it, whose time
//! ScreenCaptureKit gives on the same clock, and the row lands on the
//! film's own clock. A position is the window's pixels from its top left:
//! the window is FullSizeContentView, so its view spans the frame the film
//! is cropped to, and the grid starts at `pad_px` in it. An empty position
//! field is a row with none.
//!
//! **When a row's time is taken**, by event:
//!
//! | event | queued | stamped |
//! |---|---|---|
//! | `key`, `enter`, `paste` | the responder sending it | the frame that draws the first read after it is shown, at the cursor then |
//! | `trail` | the first frame that draws a trail after one that drew none, from the cursor the frame before to the cursor now | that frame shown |
//! | `fireworks` | each burst born, at its origin | its frame shown |
//! | `copy` | the selection copied, its two ends | the next frame shown |
//! | `picture` | the frame that first draws a placement, `shown`, or the first that draws none, `gone`, their union | that frame shown |
//! | `light` | the title strip's lamp changed, `CD on` | at once: a lamp is an image view's picture swapped, not a frame |
//!
//! "Shown" is the moment the layer took the frame's surface, the latency
//! instrument's `shown` leg: on the GPU path the view's completion, on the
//! CPU path and a frame drawn now the present itself. The panel shows it
//! at the next vsync, at most one display period later.
//!
//! **No row holds what was typed.** A key's row is its class, `key` or
//! `enter`, its time and the cursor's place, and its detail is `-`: a take
//! types at prompts like any session, a password at a `sudo` or `ssh`
//! prompt with echo off among them, and a file left armed would keep every
//! later session's keys. Nothing downstream wants the character: the
//! mixer reads the event, the time and x.
//!
//! **Only a build with the `take` feature has it.** Without the feature
//! -- every build but the demo's MP4 take -- [`Take::from_env`] reads no
//! variable and every window's journal is off, so a shipped baud cannot be
//! made to journal. The feature changes nothing else: `from_env` is the
//! one place it is read, and every hook only reads the app's state and
//! writes the journal, so what is drawn is the same either way.
//!
//! **One window writes it.** The first window a process opens claims the
//! file ([`Take::from_env`]); a later one journals nothing, so no window
//! truncates another's rows. `BAUD_TAKE` is not passed on to the shell
//! (`pty::OUTER`), so a baud started inside the take journals nothing
//! either.
//!
//! **What it costs**: off, every hook is one test of an `Option`; the
//! positions are handed in as closures, so nothing is computed. On, a row
//! is a line appended to the file, written when it is stamped.

use baud_domain::{HostTime, KeyCode, KeyEvent};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

/// The variable naming the journal's file.
pub(crate) const ENV: &str = "BAUD_TAKE";

/// Whether a window of this process has claimed the journal.
static CLAIMED: AtomicBool = AtomicBool::new(false);

/// A rectangle or a point of the window, in pixels from its top left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Pos {
    None,
    At(i64, i64),
    Span(i64, i64, i64, i64),
    /// The cursor's centre where the row is committed.
    Cursor,
}

impl Pos {
    fn fields(self) -> String {
        match self {
            Pos::None | Pos::Cursor => "\t\t\t".to_string(),
            Pos::At(x, y) => format!("{x}\t{y}\t\t"),
            Pos::Span(x, y, x2, y2) => format!("{x}\t{y}\t{x2}\t{y2}"),
        }
    }

    /// A rectangle's centre, `[x, y, w, h]`.
    pub(crate) fn centre(r: [i64; 4]) -> (i64, i64) {
        (r[0] + r[2] / 2, r[1] + r[3] / 2)
    }
}

/// One row waiting for its time.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct Note {
    pub(crate) event: &'static str,
    pub(crate) detail: String,
    pub(crate) pos: Pos,
}

/// The rows in each stage, and what the edges are measured against.
struct Open {
    out: Box<dyn Write>,
    /// Sent to the child, waiting for the read that answers them.
    echo: Vec<Note>,
    /// Waiting for a frame to commit.
    queued: Vec<Note>,
    /// In a committed frame, waiting for it to be shown.
    committed: Vec<Note>,
    /// Whether the last frame drew a trail, and the cursor's centre in it.
    trail: Drew,
    cursor: Option<(i64, i64)>,
    /// Whether the last frame drew a placement.
    pictures: Drew,
    /// Each lamp's state as last seen, by its letters.
    lamps: Vec<(&'static str, bool)>,
}

/// **Whether a frame drew a thing**: a trail, or a placement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Drew {
    Yes,
    No,
}

/// The journal: `None` when off.
pub(crate) struct Take {
    on: Option<Open>,
}

/// **A key's detail**: `-`, whatever was typed; see the module's doc.
const KEY_DETAIL: &str = "-";

/// A detail with nothing in it that would split a row or a line.
fn clean(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    match s.trim() {
        "" => "-".to_string(),
        t => t.to_string(),
    }
}

impl Take {
    /// On when built with the `take` feature and `BAUD_TAKE` names a file
    /// that can be created, for the first window of the process alone;
    /// without the feature, off, and nothing is read.
    pub(crate) fn from_env() -> Take {
        let file = match cfg!(feature = "take") {
            true => std::env::var_os(ENV)
                .filter(|_| !CLAIMED.swap(true, Ordering::Relaxed))
                .and_then(|p| std::fs::File::create(p).ok()),
            false => None,
        };
        match file {
            Some(f) => Take::to(Box::new(std::io::BufWriter::new(f))),
            None => Take { on: None },
        }
    }

    /// On, writing to `out`.
    pub(crate) fn to(out: Box<dyn Write>) -> Take {
        Take {
            on: Some(Open {
                out,
                echo: Vec::new(),
                queued: Vec::new(),
                committed: Vec::new(),
                trail: Drew::No,
                cursor: None,
                pictures: Drew::No,
                lamps: Vec::new(),
            }),
        }
    }

    pub(crate) fn on(&self) -> bool {
        self.on.is_some()
    }

    /// A key sent to the child: `enter` for Return, else `key`.
    pub(crate) fn key(&mut self, e: &KeyEvent) {
        let Some(j) = self.on.as_mut() else { return };
        let event = match e.code {
            KeyCode::Enter => "enter",
            _ => "key",
        };
        j.echo.push(Note {
            event,
            detail: KEY_DETAIL.to_string(),
            pos: Pos::Cursor,
        });
    }

    /// A paste sent to the child.
    pub(crate) fn paste(&mut self) {
        let Some(j) = self.on.as_mut() else { return };
        j.echo.push(Note {
            event: "paste",
            detail: "-".to_string(),
            pos: Pos::Cursor,
        });
    }

    /// The core was fed a read: what was sent before it is answered.
    pub(crate) fn fed(&mut self) {
        let Some(j) = self.on.as_mut() else { return };
        let echoed = std::mem::take(&mut j.echo);
        j.queued.extend(echoed);
    }

    /// A row for the next frame committed.
    pub(crate) fn note(&mut self, event: &'static str, detail: &str, pos: impl FnOnce() -> Pos) {
        let Some(j) = self.on.as_mut() else { return };
        j.queued.push(Note {
            event,
            detail: clean(detail),
            pos: pos(),
        });
    }

    /// **A frame drawn**, before it is committed: a trail that starts in
    /// it, from the cursor the frame before to `cursor` now, and a
    /// placement that comes or goes. `kind` names the trail.
    pub(crate) fn frame(
        &mut self,
        kind: &str,
        trail: Drew,
        cursor: impl FnOnce() -> (i64, i64),
        pictures: impl FnOnce() -> Option<[i64; 4]>,
    ) {
        let Some(j) = self.on.as_mut() else { return };
        let now = cursor();
        if trail == Drew::Yes && j.trail == Drew::No {
            let from = j.cursor.unwrap_or(now);
            j.queued.push(Note {
                event: "trail",
                detail: clean(kind),
                pos: Pos::Span(from.0, from.1, now.0, now.1),
            });
        }
        j.trail = trail;
        j.cursor = Some(now);
        let placed = pictures();
        match (placed, j.pictures) {
            (Some(r), Drew::No) => j.queued.push(Note {
                event: "picture",
                detail: "shown".to_string(),
                pos: Pos::Span(r[0], r[1], r[0] + r[2], r[1] + r[3]),
            }),
            (None, Drew::Yes) => j.queued.push(Note {
                event: "picture",
                detail: "gone".to_string(),
                pos: Pos::None,
            }),
            _ => {}
        }
        j.pictures = match placed {
            Some(_) => Drew::Yes,
            None => Drew::No,
        };
    }

    /// The frame was committed; `cursor` is the cursor's centre in it.
    pub(crate) fn committed(&mut self, cursor: impl FnOnce() -> (i64, i64)) {
        let Some(j) = self.on.as_mut() else { return };
        if j.queued.is_empty() {
            return;
        }
        let at = cursor();
        for mut n in std::mem::take(&mut j.queued) {
            if n.pos == Pos::Cursor {
                n.pos = Pos::At(at.0, at.1);
            }
            j.committed.push(n);
        }
    }

    /// The committed frame is on the layer as of `shown`: its rows are
    /// written.
    pub(crate) fn shown(&mut self, shown: impl FnOnce() -> HostTime) {
        let Some(j) = self.on.as_mut() else { return };
        if j.committed.is_empty() {
            return;
        }
        let t = shown();
        for n in std::mem::take(&mut j.committed) {
            let _ = writeln!(
                j.out,
                "{}\t{}\t{}\t{}",
                t.nanos(),
                n.event,
                n.detail,
                n.pos.fields()
            );
        }
        let _ = j.out.flush();
    }

    /// **The lamps as shown now**, each by its letters and whether it is
    /// lit: a lamp that changed is a row at `now`, written at once.
    pub(crate) fn lamps(
        &mut self,
        lamps: impl FnOnce() -> Vec<(&'static str, bool)>,
        now: impl FnOnce() -> HostTime,
    ) {
        let Some(j) = self.on.as_mut() else { return };
        let seen = lamps();
        let mut changed = Vec::new();
        for &(name, lit) in &seen {
            let was = j.lamps.iter().find(|(n, _)| *n == name).map(|&(_, l)| l);
            if was.unwrap_or(false) != lit {
                changed.push((name, lit));
            }
        }
        j.lamps = seen;
        if changed.is_empty() {
            return;
        }
        let t = now();
        for (name, lit) in changed {
            let state = if lit { "on" } else { "off" };
            let _ = writeln!(
                j.out,
                "{}\tlight\t{name} {state}\t{}",
                t.nanos(),
                Pos::None.fields()
            );
        }
        let _ = j.out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::{Drew, Pos, Take};
    use baud_domain::{Codepoint, HostTime, KeyCode, KeyEvent, Modifiers};
    use std::ffi::OsString;
    use std::sync::{Arc, Mutex};

    /// A writer the test reads back.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Shared {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn key(code: KeyCode, text: Option<char>, mods: Modifiers) -> KeyEvent {
        let mut e = KeyEvent::press(code, mods);
        e.text = text.map(Codepoint::from_char);
        e
    }

    /// **A key is stamped by the frame that draws its echo, at the cursor
    /// there**: a frame shown before the read leaves it waiting, and the
    /// lamp's row is written at once. Perturbation: move what `fed` moves
    /// in `key` itself, and the key takes the first frame's time.
    #[test]
    fn a_key_takes_the_time_of_the_frame_that_draws_its_echo() {
        let out = Shared::default();
        let mut t = Take::to(Box::new(out.clone()));
        let at = |ns| HostTime::of_nanos(ns);
        t.key(&key(
            KeyCode::Char(Codepoint::from_char('h')),
            Some('h'),
            Modifiers::NONE,
        ));
        t.key(&key(
            KeyCode::Char(Codepoint::from_char(' ')),
            Some(' '),
            Modifiers::NONE,
        ));
        t.frame("plasma", Drew::No, || (10, 20), || None);
        t.committed(|| (10, 20));
        t.shown(|| at(1_000));
        t.fed();
        t.key(&key(KeyCode::Enter, None, Modifiers::NONE));
        t.frame("plasma", Drew::Yes, || (34, 20), || Some([0, 100, 50, 40]));
        t.committed(|| (34, 20));
        t.shown(|| at(2_000));
        t.lamps(|| vec![("CD", true)], || at(2_500));
        t.lamps(|| vec![("CD", true)], || at(2_600));
        let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
        assert_eq!(
            text,
            "2000\tkey\t-\t34\t20\t\t\n\
             2000\tkey\t-\t34\t20\t\t\n\
             2000\ttrail\tplasma\t10\t20\t34\t20\n\
             2000\tpicture\tshown\t0\t100\t50\t140\n\
             2500\tlight\tCD on\t\t\t\t\n"
        );
        assert_eq!(Pos::centre([10, 20, 4, 8]), (12, 24));
    }

    /// **No key row holds what was typed**: a password's characters,
    /// typed as a session at a prompt with echo off would, and the controls
    /// and whitespace beside them, journal as rows whose detail is `-`, and
    /// none of the characters is anywhere in the file; and `BAUD_TAKE` is
    /// not in a shell's environment, so a baud inside the take journals
    /// nothing. Perturbation: give a key row the character it typed, and
    /// the password is in the file.
    #[test]
    fn no_key_row_holds_what_was_typed() {
        let out = Shared::default();
        let mut t = Take::to(Box::new(out.clone()));
        // No digit: the row's time and place are digits.
        let typed = "Zq#wX!";
        for c in typed.chars() {
            let code = KeyCode::Char(Codepoint::from_char(c.to_ascii_lowercase()));
            t.key(&key(code, Some(c), Modifiers::NONE));
        }
        t.key(&key(KeyCode::Tab, Some('\t'), Modifiers::NONE));
        let d = KeyCode::Char(Codepoint::from_char('d'));
        t.key(&key(d, None, Modifiers::CTRL));
        t.key(&key(KeyCode::Enter, None, Modifiers::NONE));
        t.fed();
        t.committed(|| (5, 6));
        t.shown(|| HostTime::of_nanos(7));
        let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
        let rows: Vec<Vec<&str>> = text.lines().map(|l| l.split('\t').collect()).collect();
        // The secret as the rows would spell it, a character a row.
        let spelt: String = rows.iter().filter_map(|r| r.get(2).copied()).collect();
        assert!(
            !spelt.contains(typed) && !text.contains(typed),
            "the secret is in the journal: {text}"
        );
        assert_eq!(rows.len(), typed.len() + 3, "{text}");
        for r in &rows {
            assert_eq!(r.len(), 7, "{r:?}");
            assert_eq!(r[2], "-", "a key row's detail: {r:?}");
        }
        assert_eq!(rows.last().map(|r| r[1]), Some("enter"));
        for c in typed.chars() {
            assert!(!text.contains(c), "{c:?} is in the journal: {text}");
        }
        // The feature is read in `from_env` alone: none of the files the
        // hooks live in reads it, so it cannot change what is drawn.
        let feature = concat!("feature = ", "\"take\"");
        for (name, src) in [
            ("app.rs", include_str!("app.rs")),
            ("fireworks.rs", include_str!("fireworks.rs")),
            ("pty.rs", include_str!("pty.rs")),
        ] {
            assert!(!src.contains(feature), "{name} reads the take feature");
        }
        assert_eq!(include_str!("take.rs").matches(feature).count(), 1);
        let env = crate::pty::child_env(
            vec![
                (OsString::from(super::ENV), OsString::from("/tmp/take.tsv")),
                (OsString::from("HOME"), OsString::from("/nowhere")),
            ],
            None,
        );
        let env: Vec<String> = env
            .iter()
            .map(|e| e.to_string_lossy().into_owned())
            .collect();
        assert!(
            env.iter().any(|e| e == "HOME=/nowhere"),
            "the control: {env:?}"
        );
        assert!(!env.iter().any(|e| e.starts_with("BAUD_TAKE=")), "{env:?}");
        assert_eq!(super::clean("a\tb\n"), "a b");
    }
}
