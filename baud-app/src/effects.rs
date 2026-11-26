//! **The timed effects both drivers draw, as pure functions of a clock**:
//! blinking text's two phases and when either next flips, the cursor a
//! trail follows, and where the window sits for the scrollbar's thumb. The
//! macOS driver reads them off the display link's timestamp; the Linux
//! driver off seconds since it started, and wakes its one wait for the next
//! flip (`timed.rs`). Pure, so the tests run on every platform.

use baud_core::Term;
use baud_domain::{Chan, Col, RegisPhase, Row, Rows};
use baud_render::Blinking;
use baud_render::{Blink, BlinkPhases, GuideLook, Window};

/// Half a slow blink cycle, milliseconds: text with `SGR 5` shows for this
/// long, then hides for this long -- 75 blinks a minute. `[[source: ecma48]]`
/// § 8.3.117 gives 5 as "slowly blinking (less then 150 per minute)".
const BLINK_HALF_MS: u32 = 400;
/// Half a rapid blink cycle, milliseconds: text with `SGR 6` shows for this
/// long, then hides for this long -- 150 blinks a minute, 2.5 a second, the
/// floor of § 8.3.117's "rapidly blinking (150 per minute or more)". Under
/// WCAG 2.3.1's "more than three times in any one second period"
/// (`[[source: wcag22]]`), whose flash is "a pair of opposing changes": one
/// shown-and-hidden cycle is one flash.
const RAPID_HALF_MS: u32 = 200;
// The two bounds, in integers: 150 a minute is exactly the rapid floor, so a
// float comparison would sit on a rounding edge.
const _: () = assert!(
    60_000 < 150 * 2 * BLINK_HALF_MS,
    "SGR 5 is slow blink: fewer than 150 a minute"
);
const _: () = assert!(
    60_000 >= 150 * 2 * RAPID_HALF_MS,
    "SGR 6 is rapid blink: 150 a minute or more"
);
/// The two halves in seconds, the display link's unit.
pub(crate) const BLINK_HALF: f64 = BLINK_HALF_MS as f64 / 1000.0;
pub(crate) const RAPID_HALF: f64 = RAPID_HALF_MS as f64 / 1000.0;

/// The phases blinking text draws with at display-link time `target`, one a
/// rate: the clock's, unless the system asks for reduced motion, when both
/// are held shown. Apple's HIG names blinking under Reduce Motion, and WCAG
/// 2.2.2 asks for a way to stop blinking that lasts past five seconds; this
/// is that way, and the reasons are on
/// `reduce_motion_holds_blinking_text_shown`. Held, no phase flips, so a
/// blinking cell owes no frame. Each rate shows for half its cycle, as
/// `SGR 5` always did: the text gives a rate and no duty cycle.
pub(crate) fn text_blink(target: f64, motion: crate::config::SystemMotion) -> BlinkPhases {
    use crate::config::SystemMotion;
    let at = |half: f64| match (target / half).rem_euclid(2.0) < 1.0 {
        true => Blink::Shown,
        false => Blink::Hidden,
    };
    match motion {
        SystemMotion::Reduced => BlinkPhases::SHOWN,
        SystemMotion::Full => BlinkPhases {
            slow: at(BLINK_HALF),
            rapid: at(RAPID_HALF),
        },
    }
}

/// How far past a flip's instant the wake for it lands, seconds: the phase
/// is read in floating point, where `k * 0.2 / 0.2` can fall a hair short
/// of `k`, and a wake that read the old phase would find its own deadline
/// already behind it and wait for nothing else. A millisecond is far under
/// a frame.
const PAST_FLIP: f64 = 0.001;

/// **When blinking text next changes, at or after `target`**: the earliest
/// flip of a rate `seen` says is on screen, a millisecond past it
/// ([`PAST_FLIP`]), or none -- nothing blinks on screen, or reduced motion
/// holds both phases shown, so no wait is bounded by a blink. A slow-only
/// screen wakes at the slow flips alone, as the macOS tick repaints only
/// for a rate on screen (`Blinking::flips`).
pub(crate) fn next_flip(
    target: f64,
    seen: Blinking,
    motion: crate::config::SystemMotion,
) -> Option<f64> {
    if motion == crate::config::SystemMotion::Reduced {
        return None;
    }
    let flip = |half: f64| {
        let mut k = (target / half).floor() + 1.0;
        while k * half + PAST_FLIP <= target {
            k += 1.0;
        }
        k * half + PAST_FLIP
    };
    [(Blinking::SLOW, BLINK_HALF), (Blinking::RAPID, RAPID_HALF)]
        .into_iter()
        .filter(|&(rate, _)| seen.with(rate) == seen)
        .map(|(_, half)| flip(half))
        .reduce(f64::min)
}

/// **How the cursor guide is drawn**, `None` while it is hidden: iTerm2's
/// default colour, and its one-point line as `scale` device pixels
/// rounded, from one to four.
pub(crate) fn guide_look(guide: baud_domain::CursorGuide, scale: f64) -> Option<GuideLook> {
    let line = (1..=4u32)
        .rev()
        .find(|&n| f64::from(n) <= scale.round())
        .unwrap_or(1);
    match guide {
        baud_domain::CursorGuide::Shown => Some(GuideLook {
            colour: baud_render::CURSOR_GUIDE,
            line: baud_font::Px::of(line),
        }),
        baud_domain::CursorGuide::Hidden => None,
    }
}

/// **What the app knows of a session that the core does not**, for the
/// variables a badge names (`[[source: iterm2-doc-variables]]`): the
/// root process and its device, the job in the foreground, its name and
/// its directory, the window's title as shown; this machine's user and
/// host, which a session's `username` and `hostname` are until a program
/// names others, as iTerm2 starts a session with `NSUserName()` and the
/// machine's name (`[[source: iterm2-964a75d-ptysession]]`); and the
/// home folder, the shell's name, the system's name and when the session
/// started, which iTerm2 sets as every session starts. Each driver brings
/// the first part up to date with the title it sends.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Session {
    pub(crate) pid: Option<i32>,
    pub(crate) tty: Option<String>,
    pub(crate) job: Option<i32>,
    pub(crate) job_name: Option<String>,
    pub(crate) dir: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) user: Option<String>,
    pub(crate) host: Option<String>,
    pub(crate) home: Option<String>,
    pub(crate) shell: Option<String>,
    pub(crate) uname: Option<String>,
    pub(crate) created: Option<String>,
}

