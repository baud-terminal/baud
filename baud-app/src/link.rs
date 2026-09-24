//! `OSC 8` activation: which link a Cmd-click opens, and which it refuses.
//!
//! The core stores a link per cell and answers `Term::link_uri`; whether a
//! click on it opens anything is this app's policy rather than the
//! protocol's, so it lives here and not in the core or the domain -- and it
//! is pure over bytes, so its tests run on the Linux lint too. Two halves:
//!
//! * [`admit`] decides whether a URI may be handed to the OS at all, and
//!   what for. An allowlist, because every registered scheme handler on
//!   macOS is an application `NSWorkspace` *launches*, and a program's
//!   output is attacker-controlled: `http`, `https` and `mailto` are
//!   opened; `file` on this host is *revealed* in Finder, which launches
//!   nothing (`file:///Applications/Foo.app` handed to the opener would
//!   launch Foo); everything else is refused, and the refusal is named so
//!   the log line can say why.
//! * [`gesture`] decides what a mouse event over a link means: Cmd-click,
//!   armed on the press and opened on the release over the same link, the
//!   whole gesture the terminal's -- a child tracking the mouse never sees
//!   it, the way Shift already carves a selection out.
//!
//! What this does not do, stated: a bare `https://...` printed as text is
//! not a link here -- only an `OSC 8` run is -- and nothing marks a linked
//! cell until the pointer-cursor and hover underline land, both of which
//! need a modifier-change callback the window crate does not carry yet.
#![forbid(unsafe_code)]

use baud_core::{Focus, LINK_MAX};
use baud_domain::LinkId;

/// What an admitted target is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Hand it to the default handler for its scheme: the browser, the
    /// mail composer -- or, for `https`, whatever LaunchServices routes it
    /// to, universal-link applications included. That is an accepted risk
    /// rather than a closed one.
    Open,
    /// Select it in Finder. Never launches.
    Reveal,
}

/// The four schemes the policy knows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scheme {
    Http,
    Https,
    Mailto,
    File,
}

/// Why a URI was not handed to the OS: the first rule it failed, in the
/// order the rules are checked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refused {
    /// Empty, or past the core's own cap. The core refuses to store a URI
    /// past `LINK_MAX` now, so the second cannot arrive; the rule stands so
    /// the two caps cannot drift apart unnoticed.
    Length,
    /// A byte outside printable ASCII: a space, a control, DEL, a high
    /// byte. RFC 3986 URLs are ASCII, and the parser lets DEL and high
    /// bytes through.
    Byte,
    /// Not `http`, `https`, `mailto` or `file`. A leading `-` fails here
    /// too, which is what closes the argv hazard an opener would have had.
    Scheme,
    /// `http`/`https` with no authority, `mailto` with no address, `file`
    /// on another host or without an absolute path.
    Host,
    /// A `mailto` header other than `subject` or `body`: `bcc=` prefills a
    /// hidden recipient, and any other header reaches whatever application
    /// is registered for `mailto`, which is not necessarily a composer.
    Header,
}

/// A URI the policy admitted, with what it is for. Only [`admit`] builds
/// one, so the opener's one call site cannot be handed a raw string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target<'a> {
    uri: &'a str,
    scheme: Scheme,
}

impl<'a> Target<'a> {
    pub fn as_str(&self) -> &'a str {
        self.uri
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    pub fn action(&self) -> Action {
        match self.scheme {
            Scheme::File => Action::Reveal,
            Scheme::Http | Scheme::Https | Scheme::Mailto => Action::Open,
        }
    }
}

/// This machine's name, for a `file://host/path` link -- the form
/// `ls --hyperlink` emits -- to be recognised as local. Empty when unknown,
/// which admits only the empty and `localhost` hosts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Host<'a>(pub &'a str);

/// The longest URI the policy admits: the core's own cap, so a URI the
/// core stored whole is never refused for length here.
pub const MAX: usize = LINK_MAX;

