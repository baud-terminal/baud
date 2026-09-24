//! The VT100-era private modes, one test each.
//!
//! `[[source: ctlseqs]]` § "DEC Private Mode Set (DECSET)" and its reset twin.
//!
//! Several of these are input-side or renderer-side: the core has no keyboard
//! and no renderer, so "acting on" them means exposing them by accessor. That
//! is the only channel an embedder has, and it is what separates a recorded
//! mode from the "set and never read" failure `docs/vt-coverage.md` treats as a
//! bug. Each test therefore checks the *accessor*, not just that the sequence
//! parsed.

use crate::support;
use baud_core::*;

/// Owns the storage so a `Term` can borrow it.
///
/// `baud-core`'s storage is caller-provided, so something in the caller's
/// frame has to hold it; in `baud-core2` that was a `Box<Storage>` and here it
/// is an `Owned`. The struct survives the port unchanged in shape because its
/// job was always "outlive the borrow", which is the same job.
struct T {
    s: Owned,
}

fn with(bytes: &[u8], f: impl Fn(&Term) -> bool) -> bool {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    support::feed(&mut term, bytes);
    f(&term)
}

/// Every one of these is off at power-on and toggles both ways.
///
/// Table-driven because the property is the same for all of them and writing it
/// six times invites one to be subtly different. `?1`, `?4`, `?5` and `?1034`
/// are pure state; the interesting ones get their own test below.
///
/// `?1036` and `?1039` are *not* here, and that is the point: they default
/// **set**, so a table that asserts "off at power-on" would have to special-case
/// them. `key_encode.rs` covers them with the reason for the default. `?8` is
/// not here for the same reason: `[[source: vt510-decarm]]` gives "Default:
/// Repeat (set)", and `key_encode.rs` covers it with what it does to a key.
#[test]
fn each_mode_defaults_off_and_toggles() {
    type Get = fn(&Term) -> bool;
    let cases: &[(&str, &[u8], Get)] = &[
        ("?1 DECCKM", b"\x1b[?1", |t| t.cursor_keys_application()),
        ("?4 DECSCLM", b"\x1b[?4", |t| t.smooth_scroll()),
        ("?5 DECSCNM", b"\x1b[?5", |t| t.reverse_video()),
        ("?1034 eightBitInput", b"\x1b[?1034", |t| {
            t.eight_bit_input()
        }),
    ];
    for (name, prefix, get) in cases {
        let set = [prefix, b"h".as_slice()].concat();
        let reset = [prefix, b"h".as_slice(), prefix, b"l".as_slice()].concat();
        assert!(!with(b"", *get), "{name}: must be off at power-on");
        assert!(with(&set, *get), "{name}: set must take effect");
        assert!(!with(&reset, *get), "{name}: reset must take effect");
    }
}

/// The modes are independent: setting one must not move another.
///
/// They share a `u32` bitset, so a wrong shift would silently alias two of
/// them. It was a `u16` at 15 of 16 bits until `?1034`, `?1036` and `?1039`
/// stopped being one bit under one number, which took it to 17 and widened it.
#[test]
fn the_modes_do_not_alias_each_other() {
    let all = b"\x1b[?1h";
    assert!(with(all, |t| t.cursor_keys_application()));
    assert!(!with(all, |t| t.smooth_scroll()), "?1 must not set ?4");
    assert!(!with(all, |t| t.reverse_video()), "?1 must not set ?5");
    // `?8` is set at power-on, so the alias it can show is a *set*: reset it
    // first.
    assert!(
        !with(b"\x1b[?8l\x1b[?1h", |t| t.auto_repeat()),
        "?1 must not set ?8"
    );
    assert!(!with(all, |t| t.eight_bit_input()));
    // And the reverse direction.
    let m = b"\x1b[?1034h";
    assert!(with(m, |t| t.eight_bit_input()));
    assert!(!with(m, |t| t.cursor_keys_application()));
    // The three that were one bit. `?1034` must not disturb either prefix, and
    // turning one prefix off must leave the other alone -- four of the sixteen
    // rows of `[[source: ctlseqs]]` line 2337 distinguish them.
    assert!(
        with(m, |t| t.meta_sends_escape()),
        "?1034 h left ?1036 alone"
    );
    assert!(
        with(m, |t| t.alt_sends_escape()),
        "?1034 h left ?1039 alone"
    );
    let off = b"\x1b[?1039l";
    assert!(!with(off, |t| t.alt_sends_escape()));
    assert!(
        with(off, |t| t.meta_sends_escape()),
        "?1039 l left ?1036 alone"
    );
    assert!(!with(off, |t| t.eight_bit_input()));
}

/// `?3` DECCOLM does the clearing the spec mandates, but does **not** resize.
///
/// Geometry is caller-owned through `Term::resize`. Resizing behind the caller
/// would desynchronise it from the application, so the request is recorded and
/// `Term::column_mode()` reports it.
#[test]
fn deccolm_clears_and_records_but_does_not_resize() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    support::feed(&mut term, b"\x1b[5;10r\x1b[3;3Hsome text\x1b[?3h");

    assert!(term.column_mode(), "the request is recorded");
    let (r, c) = term.cursor();
    assert_eq!((r.index(), c.index()), (0, 0), "DECCOLM homes the cursor");
    assert_eq!(term.geometry().0.index(), 200, "width is unchanged");

    // Screen cleared, and the scrolling region reset with it.
    let blank = {
        let mut u = T {
            s: Owned::new(&support::CORPUS),
        };
        let t2 = Term::new(u.s.storage(), &support::CORPUS);
        support::screen(&t2)
    };
    assert_eq!(support::screen(&term), blank, "the screen is cleared");
}