impl Session {
    /// **A session on this machine, starting now**: its user, `$USER` or
    /// `$LOGNAME`, and its name as `gethostname` gives it, where iTerm2
    /// starts a session's `hostname` with `[NSHost
    /// fullyQualifiedDomainName]` (`didFinishInitialization`), which can
    /// differ -- `name.local` against a name the DNS gives; the home
    /// folder, `$HOME`, where iTerm2 takes `NSHomeDirectory()`; the name
    /// of the shell a session starts, `program` when `[shell] program`
    /// names one (`pty::shell_name`); the system's
    /// name as `uname` gives it, as iTerm2's `bestGuessAtUName` does
    /// without ssh integration; and the time, as [`creation_time`] writes
    /// it.
    pub(crate) fn here(program: Option<&str>) -> Session {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        Session {
            user: std::env::var("USER")
                .or_else(|_| std::env::var("LOGNAME"))
                .ok()
                .filter(|u| !u.is_empty()),
            host: crate::hostlog::machine_name(),
            home: std::env::var("HOME").ok().filter(|h| !h.is_empty()),
            shell: Some(crate::pty::shell_name(program)),
            uname: crate::hostlog::system_name(),
            created: Some(creation_time(&crate::hostlog::local(now))),
            ..Session::default()
        }
    }
}

/// **When a session started, as iTerm2 writes it**: `yyyyMMdd_HHmmss` in
/// local time, the format `initSynthetic:` gives its date formatter for
/// `creationTimeString` (`[[source: iterm2-964a75d-ptysession]]`), in the
/// Gregorian calendar, where iTerm2's formatter takes the user's.
pub(crate) fn creation_time(at: &crate::sys::Tm) -> String {
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        i64::from(at.tm_year) + 1900,
        at.tm_mon + 1,
        at.tm_mday,
        at.tm_hour,
        at.tm_min,
        at.tm_sec
    )
}

/// **What iTerm2 draws for a badge that names itself**: the format "is
/// replaced with" this, as `updateBadgeLabel` replaces a cycle (`[[source:
/// iterm2-964a75d-ptysession]]`).
pub(crate) const CYCLE: &str = "[Cycle detected]";

/// **The value of the variable `path` names**, `None` for an undefined
/// one: a user variable as `SetUserVar` kept it, the screen's size and
/// modes, the titles, the bells, the host, user and directory a program
/// named last -- by `OSC 7` or `RemoteHost` and `CurrentDir` -- else the
/// session's own, and what `session` holds. A variable baud does not
/// answer ([`baud_domain::Answer`]) is undefined.
pub(crate) fn badge_variable(
    term: &Term<'_>,
    session: &Session,
    selection: &dyn Fn() -> Option<String>,
    path: &str,
) -> Option<baud_domain::Value> {
    use baud_domain::{GlobalVariable as G, Named, SessionVariable as V, Value};
    let text = |b: &[u8]| Value::Text(String::from_utf8_lossy(b).into_owned());
    let number = |n: f64| Some(Value::Number(n));
    let owned = |s: &Option<String>| s.clone().map(Value::Text);
    let count = |n: usize| number(f64::from(u32::try_from(n).unwrap_or(u32::MAX)));
    let pid = |p: Option<i32>| p.and_then(|p| number(f64::from(p)));
    let title = |b: &[u8], e| match b.is_empty() {
        true => None,
        false => Some(Value::Text(crate::title::window_title(b, e, ""))),
    };
    match Named::of(path) {
        Named::User(name) => term.user_var(name.as_bytes()).map(text),
        Named::Global(g) => match g {
            G::Pid => number(f64::from(std::process::id())),
            G::LocalhostName => owned(&session.host),
            G::AppBundlePath | G::EffectiveTheme => None,
        },
        Named::Other => None,
        Named::Session(v) => match v {
            V::Name | V::PresentationName => owned(&session.title),
            V::TerminalIconName => title(term.icon_name(), term.icon_name_encoding()),
            V::TerminalWindowName => title(term.title(), term.title_encoding()),
            V::Columns => count(term.geometry().0.index()),
            V::Rows => count(term.geometry().1.index()),
            V::JobName => owned(&session.job_name),
            V::JobPid => pid(session.job),
            V::Pid => pid(session.pid),
            V::Tty => owned(&session.tty),
            V::HomeDirectory => owned(&session.home),
            V::Shell => owned(&session.shell),
            V::Uname => owned(&session.uname),
            V::CreationTimeString => owned(&session.created),
            // -1 not reported, 0 clicks, 1 highlight tracking, 2 clicks and
            // drags, 3 every motion; DEC's locator reports clicks.
            V::MouseReportingMode => number(match term.mouse_tracking() {
                baud_domain::MouseTracking::Off => -1.0,
                baud_domain::MouseTracking::X10Compat
                | baud_domain::MouseTracking::Normal
                | baud_domain::MouseTracking::Locator => 0.0,
                baud_domain::MouseTracking::Highlight => 1.0,
                baud_domain::MouseTracking::ButtonEvent => 2.0,
                baud_domain::MouseTracking::AnyEvent => 3.0,
            }),
            V::Selection => Some(Value::Text(selection().unwrap_or_default())),
            V::SelectionLength => count(selection().map_or(0, |s| s.len())),
            // baud has no ssh integration: "0: No ssh integration".
            V::SshIntegrationLevel => number(0.0),
            V::ApplicationKeypad => number(match term.keypad() {
                baud_domain::KeypadMode::Application => 1.0,
                baud_domain::KeypadMode::Numeric => 0.0,
            }),
            V::BellCount => count(term.bells),
            V::Hostname => match term.session_host() {
                baud_core::SessionHost::There { host, .. } => host.map(text),
                baud_core::SessionHost::Here => owned(&session.host),
            },
            V::Username => match term.session_host() {
                baud_core::SessionHost::There { user, .. } => user.map(text),
                baud_core::SessionHost::Here => owned(&session.user),
            },
            V::Path => match term.working_directory() {
                Some(d) => Some(text(d.path)),
                None => owned(&session.dir),
            },
            // A cycle, drawn as iTerm2 draws one before any is evaluated.
            V::Badge => None,
            V::AutoNameFormat
            | V::AutoName
            | V::TriggerName
            | V::CommandLine
            | V::DeepestJob
            | V::ParentSession
            | V::ProcessTitle
            | V::Termid
            | V::MouseInfo
            | V::LastCommand
            | V::AutoLogId
            | V::LogFilename
            | V::TmuxClientName
            | V::TmuxPaneTitle
            | V::TmuxRole
            | V::TmuxStatusLeft
            | V::TmuxStatusRight
            | V::TmuxWindowPane
            | V::TmuxWindowTitle
            | V::TmuxWindowPaneIndex
            | V::Id
            | V::ProfileName => None,
        },
    }
}

/// **The badge's text**: the format a program set evaluated as iTerm2
/// evaluates an interpolated string (`baud_domain::evaluate`), its
/// variables read by [`badge_variable`]; a format naming the badge itself
/// is [`CYCLE`], and one whose evaluation fails is a bug and the reason,
/// as iTerm2's badge shows "🐞" and the error (`[[source:
/// iterm2-964a75d-ptysession]]`).
pub(crate) fn badge_text(
    format: &str,
    variable: &dyn Fn(&str) -> Option<baud_domain::Value>,
) -> String {
    let mut cycle = false;
    baud_domain::interpolated_parts(format, |p| {
        if let baud_domain::Interpolated::Expression(e) = p {
            cycle |= baud_domain::names(e, |path| {
                baud_domain::Named::of(path)
                    == baud_domain::Named::Session(baud_domain::SessionVariable::Badge)
            });
        }
    });
    if cycle {
        return CYCLE.to_string();
    }
    match baud_domain::evaluate(format, variable) {
        Ok(text) => text,
        Err(fault) => format!("\u{1F41E} {fault}"),
    }
}

