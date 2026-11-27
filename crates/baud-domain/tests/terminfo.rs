//! terminfo padding, read from the pinned entries.
//!
//! **Every pinned `.ti` is a positive control**: the parser has to find the
//! flash each entry carries and nothing where an entry has none, or a probe
//! that cannot fire would read every entry as `Absent` and pass. The test
//! lists the directory, so a pin added beside them fails until it is a
//! case. Then one test for each refusal the grammar leaves to this reader.
use baud_domain::terminfo::{
    Delay, Flash, FlashError, Force, Padding, PaddingError, Per, flash, string_capability,
};
use core::time::Duration;

macro_rules! pinned {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/research/sources/terminfo/",
            $name,
            ".ti"
        ))
    };
}

fn padded(millis: u16, force: Force) -> Flash {
    let text = format!("{millis}");
    let p = Padding::parse(text.as_bytes()).expect("a plain number parses");
    Flash::Padded(Padding { force, ..p })
}

#[test]
fn every_pinned_entry_reads_as_the_flash_it_carries() {
    let cases: [(&str, &[u8], Flash); 14] = [
        ("xterm", pinned!("xterm"), padded(100, Force::Mandatory)),
        (
            "xterm-256color",
            pinned!("xterm-256color"),
            padded(100, Force::Mandatory),
        ),
        // The same name from ncurses 6.6's database on Linux: its flash is
        // the padding the app compiles in from the macOS entry, mandatory
        // too, so `bell::LENGTH` is what a Linux `tput flash` waits.
        (
            "xterm-256color-ncurses66",
            pinned!("xterm-256color-ncurses66"),
            padded(100, Force::Mandatory),
        ),
        // Note AM's capture, the same bytes as `xterm-256color` since
        // 2026-10-18, pinned under its own key for the note's citations.
        (
            "xterm-256color-x",
            pinned!("xterm-256color-x"),
            padded(100, Force::Mandatory),
        ),
        (
            "xterm-16color",
            pinned!("xterm-16color"),
            padded(100, Force::Mandatory),
        ),
        ("dtterm", pinned!("dtterm"), padded(200, Force::Advisory)),
        ("nsterm", pinned!("nsterm"), padded(200, Force::Mandatory)),
        ("rxvt", pinned!("rxvt"), Flash::Unpadded),
        ("vt100", pinned!("vt100"), Flash::Absent),
        ("vt102", pinned!("vt102"), Flash::Absent),
        ("vt52", pinned!("vt52"), Flash::Absent),
        ("ansi", pinned!("ansi"), Flash::Absent),
        // The HP keyboard's witness, pinned for its keys; it has no flash.
        ("hpterm", pinned!("hpterm"), Flash::Absent),
        // The man page is pinned beside them and is no entry at all.
        (
            "terminfo.5",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../docs/research/sources/terminfo/terminfo.5"
            )),
            Flash::Absent,
        ),
    ];
    for (name, entry, want) in cases {
        assert_eq!(flash(entry), Ok(want), "{name}");
    }
    // Every `.ti` in the directory is one of the cases, so a pin added
    // beside them cannot escape the control.
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/research/sources/terminfo"
    );
    let mut on_disk: Vec<String> = std::fs::read_dir(dir)
        .expect("the pinned directory")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .into_string()
                .expect("ASCII")
        })
        .filter_map(|n| n.strip_suffix(".ti").map(str::to_owned))
        .collect();
    on_disk.sort();
    let mut listed: Vec<String> = cases
        .iter()
        .map(|(name, _, _)| (*name).to_owned())
        .filter(|n| *n != "terminfo.5")
        .collect();
    listed.sort();
    assert_eq!(on_disk, listed);
}

/// The value is found raw, escapes and all, and a field that cancels or
/// merely starts with the name is not it.
#[test]
fn a_capability_is_found_by_its_whole_name() {
    let xterm = pinned!("xterm-256color");
    assert_eq!(
        string_capability(xterm, b"flash"),
        Some(&b"\\E[?5h$<100/>\\E[?5l"[..])
    );
    assert_eq!(string_capability(xterm, b"bel"), Some(&b"^G"[..]));
    assert_eq!(string_capability(xterm, b"cr"), Some(&b"^M"[..]));
    // A boolean and a number have no `=`.
    assert_eq!(string_capability(xterm, b"am"), None);
    assert_eq!(string_capability(xterm, b"cols"), None);
    // `fla` is a prefix of `flash`, and the names field is not a capability.
    assert_eq!(string_capability(xterm, b"fla"), None);
    assert_eq!(
        string_capability(xterm, b"xterm-256color|xterm with 256 colors"),
        None
    );

    let cancelled = b"# a comment, with commas, flash=no\nx|test,\n\tflash@,\n\tbel=^G,\n";
    assert_eq!(
        string_capability(cancelled, b"flash"),
        None,
        "flash@ cancels it"
    );
    assert_eq!(
        string_capability(cancelled, b"bel"),
        Some(&b"^G"[..]),
        "the control"
    );
}

