//! **The link layer's sequences, written once**: what the macOS app does with
//! printed paths and links at each read, each tick of the clock, each wake
//! of the paths' worker, each frame, and when a link is opened. The app
//! calls these, and so does the film's tap (`film/tap.rs`), so what the tap
//! records is the app's own sequence, in its order and with its arguments --
//! the job watch's verdict among them, which a copy could drop.
//!
//! Nothing here opens anything: a link's [`decide`] says what a click comes
//! to, and the driver acts on it.

use baud_core::Term;
use baud_render::{LineStyle, Scrolled};

use crate::bell::Moment;
use crate::lined::{Lines, Uris};
use crate::link::{Click, Host, Refused, Target, Under};
use crate::paths::{LineSpan, Links, Moved, Paths};
use crate::probe::{Asks, Linked, Probes, Sampler, Seen, Turn, WatchCell, Writers};
use crate::rules::Run;

/// **The parts of a window the link layer reads and keeps**, borrowed for
/// one call: the terminal, its scroll, the lines at rest, the URIs' and the
/// paths' state, and the `[link]` config.
pub(crate) struct Layer<'a, 't> {
    pub(crate) term: &'a Term<'t>,
    pub(crate) scrolled: Scrolled,
    pub(crate) lines: &'a mut Lines,
    pub(crate) uris: &'a mut Uris,
    pub(crate) paths: &'a mut Paths,
    pub(crate) config: &'a crate::config::Link,
}

impl<'a, 't> Layer<'a, 't> {
    /// The worker's view of the window.
    fn seen(&mut self) -> Seen<'_, 't> {
        Seen {
            term: self.term,
            scrolled: self.scrolled,
            lines: self.lines,
            uris: self.uris,
            urls: self.config.urls_read(),
            paths: self.paths,
        }
    }
}

/// **At each `read(2)`, on the thread taking the reads**: which program
/// wrote the bytes, the foreground group sampled now, and what the process
/// watch saw since the last read that asked, taken (`probe::Sampler`).
pub(crate) fn read(
    sampler: &mut Sampler,
    now: std::time::Instant,
    bytes: &[u8],
    master: crate::sys::Fd,
    watched: &WatchCell,
) -> (Writers, Linked) {
    match sampler.read(now, bytes) {
        Asks::Yes(l) => (sampler.writers(crate::fg::group(master), watched.take()), l),
        Asks::No => (Writers::default(), Linked::No),
    }
}

/// **After a batch of reads was fed**: each read's writers kept with the
/// lines the batch wrote, `span` (`Probes::read_taken`).
pub(crate) fn fed<T: Moment>(
    probes: &mut Probes<T>,
    paths: &mut Paths,
    reads: impl IntoIterator<Item = (Writers, Linked)>,
    span: LineSpan,
) {
    for (writers, linked) in reads {
        probes.read_taken(paths, writers, span, linked);
    }
}

/// **The clock's half of a turn, at `now`**: the worker's hang watched and
/// a batch sent once one is due (`Probes::tick`), the foreground group read
/// from `master` only when a batch goes.
pub(crate) fn tick<T: Moment>(
    probes: &mut Probes<T>,
    layer: &mut Layer<'_, '_>,
    now: T,
    master: crate::sys::Fd,
) -> Turn {
    probes.tick(now, layer.seen(), || crate::fg::group(master))
}

/// **The worker's answers, merged** on its wake (`Probes::answers`).
pub(crate) fn answers<T: Moment>(probes: &mut Probes<T>, layer: &mut Layer<'_, '_>) -> Turn {
    let mut seen = layer.seen();
    probes.answers(&mut seen)
}

/// **The lines at rest for a frame** (`lined.rs`): the reported folder
/// observed before the rows are read (`Paths::observe`), then the rows
/// scanned. The style to draw them in, or none with `[link] line` off, when
/// no line is kept.
pub(crate) fn frame(layer: &mut Layer<'_, '_>) -> Option<LineStyle> {
    match layer.config.lines() {
        Some(style) => {
            if layer.paths.observe(layer.term, layer.scrolled) == Moved::Yes {
                layer.lines.forget();
            }
            layer.lines.scan(
                layer.term,
                layer.scrolled,
                &mut Links {
                    uris: layer.uris,
                    urls: layer.config.urls_read(),
                    paths: layer.paths,
                },
            );
            Some(style)
        }
        None => {
            layer.lines.off();
            None
        }
    }
}