/// **How the badge is drawn**, `None` while there is none to draw: the
/// format a program set (`Term::badge_format`) as [`badge_text`] makes
/// it, in iTerm2's default colour or the one `SetColors=badge=` set,
/// opaque as iTerm2 makes it, outlined in `page`, the background colour,
/// inside the alpha iTerm2 draws the badge at, in iTerm2's box and margins
/// at `scale` device pixels a point, in bold. Nothing while `[screen]
/// badge` is off, and nothing for a text of white space alone.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn badge_look(
    term: &Term<'_>,
    shown: crate::config::BadgeShown,
    session: &Session,
    selection: &dyn Fn() -> Option<String>,
    page: baud_render::Argb,
    scale: f64,
) -> Option<baud_render::BadgeLook> {
    badge_of(term, shown, session, selection).map(|b| b.look(page, scale))
}

/// **What the badge says**: its text and the colour a program set for it,
/// read from the terminal and the session; how it is drawn is
/// [`BadgeText::look`]'s.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct BadgeText {
    text: String,
    colour: Option<(u8, u8, u8)>,
}

/// **The badge's text and colour**, or nothing while `[screen] badge` is
/// off or the text is white space alone.
pub(crate) fn badge_of(
    term: &Term<'_>,
    shown: crate::config::BadgeShown,
    session: &Session,
    selection: &dyn Fn() -> Option<String>,
) -> Option<BadgeText> {
    if shown == crate::config::BadgeShown::Off {
        return None;
    }
    let format = term.badge_format()?;
    let text = badge_text(format, &|path| {
        badge_variable(term, session, selection, path)
    });
    if text.chars().all(char::is_whitespace) {
        return None;
    }
    Some(BadgeText {
        text,
        colour: term.badge_colour().and_then(|c| c.as_rgb()),
    })
}

impl BadgeText {
    /// **The badge as drawn**: outlined in `page`, the background colour,
    /// at `scale` device pixels a point.
    pub(crate) fn look(self, page: baud_render::Argb, scale: f64) -> baud_render::BadgeLook {
        let (colour, drawn_at) = match self.colour {
            Some((r, g, b)) => (
                baud_render::Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)),
                Chan::from_u8(0xFF),
            ),
            None => (baud_render::BADGE, baud_render::DRAWN_AT),
        };
        let s = scale as f32;
        let px = |points: f32| {
            baud_font::Px::of(u32::try_from((points * s).round() as i64).unwrap_or(0))
        };
        baud_render::BadgeLook {
            text: self.text,
            colour,
            outline: page.with_alpha(drawn_at),
            drawn_at,
            fraction: baud_render::BOX,
            top: px(baud_render::MARGINS.0),
            right: px(baud_render::MARGINS.1),
            scale: s,
            style: baud_render::FaceStyle::Bold,
        }
    }
}

/// The phase a ReGIS page's blinking pixels show: text blink's slow one,
/// its faint off phase under dimmed flashing lights faint as the text's.
pub(crate) fn regis_phase(slow: Blink) -> RegisPhase {
    match slow {
        Blink::Shown => RegisPhase::Shown,
        Blink::Hidden => RegisPhase::Hidden,
        Blink::Faint => RegisPhase::Faint,
    }
}

/// How long the alternate-screen veil takes to reveal the new screen,
/// seconds: the switch's form under Reduce Motion. Only paid on a screen
/// switch when the transition is on.
pub(crate) const TRANSITION_S: f64 = 0.15;

/// **How a screen switch is drawn**: the new screen sliding in over the
/// old one toward the screen it brought up, or, under Reduce Motion, the
/// veil -- a dissolve in place of the motion.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum SwitchForm {
    Slide(baud_render::Toward),
    Veil,
}

/// The form a switch to `screen` takes under `motion`.
pub(crate) fn switch_form(
    screen: baud_core::Screen,
    motion: crate::config::SystemMotion,
) -> SwitchForm {
    match motion {
        crate::config::SystemMotion::Reduced => SwitchForm::Veil,
        crate::config::SystemMotion::Full => SwitchForm::Slide(match screen {
            baud_core::Screen::Alternate => baud_render::Toward::Alternate,
            baud_core::Screen::Normal => baud_render::Toward::Primary,
        }),
    }
}

/// **A switch's frame at `secs`** on the display link's clock, or `None`
/// once it has settled. The slide: for [`baud_render::SLIDE_S`] the new
/// screen slides in under the faint veil, and then the veil fades off over
/// [`TRANSITION_S`]. The veil alone, under Reduce Motion: the fade over
/// [`TRANSITION_S`].
pub(crate) fn switch_at(start: f64, form: SwitchForm, secs: f64) -> Option<SwitchFrame> {
    let t = (secs - start).max(0.0);
    let slide_s = f64::from(baud_render::SLIDE_S);
    match form {
        SwitchForm::Slide(toward) if t < slide_s => Some(SwitchFrame {
            slide: Some(baud_render::ScreenSlide {
                progress: (t / slide_s) as f32,
                toward,
            }),
            veil: Some(baud_render::Transition::faint(0.0)),
        }),
        SwitchForm::Slide(_) if t < slide_s + TRANSITION_S => Some(SwitchFrame {
            slide: None,
            veil: Some(baud_render::Transition::faint(
                ((t - slide_s) / TRANSITION_S) as f32,
            )),
        }),
        SwitchForm::Veil if t < TRANSITION_S => Some(SwitchFrame {
            slide: None,
            veil: Some(baud_render::Transition::fade((t / TRANSITION_S) as f32)),
        }),
        SwitchForm::Slide(_) | SwitchForm::Veil => None,
    }
}

/// Whether a CPU frame was composed from a slide's two frames.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Composed {
    No,
    Yes,
}

/// A switch's frame: where the slide is, if it is still sliding, and the
/// veil over the new screen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct SwitchFrame {
    pub(crate) slide: Option<baud_render::ScreenSlide>,
    pub(crate) veil: Option<baud_render::Transition>,
}

/// Where the window sits in the ring, for the scrollback's thumb: the
/// core's window as drawn (`Term::window_back`), which is the live screen on
/// the alternate screen, so the thumb, the cursor, a selection and the link
/// under a click agree with what is drawn.
pub(crate) fn window_of(term: &Term<'_>) -> Window {
    Window {
        // What the live surface can reach, by the same clamp: nothing on the
        // alternate screen, so no thumb is drawn there even at the bottom
        // of its travel, where a reveal would otherwise show one over the
        // primary's history.
        retained: term.looking_back(Rows::of_count(term.scrollback().len())),
        back: term.window_back(),
        rows: term.viewport().rows(),
    }
}