/// A comment line anywhere in the entry is skipped whole, commas and a
/// capability-shaped text included -- not only one on the first line, which
/// is the only place the pinned files carry one.
#[test]
fn a_comment_line_between_fields_is_skipped() {
    let entry =
        b"x|test,\n\tbel=^G,\n# old, flash=\\E[?5h$<999>\\E[?5l, kept\n\tflash=\\E[?5h$<50>\\E[?5l,\n";
    assert_eq!(
        string_capability(entry, b"flash"),
        Some(&b"\\E[?5h$<50>\\E[?5l"[..]),
        "the capability, not the comment"
    );
    assert_eq!(flash(entry), Ok(padded(50, Force::Advisory)));
}

/// The names field is not a capability, even when its text looks like one.
#[test]
fn the_names_field_is_never_a_capability() {
    let entry = b"flash=\\E[?5h$<50>\\E[?5l|odd names,\n\tbel=^G,\n";
    assert_eq!(string_capability(entry, b"flash"), None);
    assert_eq!(flash(entry), Ok(Flash::Absent));
    assert_eq!(
        string_capability(entry, b"bel"),
        Some(&b"^G"[..]),
        "the control"
    );
}

/// `%^` is the parameter language's XOR, not a control character, so the
/// `,` after it ends the field -- where a `^` read as an escape would take
/// the comma and swallow the next field whole.
#[test]
fn a_percent_caret_ends_at_its_comma() {
    let entry = b"x|test,\n\tsgr=%p1%p2%^%d,\n\txor=%^,\n\tflash=\\E[?5h$<50>\\E[?5l,\n";
    assert_eq!(string_capability(entry, b"xor"), Some(&b"%^"[..]));
    assert_eq!(flash(entry), Ok(padded(50, Force::Advisory)));
    // And after a `^%` -- control-% -- a `^` is an escape again.
    let control = b"x|test,\n\tctl=^%^,,\n\tbel=^G,\n";
    assert_eq!(string_capability(control, b"ctl"), Some(&b"^%^,"[..]));
}

/// An escaped comma and a `^,` do not end the field.
#[test]
fn escaped_commas_do_not_end_a_field() {
    let entry = b"x|test,\n\tone=a\\,b,\n\ttwo=^,c,\n\tthree=d,\n";
    assert_eq!(string_capability(entry, b"one"), Some(&b"a\\,b"[..]));
    assert_eq!(string_capability(entry, b"two"), Some(&b"^,c"[..]));
    assert_eq!(string_capability(entry, b"three"), Some(&b"d"[..]));
}

#[test]
fn the_grammar_reads_every_form_it_admits() {
    let ok = |t: &[u8], millis: u16, tenth: u16, per: Per, force: Force| {
        let p = Padding::parse(t).unwrap_or_else(|e| panic!("{t:?}: {e:?}"));
        assert_eq!(p.delay.millis_and_tenth(), (millis, tenth), "{t:?}");
        assert_eq!((p.per, p.force), (per, force), "{t:?}");
    };
    ok(b"5", 5, 0, Per::Once, Force::Advisory);
    ok(b"100/", 100, 0, Per::Once, Force::Mandatory);
    ok(b"2.5", 2, 5, Per::Once, Force::Advisory);
    ok(b".5", 0, 5, Per::Once, Force::Advisory);
    ok(b"3*", 3, 0, Per::Line, Force::Advisory);
    ok(b"3*/", 3, 0, Per::Line, Force::Mandatory);
    ok(b"3/*", 3, 0, Per::Line, Force::Mandatory);
    ok(b"6553.5", 6553, 5, Per::Once, Force::Advisory);
    ok(b"0", 0, 0, Per::Once, Force::Advisory);
}