/// Decide whether `uri` may be handed to the OS, and what for.
pub fn admit<'a>(uri: &'a [u8], host: Host<'_>) -> Result<Target<'a>, Refused> {
    if uri.is_empty() || uri.len() > MAX {
        return Err(Refused::Length);
    }
    if !uri.iter().all(|b| (0x21..=0x7e).contains(b)) {
        return Err(Refused::Byte);
    }
    // Every byte is printable ASCII, so this cannot fail; the arm is what
    // the type system asks for rather than a second rule.
    let s = core::str::from_utf8(uri).map_err(|_| Refused::Byte)?;
    let colon = s.find(':').ok_or(Refused::Scheme)?;
    let (name, rest) = (&s[..colon], &s[colon + 1..]);
    let scheme = if name.eq_ignore_ascii_case("http") {
        Scheme::Http
    } else if name.eq_ignore_ascii_case("https") {
        Scheme::Https
    } else if name.eq_ignore_ascii_case("mailto") {
        Scheme::Mailto
    } else if name.eq_ignore_ascii_case("file") {
        Scheme::File
    } else {
        return Err(Refused::Scheme);
    };
    match scheme {
        Scheme::Http | Scheme::Https => {
            let auth = rest.strip_prefix("//").ok_or(Refused::Host)?;
            let end = auth.find(['/', '?', '#']).unwrap_or(auth.len());
            if end == 0 {
                return Err(Refused::Host);
            }
        }
        Scheme::Mailto => {
            let (addr, query) = match rest.find('?') {
                Some(q) => (&rest[..q], Some(&rest[q + 1..])),
                None => (rest, None),
            };
            if addr.is_empty() {
                return Err(Refused::Host);
            }
            if let Some(q) = query {
                for pair in q.split('&') {
                    let key = pair.split('=').next().unwrap_or("");
                    if !(key.eq_ignore_ascii_case("subject") || key.eq_ignore_ascii_case("body")) {
                        return Err(Refused::Header);
                    }
                }
            }
        }
        Scheme::File => {
            let auth = rest.strip_prefix("//").ok_or(Refused::Host)?;
            let end = auth.find('/').ok_or(Refused::Host)?;
            let h = &auth[..end];
            let local = h.is_empty()
                || h.eq_ignore_ascii_case("localhost")
                || h.eq_ignore_ascii_case(host.0);
            if !local {
                return Err(Refused::Host);
            }
        }
    }
    Ok(Target { uri: s, scheme })
}

/// A mouse event's kind, as the gesture reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    Down,
    Dragged,
    Up,
    Exited,
    Other,
}

/// The modifier chord of a press: Cmd alone is the link gesture, and
/// Shift wins over it -- one meaning per modifier, and Shift's is the
/// selection carve-out, which is older and documented.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chord {
    Cmd,
    Other,
}

/// The click count of a press: a repeat is a double or triple click, which
/// arms so its release is swallowed too and never opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clicks {
    Single,
    Repeat,
}

/// A press the terminal took for itself: the link under it, and whether
/// the release over the same link opens it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Armed {
    pub link: LinkId,
    pub clicks: Clicks,
}

/// What a mouse event means while a link gesture may be in progress.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gesture {
    /// Not the terminal's: the child's tracking or the selection takes it.
    Pass,
    /// A Cmd press on a link: the gesture is the terminal's from here.
    Arm(Armed),
    /// Swallow this event and stay armed.
    Hold,
    /// Swallow this event and disarm; nothing opens.
    Drop,
    /// Swallow this event, disarm, and open the link.
    Open(LinkId),
}