/// `?12` is the blink axis of DECSCUSR reached by another route, so it must
/// change blink **without** changing the shape.
#[test]
fn cursor_blink_does_not_change_the_cursor_shape() {
    let style = |b: &[u8]| {
        let mut t = T {
            s: Owned::new(&support::CORPUS),
        };
        let mut term = Term::new(t.s.storage(), &support::CORPUS);
        support::feed(&mut term, b);
        term.cursor_style()
    };
    // Steady bar, then ?12h: blinking bar, still a bar.
    assert_eq!(style(b"\x1b[6 q"), CursorStyle::SteadyBar);
    assert_eq!(style(b"\x1b[6 q\x1b[?12h"), CursorStyle::BlinkingBar);
    assert_eq!(style(b"\x1b[6 q\x1b[?12h\x1b[?12l"), CursorStyle::SteadyBar);
    // And from an underline, it stays an underline.
    assert_eq!(style(b"\x1b[4 q\x1b[?12h"), CursorStyle::BlinkingUnderline);
}

/// `?47` switches buffers and does **not** save the cursor; `?1049` does.
///
/// `?1049` exists because applications kept getting `?47` wrong, so the two are
/// deliberately not aliases.
#[test]
fn legacy_alt_screen_does_not_save_the_cursor() {
    let pos = |b: &[u8]| {
        let mut t = T {
            s: Owned::new(&support::CORPUS),
        };
        let mut term = Term::new(t.s.storage(), &support::CORPUS);
        support::feed(&mut term, b);
        let (r, c) = term.cursor();
        (r.index(), c.index())
    };
    // ?1049 restores the cursor on the way out.
    assert_eq!(pos(b"\x1b[9;20H\x1b[?1049h\x1b[1;1H\x1b[?1049l"), (8, 19));
    // ?47 does not: the cursor is wherever the alt screen left it.
    assert_eq!(pos(b"\x1b[9;20H\x1b[?47h\x1b[1;1H\x1b[?47l"), (0, 0));
}

/// Every one of these is reported by DECRQM, which is what makes them
/// discoverable rather than merely present.
#[test]
fn decrqm_reports_each_mode() {
    let ask = |b: &[u8]| {
        let mut t = T {
            s: Owned::new(&support::CORPUS),
        };
        let mut term = Term::new(t.s.storage(), &support::CORPUS);
        support::feed(&mut term, b);
        term.reply().to_vec()
    };
    assert_eq!(ask(b"\x1b[?1$p"), b"\x1b[?1;2$y", "?1 reset at power-on");
    assert_eq!(ask(b"\x1b[?1h\x1b[?1$p"), b"\x1b[?1;1$y");
    assert_eq!(ask(b"\x1b[?5h\x1b[?5$p"), b"\x1b[?5;1$y");
    assert_eq!(ask(b"\x1b[?8h\x1b[?8$p"), b"\x1b[?8;1$y");
    assert_eq!(ask(b"\x1b[?8$p"), b"\x1b[?8;1$y", "?8 set at power-on");
    assert_eq!(ask(b"\x1b[?8l\x1b[?8$p"), b"\x1b[?8;2$y");
    assert_eq!(ask(b"\x1b[?45h\x1b[?45$p"), b"\x1b[?45;1$y");
    assert_eq!(ask(b"\x1b[?1034h\x1b[?1034$p"), b"\x1b[?1034;1$y");
}

/// A multi-parameter DECRST applies to **every** parameter.
///
/// `CSI ? 7 ; 25 l` reset autowrap and left the cursor visible until this was
/// fixed; rxvt's own reset string is `CSI ? 1 ; 3 ; 4 ; 6 l`.
#[test]
fn a_multi_parameter_form_applies_to_all_of_them() {
    let mut t = T {
        s: Owned::new(&support::CORPUS),
    };
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    support::feed(&mut term, b"\x1b[?1;4;5;8;1034h");
    assert!(term.cursor_keys_application(), "?1");
    assert!(term.smooth_scroll(), "?4");
    assert!(term.reverse_video(), "?5");
    assert!(term.auto_repeat(), "?8");
    assert!(term.eight_bit_input(), "?1034");

    support::feed(&mut term, b"\x1b[?1;4;5;8;1034l");
    assert!(!term.cursor_keys_application(), "?1 reset");
    assert!(!term.smooth_scroll(), "?4 reset");
    assert!(!term.reverse_video(), "?5 reset");
    assert!(!term.auto_repeat(), "?8 reset");
    assert!(!term.eight_bit_input(), "?1034 reset");
}

/// RIS clears every one, so a crashed program cannot strand the next.
#[test]
fn ris_clears_them_all() {
    let after = b"\x1b[?1;4;5;8;1034h\x1bc";
    assert!(!with(after, |t| t.cursor_keys_application()));
    assert!(!with(after, |t| t.smooth_scroll()));
    assert!(!with(after, |t| t.reverse_video()));
    assert!(!with(after, |t| t.eight_bit_input()));
    assert!(!with(after, |t| t.column_mode()));
    // RIS restores, and restoring is not the same as clearing: `?1036` and
    // `?1039` are set at power-on, so the fixed point is *on*. A reset that
    // cleared them would leave the next program without an ESC prefix and no
    // way to know why.
    let after = b"\x1b[?1036;1039l\x1bc";
    assert!(with(after, |t| t.meta_sends_escape()));
    assert!(with(after, |t| t.alt_sends_escape()));
    // And `?8`: `[[source: vt510-decarm]]` gives "Default: Repeat (set)".
    assert!(with(b"\x1b[?8l\x1bc", |t| t.auto_repeat()));
}
