//! `OSC 3 ; prop=value`, "Set X property on top-level window. Pt should be
//! in the form \"prop=value\", or just \"prop\" to delete the property"
//! (`[[source: ctlseqs]]`). baud has no X window, so the properties with a
//! counterpart are mapped (note AV): the window name, ICCCM's `WM_NAME`
//! and EWMH's `_NET_WM_NAME`, is the title, and the icon name,
//! `WM_ICON_NAME` and `_NET_WM_ICON_NAME`, is the icon name; every other
//! property is counted and dropped.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 4,
};

/// **`WM_NAME` names the window, in ISO 8859-1**: xterm sets it with
/// `XSetTextProperty` typed `XA_STRING` (`ChangeXprop`, `[[source:
/// xterm-misc]]`), and ICCCM says the encoding "is implied by the type of
/// the property" (`[[source: icccm-2-0]]`, chapter 4, "WM_NAME
/// Property"), and "STRING as a type ... specifies the ISO Latin-1 character
/// set" (chapter 2, "TEXT Properties"). So UTF-8 bytes sent under UTF-8 read as Latin-1, as on an X
/// server. The value is everything after the first `=`.
#[test]
fn wm_name_names_the_window_in_iso_8859_1() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]3;WM_NAME=caf\xc3\xa9\x07");
        assert_eq!(t.title(), b"caf\xc3\xa9");
        assert_eq!(t.title_encoding(), InputEncoding::Latin1);
        support::feed(t, b"\x1b]3;WM_NAME=a=b\x1b\\");
        assert_eq!(t.title(), b"a=b", "split at the first =");
        assert_eq!(t.icon_name(), b"", "the icon name is its own");
    });
}

/// **`_NET_WM_NAME` names the window too, in UTF-8**: "The Client SHOULD
/// set this to the title of the window in UTF-8 encoding"
/// (`[[source: ewmh-1-5-app]]` 5.1), its type `UTF8_STRING`. One title
/// for both, the last write winning. A value that is not UTF-8 is no
/// `UTF8_STRING`: counted, and the title kept.
#[test]
fn net_wm_name_names_the_window_in_utf8() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]3;_NET_WM_NAME=caf\xc3\xa9\x07");
        assert_eq!(t.title(), b"caf\xc3\xa9");
        assert_eq!(t.title_encoding(), InputEncoding::Utf8);
        let dropped = t.properties_dropped();
        support::feed(t, b"\x1b]3;_NET_WM_NAME=\xff\xfe\x07");
        assert_eq!(t.title(), b"caf\xc3\xa9", "kept");
        assert_ne!(t.properties_dropped(), dropped, "counted");
        support::feed(t, b"\x1b]3;WM_NAME=x\x07");
        assert_eq!(t.title(), b"x", "the last write wins");
    });
}

/// **The icon name's two properties set the icon name, and only it.**
#[test]
fn the_icon_name_properties_set_the_icon_name() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]2;shell\x07\x1b]3;WM_ICON_NAME=sh\x07");
        assert_eq!(t.icon_name(), b"sh");
        assert_eq!(t.icon_name_encoding(), InputEncoding::Latin1);
        support::feed(t, b"\x1b]3;_NET_WM_ICON_NAME=\xc3\xa9\x07");
        assert_eq!(t.icon_name(), b"\xc3\xa9");
        assert_eq!(t.icon_name_encoding(), InputEncoding::Utf8);
        assert_eq!(t.title(), b"shell", "the title untouched");
    });
}

/// **A bare name deletes the property, and a deleted name is no name**: the
/// title or icon name is emptied, which the app shows as its own name, as a
/// window manager shows its default for a window with no `WM_NAME`.
#[test]
fn a_bare_name_deletes_the_name() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]0;both\x07\x1b]3;WM_NAME\x07");
        assert_eq!(t.title(), b"");
        assert_eq!(t.icon_name(), b"both");
        support::feed(t, b"\x1b]3;_NET_WM_ICON_NAME\x07");
        assert_eq!(t.icon_name(), b"");
    });
}

/// **Every other property is counted and dropped**: one no window here has
/// (`WM_CLASS`), a name in another case -- an X atom's name is matched
/// exactly -- an empty one, which xterm's `do_osc` drops for want of data
/// (`[[source: xterm-misc]]`), and `=x`, which names nothing. Each counts
/// as unsupported too, so the inventory sees it.
#[test]
fn every_other_property_is_counted_and_dropped() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b]2;kept\x07");
        let (dropped, unsupported) = (t.properties_dropped(), t.unsupported);
        for p in [&b"WM_CLASS=x"[..], b"wm_name=x", b"", b"=x", b"WM_CLASS"] {
            let before = t.properties_dropped();
            let mut seq = b"\x1b]3;".to_vec();
            seq.extend_from_slice(p);
            seq.push(0x07);
            support::feed(t, &seq);
            assert_ne!(t.properties_dropped(), before, "{p:?} counted");
        }
        assert_ne!(t.properties_dropped(), dropped);
        assert_eq!(t.unsupported, unsupported + 5);
        assert_eq!(t.title(), b"kept");
        assert_eq!(t.icon_name(), b"");
    });
}

/// **XTSMTITLE's hexadecimal mode is OSC 0, 1 and 2's, not OSC 3's**:
/// xterm's `ChangeXprop` sets the property from the bytes as sent, past
/// `ChangeGroup`, which alone decodes hexadecimal.
#[test]
fn the_hexadecimal_title_mode_does_not_apply() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[>0t\x1b]3;WM_NAME=6869\x07");
        assert_eq!(t.title(), b"6869");
        support::feed(t, b"\x1b]2;6869\x07");
        assert_eq!(t.title(), b"hi", "OSC 2 is decoded");
    });
}

/// **`[window] operations = "off"` refuses OSC 3 too, and counts it**:
/// xterm gates it under `allowWindowOps`, subcategory `SetXprop`
/// (`case OSC_X_Property`, `AllowWindowOps(xw, ewSetXprop)`,
/// `[[source: xterm-misc]]`). OSC 2 stays honoured.
#[test]
fn the_window_operations_setting_refuses_it() {
    support::with_term(&WS, |t| {
        t.set_window_ops(WindowOps::Off);
        let (refused, dropped) = (t.window_ops_refused(), t.properties_dropped());
        support::feed(t, b"\x1b]3;WM_NAME=x\x07");
        assert_eq!(t.title(), b"");
        assert_ne!(t.window_ops_refused(), refused, "counted");
        assert_eq!(t.properties_dropped(), dropped, "as refused, not dropped");
        support::feed(t, b"\x1b]2;y\x07");
        assert_eq!(t.title(), b"y");
        t.set_window_ops(WindowOps::On);
        support::feed(t, b"\x1b]3;WM_NAME=x\x07");
        assert_eq!(t.title(), b"x");
    });
}