/// The gesture, as a pure function of the event, the app's focus, what is
/// armed and the link under the pointer. Arm only on a Cmd press over a
/// link while the window is key -- a click into a background window is the
/// one that would open something the user did not intend, and AppKit does
/// not deliver it anyway until the view accepts first mouse, which is the
/// crate change recorded beside hover. Open on the release over the *same
/// link*, whatever modifiers the release carries: a press that drifts a
/// point across a cell boundary inside the link, or from a wide lead onto
/// its tail, still opens; a release over another link, or off one, drops.
pub fn gesture(
    press: Press,
    chord: Chord,
    clicks: Clicks,
    focus: Focus,
    armed: Option<Armed>,
    under: Option<LinkId>,
) -> Gesture {
    match (armed, press) {
        (None, Press::Down) => match (chord, focus, under) {
            (Chord::Cmd, Focus::Gained, Some(link)) => Gesture::Arm(Armed { link, clicks }),
            _ => Gesture::Pass,
        },
        (None, _) => Gesture::Pass,
        (Some(_), Press::Dragged | Press::Other) => Gesture::Hold,
        (Some(a), Press::Up) => match (a.clicks, under) {
            (Clicks::Single, Some(l)) if l == a.link => Gesture::Open(l),
            _ => Gesture::Drop,
        },
        (Some(_), Press::Exited | Press::Down) => Gesture::Drop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Host<'static> = Host("");

    fn ok(uri: &str) -> Target<'_> {
        admit(uri.as_bytes(), Host("thishost.local")).unwrap_or_else(|r| panic!("{uri}: {r:?}"))
    }

    fn no(uri: &str) -> Refused {
        admit(uri.as_bytes(), Host("thishost.local")).expect_err(uri)
    }

    /// Perturbation: add `ftp` to the allowlist -- the ftp case fails here,
    /// and `the_refusal_names_the_first_rule_that_fails` goes with it because
    /// `ftp` is the refused scheme it uses; nothing else moves.
    #[test]
    fn the_four_schemes_are_admitted_and_nothing_else_is() {
        assert_eq!(ok("http://x").scheme(), Scheme::Http);
        assert_eq!(ok("HTTPS://x").scheme(), Scheme::Https);
        assert_eq!(ok("mailto:a@b").scheme(), Scheme::Mailto);
        assert_eq!(ok("file:///tmp/x").scheme(), Scheme::File);
        for uri in [
            "ftp://x",
            "ssh://x",
            "javascript:alert(1)",
            "x-apple.systempreferences:com.apple.preference",
            "-https://x",
            "https",
            "data:text/html,hi",
        ] {
            assert_eq!(no(uri), Refused::Scheme, "{uri}");
        }
    }

    /// **The opener is handed exactly the URI the policy admitted**: the
    /// target carries the bytes it was given, not a rebuilt or trimmed form.
    /// Perturbation: make `as_str` answer anything else -- fails alone.
    #[test]
    fn the_admitted_target_is_the_uri_it_was_given() {
        for uri in [
            "https://h/p?q#f",
            "mailto:a@b?subject=s",
            "file:///Applications/Foo.app",
        ] {
            assert_eq!(ok(uri).as_str(), uri);
        }
    }

    /// Only `file` reveals; the three web-ish schemes open. Perturbation:
    /// make `action` answer `Open` for `File` -- fails alone.
    #[test]
    fn a_file_link_is_revealed_and_the_others_are_opened() {
        assert_eq!(ok("file:///Applications/Foo.app").action(), Action::Reveal);
        assert_eq!(ok("https://x").action(), Action::Open);
        assert_eq!(ok("mailto:a@b").action(), Action::Open);
    }

    /// `ls --hyperlink` names the host; only this host, `localhost` and no
    /// host are local, and the path must be absolute. Perturbation: drop the
    /// host test -- the `other` case fails alone.
    #[test]
    fn a_file_link_is_local_or_refused() {
        assert!(admit(b"file:///p", NONE).is_ok());
        assert!(admit(b"file://localhost/p", NONE).is_ok());
        assert!(admit(b"file://THISHOST.local/p", Host("thishost.local")).is_ok());
        assert_eq!(
            admit(b"file://other/p", Host("thishost.local")),
            Err(Refused::Host)
        );
        assert_eq!(admit(b"file://thishost.local/p", NONE), Err(Refused::Host));
        assert_eq!(no("file://p"), Refused::Host);
        assert_eq!(no("file:/p"), Refused::Host);
    }

    /// Perturbation: delete the authority check -- fails alone.
    #[test]
    fn a_hostless_web_url_is_refused() {
        for uri in ["http:///etc/passwd", "https://", "http://?q", "http:h"] {
            assert_eq!(no(uri), Refused::Host, "{uri}");
        }
        ok("http://h");
        ok("https://h/p?q#f");
    }

    /// Perturbation: admit every header -- the `bcc` cases fail alone.
    #[test]
    fn a_mailto_header_other_than_subject_or_body_is_refused() {
        ok("mailto:a@b");
        ok("mailto:a@b?subject=x&body=y");
        assert_eq!(no("mailto:a@b?bcc=c@d"), Refused::Header);
        assert_eq!(no("mailto:a@b?subject=x&Bcc=c"), Refused::Header);
        assert_eq!(no("mailto:a@b?attach=/etc/passwd"), Refused::Header);
        assert_eq!(no("mailto:"), Refused::Host);
    }

    /// Perturbation: widen the range to `0x20..=0x7f` -- the space and DEL
    /// cases fail, the tab, CR and high-byte cases stay green.
    #[test]
    fn a_byte_outside_printable_ascii_refuses_the_whole_uri() {
        for uri in [
            &b"https://h/a b"[..],
            b"https://h/a\tb",
            b"https://h/a\rb",
            b"https://h/a\x7fb",
            b"https://h/\xc3\xa9",
        ] {
            assert_eq!(admit(uri, NONE), Err(Refused::Byte), "{uri:?}");
        }
    }

    /// The cap is the core's, so a URI the core stored whole is never
    /// refused here for length; the control pins the two together.
    /// Perturbation: `MAX - 1` -- the 256-byte case fails.
    #[test]
    fn a_uri_at_the_cap_is_admitted_and_past_it_is_refused() {
        assert_eq!(MAX, LINK_MAX);
        let base = "https://h/";
        let at = format!("{base}{}", "a".repeat(MAX - base.len()));
        assert_eq!(at.len(), MAX);
        ok(&at);
        assert_eq!(no(&format!("{at}a")), Refused::Length);
        assert_eq!(admit(b"", NONE), Err(Refused::Length));
    }

    /// Perturbation: reorder the checks -- the reordered pair fails.
    #[test]
    fn the_refusal_names_the_first_rule_that_fails() {
        let long = format!("ftp://h/a b{}", "c".repeat(300));
        assert_eq!(no(&long), Refused::Length);
        assert_eq!(no("ftp://h/a b"), Refused::Byte);
        assert_eq!(no("ftp://h/ab"), Refused::Scheme);
    }

    #[allow(non_snake_case)]
    fn L() -> LinkId {
        LinkId::from_index(3)
    }

    #[allow(non_snake_case)]
    fn M() -> LinkId {
        LinkId::from_index(4)
    }

    fn armed(clicks: Clicks) -> Option<Armed> {
        Some(Armed { link: L(), clicks })
    }

    /// **An armed click holds through a drag and through an event that is
    /// neither a press nor a release**, so a pointer moving inside the link
    /// or a stray event between the down and the up does not lose the click.
    /// Perturbation: drop on `Press::Other` -- fails alone.
    #[test]
    fn an_armed_click_holds_through_a_drag_and_any_other_event() {
        for press in [Press::Dragged, Press::Other] {
            let g = gesture(
                press,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                armed(Clicks::Single),
                Some(L()),
            );
            assert_eq!(g, Gesture::Hold, "{press:?}");
        }
    }

    /// Perturbation: require `Chord::Cmd` on the Up -- the second case
    /// fails alone, and users who release Cmd before the button would have
    /// lost the click.
    #[test]
    fn a_cmd_click_on_a_link_arms_on_the_down_and_opens_on_the_up() {
        let down = gesture(
            Press::Down,
            Chord::Cmd,
            Clicks::Single,
            Focus::Gained,
            None,
            Some(L()),
        );
        assert_eq!(
            down,
            Gesture::Arm(Armed {
                link: L(),
                clicks: Clicks::Single
            })
        );
        let up = gesture(
            Press::Up,
            Chord::Other,
            Clicks::Single,
            Focus::Gained,
            armed(Clicks::Single),
            Some(L()),
        );
        assert_eq!(up, Gesture::Open(L()));
    }

    /// The release opens over the *same link*, not the same cell: a drift
    /// across a cell boundary inside the link, or lead to tail, still
    /// opens; a release over another link or off one drops. Perturbation:
    /// open on any release while armed -- the `M` and `None` cases fail.
    #[test]
    fn a_press_that_leaves_the_link_opens_nothing_and_still_swallows_the_up() {
        let a = armed(Clicks::Single);
        assert_eq!(
            gesture(
                Press::Dragged,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                a,
                Some(L())
            ),
            Gesture::Hold
        );
        assert_eq!(
            gesture(
                Press::Up,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                a,
                Some(M())
            ),
            Gesture::Drop
        );
        assert_eq!(
            gesture(
                Press::Up,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                a,
                None
            ),
            Gesture::Drop
        );
        assert_eq!(
            gesture(
                Press::Exited,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                a,
                Some(L())
            ),
            Gesture::Drop
        );
    }

    /// Perturbation: read `Chord::Other` as Cmd -- fails alone.
    #[test]
    fn shift_beats_cmd_so_a_cmd_shift_click_is_a_selection() {
        assert_eq!(
            gesture(
                Press::Down,
                Chord::Other,
                Clicks::Single,
                Focus::Gained,
                None,
                Some(L())
            ),
            Gesture::Pass
        );
    }

    /// Perturbation: arm on any Cmd press -- fails alone.
    #[test]
    fn a_cmd_click_off_a_link_is_not_swallowed() {
        assert_eq!(
            gesture(
                Press::Down,
                Chord::Cmd,
                Clicks::Single,
                Focus::Gained,
                None,
                None
            ),
            Gesture::Pass
        );
    }

    /// A Terminal.app user Cmd-double-clicks: the second press must not
    /// leave a word selection behind or reach a tracking child, and must
    /// not open twice. Perturbation: pass a repeat press -- fails alone.
    #[test]
    fn a_repeat_click_on_a_link_is_swallowed_whole_and_opens_nothing() {
        let down = gesture(
            Press::Down,
            Chord::Cmd,
            Clicks::Repeat,
            Focus::Gained,
            None,
            Some(L()),
        );
        assert_eq!(
            down,
            Gesture::Arm(Armed {
                link: L(),
                clicks: Clicks::Repeat
            })
        );
        let up = gesture(
            Press::Up,
            Chord::Cmd,
            Clicks::Repeat,
            Focus::Gained,
            armed(Clicks::Repeat),
            Some(L()),
        );
        assert_eq!(up, Gesture::Drop);
    }

    /// Perturbation: drop the focus test -- fails alone.
    #[test]
    fn a_press_into_a_background_window_arms_nothing() {
        assert_eq!(
            gesture(
                Press::Down,
                Chord::Cmd,
                Clicks::Single,
                Focus::Lost,
                None,
                Some(L())
            ),
            Gesture::Pass
        );
    }
}