/// **What a click on a link comes to**, decided and not acted on.
pub(crate) enum Decision<'a> {
    /// A printed path: asked of the worker again, and decided on its answer
    /// (`paths::on_found`).
    Path,
    /// An `OSC 8` link whose slot a sweep freed since it was read.
    Gone,
    /// A URI read from the text that is no URI.
    Unreadable(Refused, &'a [u8]),
    /// A URI, and what admitting it came to.
    Read { uri: &'a [u8], then: Then<'a> },
}

/// **What an admitted URI comes to**, with what a rule built for it.
pub(crate) enum Then<'a> {
    Refused(Refused),
    /// A rule's `copy`: the link's text.
    Copy(String),
    /// A rule's command, built, never started here.
    Command(Option<Result<std::process::Command, Refused>>),
    /// A rule's window: its argv.
    Window(Option<Result<Run, Refused>>),
    /// Its file, which the worker found gone, asked about again.
    Again(Vec<u8>),
    /// Opened by the system, or revealed: the target, for the driver.
    OpenOrReveal(Target<'a>),
}

/// **A click on `link` at `line`, decided**: its URI admitted with the
/// origin of the line it was printed on (`Paths::printed_origin`), then
/// what the click comes to (`link::click`) and what its rule builds.
pub(crate) fn decide<'a>(
    link: &'a Under,
    line: baud_render::AbsLine,
    term: &'a Term<'_>,
    paths: &Paths,
    uris: &Uris,
) -> Decision<'a> {
    let uri = match link {
        Under::Path(_) => return Decision::Path,
        Under::Osc8(l) => match l.uri(term) {
            Some(uri) => uri,
            None => return Decision::Gone,
        },
        Under::Bare(b) => match b.uri() {
            Ok(uri) => uri,
            Err(why) => return Decision::Unreadable(why, b.text()),
        },
    };
    let origin = paths.printed_origin(link, line);
    let then = match crate::link::admit_printed(uri, Host(uris.host()), uris.rules(), origin) {
        Err(why) => Then::Refused(why),
        Ok(t) => match crate::link::click(&t, paths) {
            Click::Copy => Then::Copy(t.as_str().to_string()),
            Click::Command => {
                Then::Command(t.rule().and_then(|ix| uris.rules().command(ix, t.as_str())))
            }
            Click::Window => {
                Then::Window(t.rule().and_then(|ix| uris.rules().window(ix, t.as_str())))
            }
            Click::Again(path) => Then::Again(path),
            Click::Open | Click::Reveal => Then::OpenOrReveal(t),
        },
    };
    Decision::Read { uri, then }
}

#[cfg(test)]
mod tests {
    use crate::probe::{Linked, Sampler, WatchCell, Watched};

    /// **A read carries the process watch's verdict, taken**: what the
    /// app's reader and the film's tap both pass on, which a copy of the
    /// sequence could drop for `Watched::Nothing` -- R1's shape, a job that
    /// wrote and ended between two reads. A read carrying a link's
    /// introducer asks for the group whatever the pace. The control: with
    /// nothing raised, nothing is passed on. Named break: pass
    /// `Watched::Nothing` in [`super::read`].
    #[test]
    fn a_read_carries_the_process_watchs_verdict() {
        let link: &[u8] = b"\x1b]8;;file:///etc/hosts\x1b\\hosts\x1b]8;;\x1b\\";
        let cell = WatchCell::default();
        let mut s = Sampler::default();
        let (w, l) = super::read(&mut s, std::time::Instant::now(), link, -1, &cell);
        assert_eq!(
            (w.watched, l),
            (Watched::Nothing, Linked::Yes),
            "the control"
        );
        cell.raise();
        let mut s = Sampler::default();
        let (w, _) = super::read(&mut s, std::time::Instant::now(), link, -1, &cell);
        assert_eq!(w.watched, Watched::Elsewhere, "the watch's verdict");
        assert_eq!(cell.take(), Watched::Nothing, "taken by the read");
    }
}