/// The cursor a trail follows, while the window is on the live screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TrailCursor {
    /// The cursor as it is drawn.
    Shown(Row, Col),
    /// Where a program put the cursor before it hid it
    /// (`Term::placed_cursor`): helix's selection, where helix draws its own
    /// block. Never drawn here -- the program hid it.
    Placed(Row, Col),
}

impl TrailCursor {
    pub(crate) fn cell(self) -> (Row, Col) {
        match self {
            TrailCursor::Shown(r, c) | TrailCursor::Placed(r, c) => (r, c),
        }
    }

    /// The shape the trail arrives at: the drawn cursor's own, and the whole
    /// cell for a placement, because the program draws a block there whatever
    /// shape the terminal's cursor is configured to -- the smear's hole has to
    /// be that block, or a bar-shaped hole leaves the trail over it.
    pub(crate) fn style(self, drawn: baud_core::CursorStyle) -> baud_core::CursorStyle {
        match self {
            TrailCursor::Shown(..) => drawn,
            TrailCursor::Placed(..) => baud_core::CursorStyle::SteadyBlock,
        }
    }
}

/// **The cursor a trail follows**: the one drawn, and while it is hidden,
/// where a program placed it before hiding it. Helix hides the terminal's
/// cursor for good and draws its own at the selection, moving the hidden one
/// there first on every frame, so a trail that followed only the drawn cursor
/// drew nothing on `gg` or `ge`. A hidden cursor no program placed is not
/// followed: the trail keeps where it last was, and one drawing fades out
/// there (`trail.rs`).
pub(crate) fn trail_cursor(term: &Term<'_>) -> Option<TrailCursor> {
    if term.window_back().index() != 0 {
        return None;
    }
    match term.cursor_visible() {
        true => {
            let (r, c) = term.cursor();
            Some(TrailCursor::Shown(r, c))
        }
        false => term.placed_cursor().map(|(r, c)| TrailCursor::Placed(r, c)),
    }
}