#[test]
fn the_grammar_refuses_what_it_does_not_settle() {
    let no = |t: &[u8], want: PaddingError| {
        assert_eq!(Padding::parse(t), Err(want), "{t:?}");
    };
    no(b"", PaddingError::Empty);
    no(b"*", PaddingError::Empty);
    no(b"1.25", PaddingError::Precision);
    no(b"5.", PaddingError::Point);
    no(b"5./", PaddingError::Point);
    no(b"5**", PaddingError::Suffix);
    no(b"5//", PaddingError::Suffix);
    no(b"5x", PaddingError::Byte);
    no(b"5 ", PaddingError::Byte);
    // One tenth past what the representation holds, and far past it.
    no(b"6553.6", PaddingError::Overflow);
    no(b"6554", PaddingError::Overflow);
    no(b"99999999999999999999", PaddingError::Overflow);
}

/// The length is the entry's, in the clock's unit.
#[test]
fn a_delay_is_its_duration() {
    let d = |t: &[u8]| -> Delay { Padding::parse(t).unwrap().delay };
    assert_eq!(d(b"100/").duration(), Duration::from_millis(100));
    assert_eq!(d(b"2.5").duration(), Duration::from_micros(2500));
    assert_eq!(d(b"6553.5").duration(), Duration::from_micros(6_553_500));
    assert!(d(b"100") < d(b"200"), "ordered by length");
}

/// `\E` and `\e` are both ESC, so the shape is read after decoding.
#[test]
fn the_shape_is_read_after_the_escapes_are_decoded() {
    let entry = b"x|test,\n\tflash=\\e[?5h$<50>\\E[?5l,\n";
    assert_eq!(flash(entry), Ok(padded(50, Force::Advisory)));
    // Octal 033 is ESC too.
    let octal = b"x|test,\n\tflash=\\033[?5h$<50>\\033[?5l,\n";
    assert_eq!(flash(octal), Ok(padded(50, Force::Advisory)));
    // And so is `^[`, control-[.
    let caret = b"x|test,\n\tflash=^[[?5h$<50>^[[?5l,\n";
    assert_eq!(flash(caret), Ok(padded(50, Force::Advisory)));
}

/// Anything but set, one padding, reset is not a reversed screen held for a
/// length, and the build that reads it must fail rather than guess.
#[test]
fn a_flash_of_another_shape_is_refused() {
    let flash_of = |v: &str| {
        let entry = format!("x|test,\n\tflash={v},\n");
        flash(entry.as_bytes())
    };
    // The padding after the reset: the reversed screen is not held.
    assert_eq!(flash_of("\\E[?5h\\E[?5l$<100>"), Err(FlashError::Shape));
    // The padding before the set.
    assert_eq!(flash_of("$<100>\\E[?5h\\E[?5l"), Err(FlashError::Shape));
    // Reset first: a flash for a screen already reversed, which this reader
    // does not claim to understand.
    assert_eq!(flash_of("\\E[?5l$<100>\\E[?5h"), Err(FlashError::Shape));
    // Two paddings.
    assert_eq!(flash_of("\\E[?5h$<50>$<50>\\E[?5l"), Err(FlashError::Shape));
    // Something else between.
    assert_eq!(flash_of("\\E[?5h$<50>\\E[H\\E[?5l"), Err(FlashError::Shape));
    // Another mode: a bell, not a reversed screen.
    assert_eq!(flash_of("^G"), Err(FlashError::Shape));
    // Trailing bytes after the reset.
    assert_eq!(flash_of("\\E[?5h$<50>\\E[?5lx"), Err(FlashError::Shape));
    assert_eq!(
        flash_of("\\E[?5h$<50\\E[?5l"),
        Err(FlashError::Unterminated)
    );
    assert_eq!(flash_of("\\E[?5h$<50*>\\E[?5l"), Err(FlashError::PerLine));
    assert_eq!(
        flash_of("\\E[?5h$<5.25>\\E[?5l"),
        Err(FlashError::Padding(PaddingError::Precision))
    );
    // The control: the same builder reads the one shape it admits.
    assert_eq!(
        flash_of("\\E[?5h$<50>\\E[?5l"),
        Ok(padded(50, Force::Advisory))
    );
}

/// The reader is `const`, so a build can call it: a pinned entry that does
/// not parse is a compile error, not a run-time surprise.
#[test]
fn the_reader_runs_at_compile_time() {
    const XTERM: Result<Flash, FlashError> = flash(pinned!("xterm-256color"));
    assert_eq!(XTERM, Ok(padded(100, Force::Mandatory)));
}
