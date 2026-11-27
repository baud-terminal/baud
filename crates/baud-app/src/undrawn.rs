//! **What a session showed, of what its programs sent**: the bytes the
//! searches for this machine's names read, with each shell folder report
//! (`OSC 7`) written as the window's title would show its folder.
//!
//! The shell integration reports its folder at every prompt as
//! `OSC 7 ; file://HOST/PATH`. The core draws none of it: it keeps the
//! folder (`Term::working_directory`), and the window's title shows that
//! folder as [`folder_text`] spells it -- relative to the session's home,
//! never the host. So a report is searched as that text: a forbidden path
//! the title would draw is still found, and the host, which nothing draws,
//! is not. Everything else is searched as it came: printed text, titles,
//! prompt marks, links, every other `OSC`.
//!
//! **A report is found by framing that can only keep bytes.** `OSC` is
//! `ESC ]`, ended by BEL or by `ESC \`, the two terminators
//! `[[source: ctlseqs]]` gives. One ended any other way -- `CAN`, `SUB`,
//! `ESC` and anything but `\`, a control inside it, the end of the bytes --
//! is kept whole, so a mistake here keeps what a program printed and never
//! drops it. `0x9D` is not an `OSC`: the session runs UTF-8, where it is a
//! continuation byte. Each framed body is classified by the core's own
//! parser (`baud_domain::Osc::parse`), and only a `file:` report whose path
//! decodes is written as its folder.
//!
//! Shared, through `#[path]`, by baud's README session test and the demo.

use baud_domain::{DirectoryReport, Osc, OscBody};

use crate::folder::folder_text;

/// `bytes` with each shell folder report written as the title shows its
/// folder against `home`, the session's home.
pub fn shown_bytes(bytes: &[u8], home: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"\x1b]") {
            if let Some((body, len)) = framed(&bytes[i + 2..]) {
                match folder(body, home) {
                    Some(text) => out.extend_from_slice(text.as_bytes()),
                    None => out.extend_from_slice(&bytes[i..i + 2 + len]),
                }
                i += 2 + len;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// The body of an `OSC` whose introducer came just before `rest`, and the
/// bytes it takes with its terminator; none unless BEL or `ESC \` ends it
/// before any other control.
fn framed(rest: &[u8]) -> Option<(&[u8], usize)> {
    for (n, &b) in rest.iter().enumerate() {
        match b {
            0x07 => return Some((&rest[..n], n + 1)),
            0x1b => {
                return (rest.get(n + 1) == Some(&b'\\')).then_some((&rest[..n], n + 2));
            }
            0x00..=0x1f | 0x7f => return None,
            _ => {}
        }
    }
    None
}

/// What a body shows as, when it is a folder report: the folder as the
/// title spells it.
fn folder(body: &[u8], home: Option<&[u8]>) -> Option<String> {
    let Osc::WorkingDirectory(DirectoryReport::File(uri)) = Osc::parse(OscBody::Whole(body)) else {
        return None;
    };
    let mut buf = vec![0u8; body.len()];
    let path = uri.decode_path(&mut buf)?;
    Some(folder_text(path, home))
}

#[cfg(test)]
mod tests {
    use super::shown_bytes;

    const HOST: &str = "build-host.example";
    const HOME: &[u8] = b"/Users/me";

    /// Whether `bytes` hold `name`.
    fn holds(bytes: &[u8], name: &str) -> bool {
        bytes.windows(name.len()).any(|w| w == name.as_bytes())
    }

    fn report(path: &str, end: &str) -> String {
        format!("\x1b]7;file://{HOST}{path}{end}")
    }

    /// **A folder report is searched as the title shows its folder, and
    /// everything else as it came.** The host in printed text, and in a
    /// title (`OSC 2`), is still there; a report naming the host, ended by
    /// BEL or by `ESC \`, is not, and is the folder relative to the home;
    /// a report cut by `CAN`, and the host printed after it, are kept. A
    /// folder outside the home whose path the title shows whole keeps
    /// `/Users/` in sight, and the same folder under the home does not.
    /// Named breaks: strip every `OSC` (the title keeps no host); end a
    /// body at BEL alone (the `ESC \` report keeps its host); strip a body
    /// cut short (the host printed after it goes).
    #[test]
    fn a_folder_report_is_searched_as_the_title_shows_it() {
        let home = Some(HOME);
        let printed = format!("user@{HOST}:~$ ls\r\n");
        assert!(
            holds(&shown_bytes(printed.as_bytes(), home), HOST),
            "printed text"
        );
        let title = format!("\x1b]2;{HOST}: zsh\x07");
        assert!(holds(&shown_bytes(title.as_bytes(), home), HOST), "a title");
        let home_s = String::from_utf8_lossy(HOME).into_owned();
        for end in ["\x07", "\x1b\\"] {
            let r = report(&format!("{home_s}/src"), end);
            let shown = shown_bytes(r.as_bytes(), home);
            assert!(!holds(&shown, HOST), "{end:?}: the host is not drawn");
            assert!(!holds(&shown, "/Users/"), "{end:?}: the home is not drawn");
            assert_eq!(String::from_utf8_lossy(&shown), "~/src", "{end:?}");
        }
        let cut = format!("\x1b]7;file://{HOST}/x\x18{HOST} printed\r\n");
        let shown = shown_bytes(cut.as_bytes(), home);
        assert_eq!(shown, cut.as_bytes(), "a cut report is kept whole");
        let outside = report("/Users/you/x", "\x07");
        assert!(
            holds(&shown_bytes(outside.as_bytes(), home), "/Users/"),
            "outside the home"
        );
        let inside = report(&format!("{home_s}/x"), "\x07");
        assert!(
            !holds(&shown_bytes(inside.as_bytes(), home), "/Users/"),
            "under the home"
        );
        let other = "\x1b]8;;https://a.example\x07a\x1b]8;;\x07";
        assert_eq!(
            shown_bytes(other.as_bytes(), home),
            other.as_bytes(),
            "a link as it came"
        );
    }
}