/// **The lines a read's scroll carries a trail in flight up by**, under
/// `[cursor] trail_scroll` (`key`): every line the whole screen retired
/// while the window is at the live screen (`moved`, the driver's count of
/// the read) -- under a smooth scroll or a jump, on every draw path, on
/// either screen (on the alternate screen, up to half its provisioned ring
/// in one read; past that the count wraps) and with a status line below,
/// so the trail stays on the text it was drawn from, as it does where the
/// grid slides. None where a region retired a line in the read, because
/// the rows under the region stayed; none while the window looks back,
/// where the chase has snapped; none across a switch, where `moved` is no
/// count; and none under `stay`.
pub(crate) fn trail_rides(
    term: &Term<'_>,
    before: crate::keep::Before,
    moved: crate::keep::Ring,
    key: crate::config::TrailScroll,
) -> Option<Rows> {
    match moved {
        crate::keep::Ring::Retired(n)
            if n.index() > 0
                && key == crate::config::TrailScroll::Ride
                && term.window_back().index() == 0
                && crate::keep::whole_screen_scrolled(term, before) =>
        {
            Some(n)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SystemMotion as M;

    /// A ten-row terminal after `setup`, a read of `bytes`, and the lines
    /// `trail_rides` gives it under `key`.
    fn ridden(setup: &[u8], bytes: &[u8], key: crate::config::TrailScroll) -> Option<Rows> {
        ridden_back(setup, Rows::ZERO, bytes, key)
    }

    /// The same, with the window `back` lines up the history for the read.
    fn ridden_back(
        setup: &[u8],
        back: Rows,
        bytes: &[u8],
        key: crate::config::TrailScroll,
    ) -> Option<Rows> {
        let ws = baud_domain::WinSize {
            ws_row: 10,
            ws_col: 40,
        };
        let mut owned = baud_core::Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(baud_domain::BytePos::ZERO, b);
            t.feed(
                baud_domain::BytePos::ZERO,
                baud_domain::ReadLen::from_index(b.len()),
            );
        };
        feed(&mut t, setup);
        t.set_window_back(back);
        assert_eq!(
            t.window_back(),
            back,
            "the control: the window is {back:?} back"
        );
        let before = crate::keep::Before::of(&t);
        feed(&mut t, bytes);
        trail_rides(&t, before, crate::keep::live_moved(&t, before), key)
    }

    /// **A trail rides every line the whole screen retires at the live
    /// window**: on the primary, on the alternate screen and with a status
    /// line below the screen, as it does where the grid slides; and the
    /// lines are the read's count, up to half the alternate screen's ring,
    /// here the ten rows of storage sized to the screen (the drivers'
    /// 150: `the_alternate_screen_rides_round_its_provisioned_rows`).
    /// The cases the ride was built for on 2026-11-01: before it, the
    /// alternate screen and a status line rode nothing on either driver.
    /// Perturbation: the alternate screen refused, as the smooth slide's
    /// gate refuses it, fails its two cases.
    #[test]
    fn a_trail_rides_every_whole_screen_scroll_at_the_live_window() {
        use crate::config::TrailScroll::Ride;
        let bottom: &[u8] = b"\x1b[10;1H";
        let alt: &[u8] = b"\x1b[?1049h\x1b[10;1H";
        // The status line is a row beside the screen's ten, none of them.
        let status: &[u8] = b"\x1b[2$~\x1b[10;1H";
        let wrong: Vec<String> = [
            (bottom, &b"\n"[..], 1, "the primary"),
            (alt, b"\n", 1, "the alternate screen"),
            (status, b"\n", 1, "a status line below"),
            (bottom, b"\n\n\n", 3, "three lines in one read"),
            (
                alt,
                b"\n\n\n\n\n",
                5,
                "half the alternate screen's ring in one read",
            ),
        ]
        .into_iter()
        .filter_map(|(setup, bytes, lines, what)| {
            let got = ridden(setup, bytes, Ride);
            (got != Some(Rows::of_count(lines))).then(|| format!("{what}: {got:?}"))
        })
        .collect();
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    /// **A trail rides no region's scroll, and nothing else**: a region set,
    /// scrolled and reset inside the read; a region anchored at the top on
    /// the alternate screen, vim's; a read that scrolled the whole screen
    /// and a region both; the window looking back; a switch; a scroll down;
    /// and `stay`; and, the one limit, a read that scrolls the alternate
    /// screen past half its ring, whose count wraps -- here the ten rows of
    /// storage sized to the screen, where the drivers provision 150. The
    /// controls: the primary, vim's screen with its region reset and the
    /// live window each ride a line. Perturbation: a region's scroll let
    /// through fails it at the first region.
    #[test]
    fn a_trail_rides_no_region_scroll_and_nothing_under_stay() {
        use crate::config::TrailScroll::{Ride, Stay};
        let bottom = b"\x1b[10;1H";
        assert_eq!(
            ridden(bottom, b"\n", Ride),
            Some(Rows::of_count(1)),
            "the control"
        );
        assert_eq!(
            ridden(bottom, b"\x1b[1;5r\x1b[5;1Hx\r\ny\r\n\x1b[r", Ride),
            None,
            "a region set, scrolled and reset inside the read"
        );
        let vim = b"\x1b[?1049h\x1b[1;8r\x1b[8;1H";
        assert_eq!(
            ridden(vim, b"\n", Ride),
            None,
            "vim's region on the alternate screen"
        );
        assert_eq!(
            ridden(vim, b"\x1b[r\x1b[10;1H\n", Ride),
            Some(Rows::of_count(1)),
            "the control: its whole screen"
        );
        assert_eq!(
            ridden(bottom, b"\n\x1b[1;5r\x1b[5;1H\n\x1b[r", Ride),
            None,
            "the whole screen and a region in one read"
        );
        let back = b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh\r\ni\r\nj\r\nk\r\nl\x1b[1;1H";
        assert_eq!(ridden(back, b"\x1b[1T", Ride), None, "a scroll down");
        assert_eq!(ridden(back, b"\x1bM", Ride), None, "RI on the top row");
        let held = b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\nh\r\ni\r\nj\r\nk\r\nl";
        assert_eq!(
            ridden_back(held, Rows::ZERO, b"\r\nm", Ride),
            Some(Rows::of_count(1)),
            "the control: at the live window"
        );
        assert_eq!(
            ridden_back(held, Rows::of_count(2), b"\r\nm", Ride),
            None,
            "the window looking back"
        );
        assert_eq!(
            ridden(bottom, b"\x1b[?1049h\x1b[10;1H\n", Ride),
            None,
            "a switch"
        );
        assert_eq!(ridden(bottom, b"\n", Stay), None, "stay");
        let alt = b"\x1b[?1049h\x1b[10;1H";
        assert_eq!(
            ridden(alt, b"\n\n\n\n\n\n", Ride),
            None,
            "past half the alternate screen's ring, its count wraps"
        );
    }

    /// The lines `trail_rides` gives each read of `n` line feeds in `ns`,
    /// one after another, on a 24-row terminal over the storage both
    /// drivers provision (`MAX_GRID`), after `setup`; with the live
    /// surface's ring.
    fn ridden_provisioned(setup: &[u8], ns: &[usize]) -> (Rows, Vec<Option<Rows>>) {
        let ws = baud_domain::WinSize {
            ws_row: 24,
            ws_col: 80,
        };
        let mut owned = baud_core::Owned::with_geometry(
            &crate::host::MAX_GRID,
            &baud_core::CacheGeometry::UNKNOWN,
        );
        let mut t = Term::new(owned.storage(), &ws);
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(baud_domain::BytePos::ZERO, b);
            t.feed(
                baud_domain::BytePos::ZERO,
                baud_domain::ReadLen::from_index(b.len()),
            );
        };
        feed(&mut t, setup);
        let rode = ns
            .iter()
            .map(|&n| {
                let before = crate::keep::Before::of(&t);
                feed(&mut t, &vec![b'\n'; n]);
                let moved = crate::keep::live_moved(&t, before);
                trail_rides(&t, before, moved, crate::config::TrailScroll::Ride)
            })
            .collect();
        (t.ring(), rode)
    }

    /// **On the alternate screen a read's count wraps round the rows the
    /// storage is provisioned with, not the window's**: both drivers
    /// provision `MAX_GRID`'s 150 rows, so on a 24-row window a read of n
    /// lines rides n round a ring of 150 -- the n mod 150 lines left over
    /// while they are at most 75, and nothing past that. So 13 lines ride,
    /// past half the window; 76 to 150 ride nothing; 151 ride one, 225
    /// ride 75 and 226 nothing. The control: the primary on the same
    /// storage, round the history's ring, rides every n.
    #[test]
    fn the_alternate_screen_rides_round_its_provisioned_rows() {
        let provisioned = usize::from(crate::host::MAX_GRID.ws_row);
        assert_eq!(provisioned, 150, "the rows docs/cursor.md names");
        let ns = [1, 12, 13, 75, 76, 150, 151, 153, 225, 226];
        let (ring, rode) = ridden_provisioned(b"\x1b[?1049h\x1b[24;1H", &ns);
        let fold = |n: usize| match n % provisioned {
            0 => None,
            left if left * 2 <= provisioned => Some(Rows::of_count(left)),
            _ => None,
        };
        let want: Vec<_> = ns.iter().map(|&n| fold(n)).collect();
        assert_eq!(
            want,
            [1, 12, 13, 75, 0, 0, 1, 3, 75, 0]
                .map(|k| (k > 0).then(|| Rows::of_count(k)))
                .to_vec(),
            "the rule, spelled out"
        );
        assert_eq!(rode, want, "the alternate screen, read by read");
        assert_eq!(ring, Rows::of_index(provisioned), "the alternate ring");
        let (ring, rode) = ridden_provisioned(b"\x1b[24;1H", &ns);
        assert!(ring.index() > 2 * 226, "the control's ring: {ring:?}");
        assert_eq!(
            rode,
            ns.map(|n| Some(Rows::of_count(n))).to_vec(),
            "the control: the primary rides every read"
        );
    }

    /// **The guide's look**: iTerm2's colour while shown, none while
    /// hidden, and its one-point line one device pixel at a backing scale
    /// of 1 and two at 2, from one to four.
    #[test]
    fn the_guide_line_is_a_point_in_device_pixels() {
        use baud_domain::CursorGuide;
        assert_eq!(guide_look(CursorGuide::Hidden, 2.0), None);
        for (scale, line) in [(1.0, 1), (2.0, 2), (3.0, 3), (0.5, 1), (9.0, 4)] {
            assert_eq!(
                guide_look(CursorGuide::Shown, scale),
                Some(GuideLook {
                    colour: baud_render::CURSOR_GUIDE,
                    line: baud_font::Px::of(line),
                }),
                "scale {scale}"
            );
        }
    }

    /// **The badge is the program's format evaluated**: its text and
    /// escapes, an undefined user variable empty, in iTerm2's colour, box
    /// and margins at the backing scale, in bold; the colour `SetColors`
    /// set opaque; nothing while the setting is off, before a program sets
    /// one, or for white space.
    #[test]
    fn the_badge_is_the_programs_format_evaluated() {
        use crate::config::BadgeShown;
        let ws = baud_domain::WinSize {
            ws_col: 40,
            ws_row: 10,
        };
        let mut owned = baud_core::Owned::new(&ws);
        let mut t = baud_core::Term::new(owned.storage(), &ws);
        let feed = |t: &mut baud_core::Term<'_>, b: &[u8]| {
            t.write_at(baud_domain::BytePos::ZERO, b);
            t.feed(
                baud_domain::BytePos::ZERO,
                baud_domain::ReadLen::from_index(b.len()),
            );
        };
        assert_eq!(
            badge_look(&t, BadgeShown::On, &Session::default(), &|| None, PAGE, 2.0),
            None,
            "none set"
        );
        // `on \(user.host)\tx`, base64.
        feed(
            &mut t,
            b"\x1b]1337;SetBadgeFormat=b24gXCh1c2VyLmhvc3QpXHR4\x07",
        );
        let look = badge_look(&t, BadgeShown::On, &Session::default(), &|| None, PAGE, 2.0)
            .expect("a badge");
        assert_eq!(look.text, "on \tx");
        assert_eq!(look.colour, baud_render::BADGE);
        assert_eq!(
            look.outline,
            PAGE.with_alpha(baud_render::DRAWN_AT),
            "the background, inside the badge's half"
        );
        assert_eq!(
            (look.top, look.right),
            (baud_font::Px::of(60), baud_font::Px::of(20))
        );
        assert_eq!(look.fraction, baud_render::BOX);
        assert_eq!(look.style, baud_render::FaceStyle::Bold);
        assert_eq!(
            badge_look(
                &t,
                BadgeShown::Off,
                &Session::default(),
                &|| None,
                PAGE,
                2.0
            ),
            None,
            "the setting off"
        );
        feed(&mut t, b"\x1b]1337;SetColors=badge=00ff00\x07");
        assert_eq!(
            badge_look(&t, BadgeShown::On, &Session::default(), &|| None, PAGE, 1.0)
                .map(|l| l.outline),
            Some(PAGE),
            "an opaque badge, an opaque outline"
        );
        assert_eq!(
            badge_look(&t, BadgeShown::On, &Session::default(), &|| None, PAGE, 1.0)
                .map(|l| l.colour),
            Some(baud_render::Argb::rgb(
                Chan::from_u8(0),
                Chan::from_u8(255),
                Chan::from_u8(0)
            ))
        );
        // `   `, base64.
        feed(&mut t, b"\x1b]1337;SetBadgeFormat=ICAg\x07");
        assert_eq!(
            badge_look(&t, BadgeShown::On, &Session::default(), &|| None, PAGE, 1.0),
            None,
            "white space alone"
        );
        let src = [
            (crate::app_source::app_source(), "badge_look"),
            (include_str!("linux.rs"), "badge_of"),
        ];
        for (s, read) in src {
            // Read with its white space taken out, so a reflow keeps it.
            let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
            assert!(
                s.contains(&format!(
                    "letbadge=crate::effects::{read}(&self.term,self.badge,&self.session,&selection{}",
                    match read {
                        "badge_of" => ")",
                        _ => ",",
                    }
                )),
                "the term, the setting, the session and the selection"
            );
            assert!(s.contains(concat!("self.renderer.", "set_badge(badge);")));
            assert!(
                s.contains(concat!("self.renderer.", "background_of(&self.term)"))
                    || s.contains(concat!("self.renderer.", "background_of(&*self.term)")),
                "the outline in the background colour"
            );
        }
    }

    /// **A user variable a program set is drawn in the badge**: `SetUserVar`
    /// keeps `user.host`, and the badge naming it shows its value.
    #[test]
    fn the_badge_draws_a_user_variable_a_program_set() {
        use crate::config::BadgeShown;
        let ws = baud_domain::WinSize {
            ws_col: 40,
            ws_row: 10,
        };
        let mut owned = baud_core::Owned::new(&ws);
        let mut t = baud_core::Term::new(owned.storage(), &ws);
        let b =
            b"\x1b]1337;SetUserVar=host=Ym94\x07\x1b]1337;SetBadgeFormat=b24gXCh1c2VyLmhvc3Qp\x07";
        t.write_at(baud_domain::BytePos::ZERO, b);
        t.feed(
            baud_domain::BytePos::ZERO,
            baud_domain::ReadLen::from_index(b.len()),
        );
        let look = badge_look(&t, BadgeShown::On, &Session::default(), &|| None, PAGE, 1.0)
            .expect("a badge");
        assert_eq!(look.text, "on box");
    }

    /// A background colour for the badge's outline.
    const PAGE: baud_render::Argb = baud_render::Argb(0xFF10_2030);

    /// A session with every fact the app reads.
    fn full_session() -> Session {
        Session {
            pid: Some(4100),
            tty: Some("/dev/ttys004".into()),
            job: Some(4200),
            job_name: Some("vim".into()),
            dir: Some("/Users/me/src".into()),
            title: Some("src \u{2014} vim".into()),
            user: Some("me".into()),
            host: Some("laptop".into()),
            home: Some("/Users/me".into()),
            shell: Some("fish".into()),
            uname: Some("Darwin".into()),
            created: Some("20261101_093005".into()),
        }
    }

    /// A term fed `bytes`.
    fn fed(owned: &mut baud_core::Owned, bytes: &[u8]) -> baud_core::Term<'static> {
        let ws = baud_domain::WinSize {
            ws_col: 40,
            ws_row: 10,
        };
        let owned: &'static mut baud_core::Owned = Box::leak(Box::new(std::mem::replace(
            owned,
            baud_core::Owned::new(&ws),
        )));
        let mut t = baud_core::Term::new(owned.storage(), &ws);
        t.write_at(baud_domain::BytePos::ZERO, bytes);
        t.feed(
            baud_domain::BytePos::ZERO,
            baud_domain::ReadLen::from_index(bytes.len()),
        );
        t
    }

    fn new_owned() -> baud_core::Owned {
        baud_core::Owned::new(baud_domain::WinSize {
            ws_col: 40,
            ws_row: 10,
        })
    }

    /// The badge `format` evaluated over `t` and `s`.
    fn text_of(t: &baud_core::Term<'_>, s: &Session, format: &str) -> String {
        badge_text(format, &|p| {
            badge_variable(t, s, &|| Some("sel é".into()), p)
        })
    }

    /// **Every variable the reference lists is answered as the table
    /// says**: one baud answers has a value with every fact known, one
    /// truthfully unset or unknown has none -- the table the core counts by
    /// and these answers cannot drift apart. The badge's own is a cycle,
    /// drawn before any is read.
    #[test]
    fn every_listed_variable_is_answered_as_the_table_says() {
        use baud_domain::{Answer, GlobalVariable, SessionVariable};
        let mut owned = new_owned();
        let t = fed(&mut owned, b"\x1b]1337;SetUserVar=x=eQ==\x07");
        let s = full_session();
        for &v in SessionVariable::ALL {
            let got = badge_variable(&t, &s, &|| Some("sel".into()), v.name());
            let named = badge_variable(
                &t,
                &s,
                &|| Some("sel".into()),
                &format!("session.{}", v.name()),
            );
            assert_eq!(got, named, "{v:?}: the session alias");
            match (v.answer(), v) {
                (Answer::Answered, SessionVariable::Badge) => assert_eq!(got, None),
                (
                    Answer::Answered,
                    SessionVariable::TerminalIconName | SessionVariable::TerminalWindowName,
                ) => {
                    assert_eq!(got, None, "{v:?}: none set");
                }
                (Answer::Answered, _) => assert!(got.is_some(), "{v:?}"),
                (Answer::Unset | Answer::Unknown, _) => assert_eq!(got, None, "{v:?}"),
            }
        }
        for (g, name) in [
            (GlobalVariable::Pid, "pid"),
            (GlobalVariable::LocalhostName, "localhostName"),
            (GlobalVariable::AppBundlePath, "appBundlePath"),
            (GlobalVariable::EffectiveTheme, "effectiveTheme"),
        ] {
            let got = badge_variable(&t, &s, &|| None, &format!("iterm2.{name}"));
            assert_eq!(got.is_some(), g.answer() == Answer::Answered, "{g:?}");
        }
        assert_eq!(
            badge_variable(&t, &s, &|| None, "user.x"),
            Some(baud_domain::Value::Text("y".into()))
        );
    }

    /// **The session's variables are its facts**: the size, the titles, the
    /// job, the root process and device, the modes, the bells, a
    /// selection's text and length in UTF-8 bytes, the window's title as
    /// `name`, the home folder, the shell, the system's name and the time
    /// the session started; and what it counts as numbers.
    #[test]
    fn the_session_variables_are_the_sessions_facts() {
        let mut owned = new_owned();
        let t = fed(
            &mut owned,
            b"\x1b]2;edit\x07\x1b]1;ic\x07\x1b[?1002h\x1b=\x07\x07",
        );
        let s = full_session();
        assert_eq!(
            text_of(&t, &s, "\\(columns)x\\(rows) \\(columns * rows)"),
            "40x10 400"
        );
        assert_eq!(
            text_of(
                &t,
                &s,
                "\\(terminalWindowName)|\\(terminalIconName)|\\(name)"
            ),
            "edit|ic|src \u{2014} vim"
        );
        assert_eq!(
            text_of(&t, &s, "\\(jobName) \\(jobPid) \\(pid) \\(tty)"),
            "vim 4200 4100 /dev/ttys004"
        );
        assert_eq!(
            text_of(
                &t,
                &s,
                "\\(mouseReportingMode) \\(applicationKeypad) \\(bellCount) \\(sshIntegrationLevel)"
            ),
            "2 1 2 0"
        );
        assert_eq!(
            text_of(&t, &s, "\\(selection)|\\(selectionLength)"),
            "sel \u{e9}|6"
        );
        assert_eq!(
            text_of(&t, &s, "\\(iterm2.pid > 0) \\(iterm2.localhostName)"),
            "1 laptop"
        );
        assert_eq!(
            text_of(
                &t,
                &s,
                "\\(homeDirectory) \\(shell) \\(uname) \\(creationTimeString)"
            ),
            "/Users/me fish Darwin 20261101_093005"
        );
        let bare = fed(&mut owned, b"");
        assert_eq!(
            text_of(
                &bare,
                &s,
                "<\\(terminalWindowName)> \\(mouseReportingMode) \\(applicationKeypad)"
            ),
            "<> -1 0"
        );
    }

    /// **A session starts with this machine's facts**: the home folder
    /// `$HOME` names, the name of the shell `$SHELL` names, else the
    /// system's, the system's own name, and when it started, as iTerm2
    /// writes the time -- eight digits, `_` and six. A configured program
    /// names the shell in its place.
    #[test]
    fn a_session_starts_with_this_machines_facts() {
        assert_eq!(
            Session::here(Some("/opt/shells/bin/fish")).shell.as_deref(),
            Some("fish")
        );
        let s = Session::here(None);
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        assert_eq!(s.home, env("HOME"));
        let shell = std::env::var("SHELL").unwrap_or_else(|_| match cfg!(target_os = "macos") {
            true => "/bin/zsh".into(),
            false => "/bin/sh".into(),
        });
        assert_eq!(s.shell.as_deref(), shell.rsplit('/').next());
        let system = match cfg!(target_os = "macos") {
            true => "Darwin",
            false => "Linux",
        };
        assert_eq!(s.uname.as_deref(), Some(system));
        let at = s.created.expect("a start");
        let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
        assert!(
            at.len() == 15 && digits(&at[..8]) && &at[8..9] == "_" && digits(&at[9..]),
            "{at}"
        );
    }

    /// **The start is written as iTerm2's formatter writes it**,
    /// `yyyyMMdd_HHmmss`, each field padded.
    #[test]
    fn the_creation_time_is_written_as_iterm2_writes_it() {
        let at = crate::sys::Tm {
            tm_year: 126,
            tm_mon: 10,
            tm_mday: 1,
            tm_hour: 9,
            tm_min: 3,
            tm_sec: 5,
            ..crate::sys::Tm::ZERO
        };
        assert_eq!(creation_time(&at), "20261101_090305");
    }

    /// **The host, user and directory are the last a program named, else
    /// the session's own**: this machine's user and name, and the
    /// foreground's directory, until `RemoteHost` names a user and host,
    /// split at its last `@`, and `CurrentDir` or `OSC 7` a directory. The
    /// host and user carry from one message to the next as iTerm2's
    /// `setHost` carries them: `user@` keeps the last host, or none, and a
    /// host alone the last user, or none; an `OSC 7` naming a host keeps
    /// the user only for the same host, its case aside, and gives none
    /// for another; one naming no host changes neither. `RIS` forgets
    /// them. "None" is an empty value, which `==` can test, not an
    /// undefined one.
    #[test]
    fn the_host_user_and_path_are_the_last_named() {
        let s = full_session();
        let mut owned = new_owned();
        let q = "\\(username)@\\(hostname):\\(path)";
        // Each empty, with an operator, which an undefined one fails.
        let empty = "\\(username == \"\") \\(hostname == \"\")";
        let t = fed(&mut owned, b"");
        assert_eq!(text_of(&t, &s, q), "me@laptop:/Users/me/src");
        let t = fed(&mut owned, b"\x1b]1337;RemoteHost=ops@db-01\x07");
        assert_eq!(text_of(&t, &s, q), "ops@db-01:/Users/me/src");
        let t = fed(
            &mut owned,
            b"\x1b]1337;RemoteHost=a@b@db-02\x07\x1b]1337;CurrentDir=/srv\x07",
        );
        assert_eq!(text_of(&t, &s, q), "a@b@db-02:/srv");
        let t = fed(&mut owned, b"\x1b]7;file://web-03/var/www\x07");
        assert_eq!(text_of(&t, &s, q), "@web-03:/var/www");
        assert_eq!(text_of(&t, &s, empty), "1 0", "no user, and defined");
        let t = fed(
            &mut owned,
            b"\x1b]1337;RemoteHost=ops@db-01\x07\x1b]7;file://web-03/var/www\x07",
        );
        assert_eq!(
            text_of(&t, &s, q),
            "@web-03:/var/www",
            "RemoteHost's user is not web-03's"
        );
        let t = fed(
            &mut owned,
            b"\x1b]1337;RemoteHost=ops@DB-01\x07\x1b]7;file://db-01/var/www\x07",
        );
        assert_eq!(text_of(&t, &s, q), "ops@db-01:/var/www", "the same host");
        let t = fed(
            &mut owned,
            b"\x1b]1337;RemoteHost=ops@db-01\x07\x1b]1337;RemoteHost=me@\x07",
        );
        assert_eq!(text_of(&t, &s, q), "me@db-01:/Users/me/src", "user@");
        let t = fed(&mut owned, b"\x1b]1337;RemoteHost=me@\x07");
        assert_eq!(text_of(&t, &s, q), "me@:/Users/me/src", "user@ first");
        assert_eq!(text_of(&t, &s, empty), "0 1");
        let t = fed(
            &mut owned,
            b"\x1b]1337;RemoteHost=ops@db-01\x07\x1b]1337;RemoteHost=box\x07",
        );
        assert_eq!(text_of(&t, &s, q), "ops@box:/Users/me/src", "a host alone");
        let t = fed(&mut owned, b"\x1b]1337;RemoteHost=box\x07");
        assert_eq!(text_of(&t, &s, empty), "1 0", "a host alone first");
        let t = fed(
            &mut owned,
            b"\x1b]1337;RemoteHost=ops@db-01\x07\x1b]7;file:///tmp\x07",
        );
        assert_eq!(text_of(&t, &s, q), "ops@db-01:/tmp", "no host named");
        let t = fed(&mut owned, b"\x1b]7;file:///tmp\x07");
        assert_eq!(text_of(&t, &s, q), "me@laptop:/tmp");
        let t = fed(
            &mut owned,
            b"\x1b]1337;RemoteHost=ops@db-01\x07\x1b]1337;CurrentDir=/srv\x07\x1bc",
        );
        assert_eq!(text_of(&t, &s, q), "me@laptop:/Users/me/src", "RIS");
    }

    /// **A badge naming itself is a cycle, and one that fails is the bug and
    /// the reason**, as iTerm2 draws both; an undefined variable empties
    /// only its own expression.
    #[test]
    fn a_cycle_and_a_failure_draw_as_iterm2_draws_them() {
        let mut owned = new_owned();
        let t = fed(&mut owned, b"");
        let s = full_session();
        assert_eq!(text_of(&t, &s, "x \\(badge) y"), CYCLE);
        assert_eq!(text_of(&t, &s, "\\(\"a\\(session.badge)\")"), CYCLE);
        assert_eq!(
            text_of(&t, &s, "\\(hostname) \\(\"a\" + 1)"),
            "\u{1F41E} Arithmetic needs numbers"
        );
        assert_eq!(text_of(&t, &s, "\\(1 +)"), "\u{1F41E} Syntax error");
        assert_eq!(text_of(&t, &s, "\\(hostname)/\\(bogus)/"), "laptop//");
    }

    /// **Both apps hand the badge the session they bring up to date with
    /// the title**: its root process, device, job, directory and the
    /// title shown, read where the title is sent, and a frame owed when
    /// they change under a badge, so a job started silently is shown.
    #[test]
    fn both_apps_bring_the_session_up_to_date() {
        // The macOS driver owes a frame through its door, which runs the
        // display link (`App::owe_frame`).
        for (name, src, at, owes) in [
            (
                "app.rs",
                crate::app_source::app_source(),
                "self.app.session",
                "owe_frame()",
            ),
            (
                "linux.rs",
                include_str!("linux.rs"),
                "self.session",
                "pending = Pending::Dirty",
            ),
        ] {
            let send = crate::app_source::split_once_in(src, "fn send_title(&mut self) {")
                .expect(name)
                .1;
            let body = &send[..send.find("\n    }\n").expect(name)];
            for read in [
                "pty.child()",
                "pty.tty()",
                "self.derive.group()",
                "self.derive.name()",
                "self.derive.dir()",
                "self.titles.sent()",
            ] {
                assert!(body.contains(read), "{name}: {read}");
            }
            assert!(body.contains(at), "{name}: {at}");
            let owed = body.split_once("badge_format().is_some()").expect(name).1;
            assert!(owed.contains(owes), "{name}: a frame owed");
        }
    }

    /// **Both apps draw a program's guide over the setting**: each frame
    /// hands the renderer what `Term::cursor_guide` says, else `[cursor]
    /// guide`, through `guide_look`.
    #[test]
    fn both_apps_draw_a_programs_guide_over_the_setting() {
        let read = concat!("self.term.cursor_guide().", "unwrap_or(self.cursor_guide)");
        for (name, src) in [
            ("app.rs", crate::app_source::app_source()),
            ("linux.rs", include_str!("linux.rs")),
        ] {
            assert_eq!(src.matches(read).count(), 1, "{name}: {read}");
            assert!(
                src.contains(concat!(
                    "set_cursor_guide(crate::effects::",
                    "guide_look(guide"
                )),
                "{name}"
            );
        }
    }

    /// **The wake for a flip reads the flipped phase**: from any moment,
    /// the deadline is ahead of it, the phase of each rate on screen has
    /// changed by then, and no flip of that rate lies between the two.
    /// Perturbation: wake a flip late, at `(k + 1) * half` -- the phase
    /// at the wake is the old one and this fails. Dropping `PAST_FLIP`
    /// fails `only_a_rate_on_screen_bounds_the_wait` and, through the
    /// clock's round trip to an instant, the wake loop of
    /// `blinking_text_wakes_at_its_own_flips_and_repaints_whole`.
    #[test]
    fn the_next_flip_is_ahead_and_reads_the_new_phase() {
        for (seen, rate) in [
            (
                Blinking::SLOW,
                (|p: &BlinkPhases| p.slow) as fn(&BlinkPhases) -> Blink,
            ),
            (Blinking::RAPID, |p: &BlinkPhases| p.rapid),
        ] {
            for i in 0..20_000u32 {
                let t = f64::from(i) * 0.000_731 + 1000.0;
                let next = next_flip(t, seen, M::Full).expect("a rate on screen");
                assert!(next > t, "{t}: {next}");
                let before = rate(&text_blink(t, M::Full));
                assert_ne!(rate(&text_blink(next, M::Full)), before, "{t} -> {next}");
                // Nothing flips back in between: half a millisecond before
                // the wake, the old phase or the new one, never a third.
                let just = rate(&text_blink(next - 0.0015, M::Full));
                assert_eq!(just, before, "{t} -> {next}: flipped early");
            }
        }
    }

    /// **Nothing on screen blinks, or reduced motion: no wake.** With both
    /// rates on screen the rapid one, which flips at every slow flip and
    /// between them, is the bound. Perturbation: return a flip for an
    /// unseen screen -- the first assertion fails.
    #[test]
    fn only_a_rate_on_screen_bounds_the_wait() {
        assert_eq!(next_flip(3.0, Blinking::UNSEEN, M::Full), None);
        assert_eq!(next_flip(3.0, Blinking::BOTH, M::Reduced), None);
        let both = next_flip(0.05, Blinking::BOTH, M::Full).expect("both");
        let rapid = next_flip(0.05, Blinking::RAPID, M::Full).expect("rapid");
        let slow = next_flip(0.05, Blinking::SLOW, M::Full).expect("slow");
        assert_eq!(both, rapid);
        assert!(slow > rapid, "{slow} {rapid}");
        assert!((rapid - (RAPID_HALF + PAST_FLIP)).abs() < 1e-9, "{rapid}");
        assert!((slow - (BLINK_HALF + PAST_FLIP)).abs() < 1e-9, "{slow}");
    }
}
