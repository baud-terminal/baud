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

use baud_core::*;

struct T {
    s: Box<Storage>,
}

fn feed(t: &mut Term, b: &[u8]) {
    let mut i = 0;
    while i < b.len() {
        let n = READ_CHUNK.min(b.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&b[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }
}

fn with(bytes: &[u8], f: impl Fn(&Term) -> bool) -> bool {
    let mut t = T {
        s: Storage::boxed(),
    };
    let mut term = Term::new(&mut t.s, OpMode::Computed);
    feed(&mut term, bytes);
    f(&term)
}

/// Every one of these is off at power-on and toggles both ways.
///
/// Table-driven because the property is the same for all of them and writing it
/// six times invites one to be subtly different. `?1`, `?4`, `?5`, `?8` and
/// `?1034` are pure state; the interesting ones get their own test below.
#[test]
fn each_mode_defaults_off_and_toggles() {
    type Get = fn(&Term) -> bool;
    let cases: &[(&str, &[u8], Get)] = &[
        ("?1 DECCKM", b"\x1b[?1", |t| t.cursor_keys_application()),
        ("?4 DECSCLM", b"\x1b[?4", |t| t.smooth_scroll()),
        ("?5 DECSCNM", b"\x1b[?5", |t| t.reverse_video()),
        ("?8 DECARM", b"\x1b[?8", |t| t.auto_repeat()),
        ("?1034 meta", b"\x1b[?1034", |t| t.meta_sends_escape()),
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
/// They share a `u16` bitset, so a wrong shift would silently alias two of them
/// — and `ModeBit` is at 15 of its 16 bits.
#[test]
fn the_modes_do_not_alias_each_other() {
    let all = b"\x1b[?1h";
    assert!(with(all, |t| t.cursor_keys_application()));
    assert!(!with(all, |t| t.smooth_scroll()), "?1 must not set ?4");
    assert!(!with(all, |t| t.reverse_video()), "?1 must not set ?5");
    assert!(!with(all, |t| t.auto_repeat()), "?1 must not set ?8");
    assert!(!with(all, |t| t.meta_sends_escape()));
    // And the reverse direction.
    let m = b"\x1b[?1034h";
    assert!(with(m, |t| t.meta_sends_escape()));
    assert!(!with(m, |t| t.cursor_keys_application()));
}

/// `?3` DECCOLM does the clearing the spec mandates, but does **not** resize.
///
/// Geometry is caller-owned through `Term::resize`. Resizing behind the caller
/// would desynchronise it from the application, so the request is recorded and
/// `Term::column_mode()` reports it.
#[test]
fn deccolm_clears_and_records_but_does_not_resize() {
    let mut t = T {
        s: Storage::boxed(),
    };
    let mut term = Term::new(&mut t.s, OpMode::Computed);
    feed(&mut term, b"\x1b[5;10r\x1b[3;3Hsome text\x1b[?3h");

    assert!(term.column_mode(), "the request is recorded");
    let (r, c) = term.cursor();
    assert_eq!((r.index(), c.index()), (0, 0), "DECCOLM homes the cursor");
    assert_eq!(term.geometry().0.index(), 200, "width is unchanged");

    // Screen cleared, and the scrolling region reset with it.
    let blank = {
        let mut u = T {
            s: Storage::boxed(),
        };
        let t2 = Term::new(&mut u.s, OpMode::Computed);
        t2.viewport().to_vec()
    };
    assert_eq!(term.viewport(), &blank[..], "the screen is cleared");
}

/// `?12` is the blink axis of DECSCUSR reached by another route, so it must
/// change blink **without** changing the shape.
#[test]
fn cursor_blink_does_not_change_the_cursor_shape() {
    let style = |b: &[u8]| {
        let mut t = T {
            s: Storage::boxed(),
        };
        let mut term = Term::new(&mut t.s, OpMode::Computed);
        feed(&mut term, b);
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
            s: Storage::boxed(),
        };
        let mut term = Term::new(&mut t.s, OpMode::Computed);
        feed(&mut term, b);
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
            s: Storage::boxed(),
        };
        let mut term = Term::new(&mut t.s, OpMode::Computed);
        feed(&mut term, b);
        term.reply().to_vec()
    };
    assert_eq!(ask(b"\x1b[?1$p"), b"\x1b[?1;2$y", "?1 reset at power-on");
    assert_eq!(ask(b"\x1b[?1h\x1b[?1$p"), b"\x1b[?1;1$y");
    assert_eq!(ask(b"\x1b[?5h\x1b[?5$p"), b"\x1b[?5;1$y");
    assert_eq!(ask(b"\x1b[?8h\x1b[?8$p"), b"\x1b[?8;1$y");
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
        s: Storage::boxed(),
    };
    let mut term = Term::new(&mut t.s, OpMode::Computed);
    feed(&mut term, b"\x1b[?1;4;5;8;1034h");
    assert!(term.cursor_keys_application(), "?1");
    assert!(term.smooth_scroll(), "?4");
    assert!(term.reverse_video(), "?5");
    assert!(term.auto_repeat(), "?8");
    assert!(term.meta_sends_escape(), "?1034");

    feed(&mut term, b"\x1b[?1;4;5;8;1034l");
    assert!(!term.cursor_keys_application(), "?1 reset");
    assert!(!term.smooth_scroll(), "?4 reset");
    assert!(!term.reverse_video(), "?5 reset");
    assert!(!term.auto_repeat(), "?8 reset");
    assert!(!term.meta_sends_escape(), "?1034 reset");
}

/// RIS clears every one, so a crashed program cannot strand the next.
#[test]
fn ris_clears_them_all() {
    let after = b"\x1b[?1;4;5;8;1034h\x1bc";
    assert!(!with(after, |t| t.cursor_keys_application()));
    assert!(!with(after, |t| t.smooth_scroll()));
    assert!(!with(after, |t| t.reverse_video()));
    assert!(!with(after, |t| t.auto_repeat()));
    assert!(!with(after, |t| t.meta_sends_escape()));
    assert!(!with(after, |t| t.column_mode()));
}
