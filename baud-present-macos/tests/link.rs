//! The opener's parse half, without opening anything: `NSURL` parses what
//! the policy admits and refuses what it refuses, the scheme check can say
//! no, and LaunchServices can be asked for a handler without launching one.
//!
//! A developer's machine and not CI, like every test in this crate.
#![cfg(target_os = "macos")]

use baud_present_macos::link::{Agree, Opened, handler, host_name, parse, scheme_agrees};
use macos_liquid_glass::objc2_foundation::{NSString, NSURL};

/// The four admitted shapes parse to their own scheme, and a space does
/// not parse at all -- the positive control that the parse can refuse.
/// **Foundation is lenient where the policy is not**: a non-ASCII byte is
/// percent-encoded rather than refused, so `https://h/é` parses to
/// `https://h/%C3%A9`, a URL the user never saw. That is why the app's
/// own Byte rule refuses first and this parse is belt and braces, not the
/// gate; the case is pinned so the leniency cannot be mistaken for a
/// refusal again.
#[test]
fn nsurl_parses_what_the_policy_admits_and_refuses_what_it_refuses() {
    for (url, scheme) in [
        ("https://h/p?q#f", "https"),
        ("HTTP://h", "http"),
        ("mailto:a@b?subject=x", "mailto"),
        ("file:///tmp/x", "file"),
    ] {
        let u = parse(url).unwrap_or_else(|e| panic!("{url}: {e:?}"));
        let got = u.scheme().expect("a scheme").to_string();
        assert!(got.eq_ignore_ascii_case(scheme), "{url}: {got}");
    }
    assert!(matches!(parse("https://a b"), Err(Opened::Unparsed)));
    let lenient =
        parse("https://h/\u{e9}").expect("Foundation percent-encodes rather than refusing");
    assert_eq!(
        lenient.absoluteString().map(|s| s.to_string()).as_deref(),
        Some("https://h/%C3%A9")
    );
}

/// **The check is a claim only if a crafted mismatch fails it.**
/// Perturbation: make `scheme_agrees` answer `Yes` always -- fails alone.
#[test]
fn the_scheme_nsurl_parsed_must_be_the_scheme_the_policy_admitted() {
    let u = NSURL::URLWithString(&NSString::from_str("http://h")).expect("parses");
    assert_eq!(scheme_agrees("http://h", &u), Agree::Yes);
    assert_eq!(scheme_agrees("HTTP://h", &u), Agree::Yes);
    assert_eq!(scheme_agrees("https://h", &u), Agree::No);
}

/// LaunchServices names a handler for `https` -- a browser is registered
/// on any Mac -- and names none for a scheme nothing claims, without
/// opening either.
#[test]
fn launch_services_names_a_handler_for_https_without_opening_it() {
    let h = handler("https://example.com/");
    assert!(h.is_some(), "no application registered for https");
    assert!(
        handler("x-baud-no-such-scheme://x").is_none(),
        "a scheme nothing claims has no handler"
    );
}

#[test]
fn this_host_has_a_name() {
    assert!(!host_name().is_empty());
}
