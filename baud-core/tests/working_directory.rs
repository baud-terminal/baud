//! `OSC 7 ; Pt`: the shell's working directory, as an RFC 8089 `file:` URI
//! (`[[source: rfc8089]]`). No standard defines the sequence (note AK,
//! section 1.5); Apple's shell files send it at each prompt as
//! `file://$HOST` and the percent-encoded path. The core keeps the last one
//! it could read for the embedder, which uses it for the window's title.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 4,
};

fn dir<'t>(t: &'t Term<'_>) -> Option<(FileHost<'t>, &'t [u8])> {
    t.working_directory().map(|d| (d.host, d.path))
}

/// **Apple's form is kept as sent, the path decoded**: the host it named,
/// for the embedder to compare with its own machine (RFC 8089 § 3), and
/// the path's `%20` as the space it encodes (RFC 3986 § 2.1).
#[test]
fn apples_report_keeps_its_host_and_decodes_its_path() {
    support::with_term(&WS, |t| {
        assert_eq!(dir(t), None, "nothing before a report");
        support::feed(t, b"\x1b]7;file://mac.local/Users/me/My%20Code\x07");
        assert_eq!(
            dir(t),
            Some((FileHost::Named(b"mac.local"), &b"/Users/me/My Code"[..]))
        );
        assert_eq!(t.strings_ignored, 0, "no longer an unsupported OSC");
    });
}

/// **No host and `localhost` are this machine**, as RFC 8089 § 2 says of
/// both, and the authority-less `file:/path` is its `local-path` form.
/// Either terminator ends it.
#[test]
fn no_host_and_localhost_are_local() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]7;file:///tmp\x1b\\");
        assert_eq!(dir(t), Some((FileHost::Local, &b"/tmp"[..])));
        support::feed(t, b"\x1b]7;file://LOCALHOST/var\x07");
        assert_eq!(dir(t), Some((FileHost::Local, &b"/var"[..])));
        support::feed(t, b"\x1b]7;file:/etc\x07");
        assert_eq!(dir(t), Some((FileHost::Local, &b"/etc"[..])));
    });
}

/// **Every report replaces the last, and one the core cannot read leaves
/// none, counted**: the shell moved somewhere, so the directory before is
/// no longer where it is. Another scheme, a byte off the grammar and a NUL
/// are each refused.
#[test]
fn a_refused_report_leaves_no_directory_and_is_counted() {
    support::with_term(&WS, |t| {
        for refused in [
            &b"kitty-shell-cwd://mac/Users/me"[..],
            b"file:///My Code",
            b"file:///a%00b",
            b"file:///a?b",
            b"",
        ] {
            support::feed(t, b"\x1b]7;file:///tmp\x07");
            assert!(dir(t).is_some());
            let (before, unsupported) = (t.directories_refused(), t.unsupported);
            support::feed(t, &[b"\x1b]7;", refused, b"\x07"].concat());
            let shown = String::from_utf8_lossy(refused);
            assert_eq!(dir(t), None, "{shown:?} leaves none");
            assert_ne!(t.directories_refused(), before, "{shown:?} is counted");
            assert_eq!(t.unsupported, unsupported + 1, "{shown:?}");
        }
    });
}

/// **A body cut at `OSC_MAX` is refused**: the path that survived the cut
/// names another directory, a parent or a prefix of a name.
#[test]
fn a_report_cut_at_osc_max_is_refused() {
    support::with_term(&WS, |t| {
        let long = [&b"\x1b]7;file:///"[..], &vec![b'a'; OSC_MAX], b"\x07"].concat();
        let before = t.directories_refused();
        support::feed(t, &long);
        assert_eq!(dir(t), None);
        assert_ne!(t.directories_refused(), before);
        let fits = [&b"\x1b]7;file:///"[..], &vec![b'a'; OSC_MAX - 20], b"\x07"].concat();
        support::feed(t, &fits);
        assert_eq!(dir(t).map(|(_, p)| p.len()), Some(OSC_MAX - 19));
    });
}

/// The same report fed a byte at a time.
#[test]
fn a_report_is_chunk_invariant() {
    support::with_term(&WS, |t| {
        for b in b"\x1b]7;file://h/a%C3%A9\x1b\\" {
            support::feed(t, &[*b]);
        }
        assert_eq!(dir(t), Some((FileHost::Named(b"h"), "/aé".as_bytes())));
    });
}

/// **`RIS` and `DECSTR` keep it**: the terminal is reset, and the shell is
/// still where it said it was.
#[test]
fn the_resets_keep_the_directory() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]7;file:///tmp\x07\x1bc");
        assert_eq!(dir(t), Some((FileHost::Local, &b"/tmp"[..])), "RIS");
        support::feed(t, b"\x1b[!p");
        assert_eq!(dir(t), Some((FileHost::Local, &b"/tmp"[..])), "DECSTR");
    });
}

/// **Under ISO 8859-1 an `OSC` reads a GR byte as its 7-bit byte**
/// (`[[source: dec-std-070]]` 3.5.4.5, the rule every `OSC` body takes), so
/// an unencoded `E9` is an `i` by the time the URI is read: on the grammar,
/// and kept so. An encoded one decodes as the bytes it names.
#[test]
fn under_latin1_a_gr_byte_is_its_seven_bit_byte() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b%@\x1b]7;file:///caf\xe9\x07");
        assert_eq!(dir(t), Some((FileHost::Local, &b"/cafi"[..])));
        support::feed(t, b"\x1b]7;file:///caf%E9\x07");
        assert_eq!(dir(t), Some((FileHost::Local, &b"/caf\xe9"[..])));
    });
}
