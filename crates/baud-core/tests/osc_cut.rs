//! **A control string cut at the body's bound is not acted on as if whole.**
//! An OSC body longer than the bound baud keeps is cut there, and a cut
//! colour spec is still a well-formed spec: `rgb:80/80/8` reads as a third
//! colour, never the one sent. A list keeps the items whose `;` arrived,
//! a prompt mark's options among them; a value that sets one thing is
//! refused; text shown to the user is shown to its last whole character,
//! and a notification ends in `…`.

mod support;

use baud_core::*;

/// The bound an OSC body is kept to: a body that reaches it is read as cut.
const KEPT: usize = 512;

/// `1;rgb:...;` filler pairs of exactly `len` bytes (at least 12), each 12
/// to 21 bytes as its three channels take one to four hex digits.
fn filler(mut len: usize) -> Vec<u8> {
    let pair = |n: usize| {
        // 9 bytes of `1;rgb://;` and three channels of 1..=4 digits.
        let digits = n - 9;
        let (a, b) = (digits / 3, digits % 3);
        let ch = |i: usize| "a".repeat(a + usize::from(i < b));
        format!("1;rgb:{}/{}/{};", ch(0), ch(1), ch(2)).into_bytes()
    };
    let mut out = Vec::new();
    while len > 33 {
        out.extend(pair(21));
        len -= 21;
    }
    match len {
        12..=21 => out.extend(pair(len)),
        _ => {
            out.extend(pair(12));
            out.extend(pair(len - 12));
        }
    }
    out
}

/// A terminal fed `bytes`.
fn fed(bytes: &[u8], f: impl FnOnce(&Term<'_>)) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    f(&t);
}

/// **A cut `OSC 4` sets no colour it was not sent.** For every cut point
/// inside the last pair's spec, entry 7 is either the colour sent,
/// `80/80/80`, or untouched; the filler before it, whole, is applied.
#[test]
fn a_cut_osc_4_sets_no_colour_it_was_not_sent() {
    let last = b"7;rgb:80/80/80";
    for kept in 3..last.len() {
        // `4;` + filler + the last pair's first `kept` bytes = KEPT.
        let body = [b"4;".as_slice(), &filler(KEPT - 2 - kept), last].concat();
        assert!(body.len() > KEPT, "the premise: the body is cut");
        let seq = [b"\x1b]".as_slice(), &body, b"\x1b\\"].concat();
        fed(&seq, |t| {
            assert_eq!(
                t.palette(Chan::from_u8(1)),
                Some((0xaa, 0xaa, 0xaa)),
                "the control: the whole filler pairs set entry 1, cut at {kept}"
            );
            let got = t.palette(Chan::from_u8(7));
            assert!(
                got.is_none() || got == Some((0x80, 0x80, 0x80)),
                "cut {kept} bytes into `{}`: entry 7 is {got:?}",
                String::from_utf8_lossy(last)
            );
        });
    }
}

/// **A cut `OSC 5` sets no special colour it was not sent**, as `OSC 4`.
#[test]
fn a_cut_osc_5_sets_no_special_colour_it_was_not_sent() {
    // OSC 5: special 1 set by the filler, then special 0 (bold) cut in its
    // spec.
    let last = b"0;rgb:80/80/80";
    for kept in 3..last.len() {
        let body = [b"5;".as_slice(), &filler(KEPT - 2 - kept), last].concat();
        assert!(body.len() > KEPT, "the premise: the body is cut");
        let seq = [b"\x1b]".as_slice(), &body, b"\x1b\\"].concat();
        fed(&seq, |t| {
            let got = t.special_color(SpecialColor::Bold);
            assert!(
                got.is_none() || got == Some((0x80, 0x80, 0x80)),
                "OSC 5 cut {kept} bytes into the last pair: bold is {got:?}"
            );
        });
    }
}

/// **A cut `OSC 104` resets no entry the cut fell in**: entry 3 set, then
/// a reset list cut inside its last index, `33` cut to `3`.
#[test]
fn a_cut_osc_104_resets_no_entry_the_cut_fell_in() {
    // `104;20;` and `200;` to the byte before the bound, each whole, then
    // `33`, of which the bound keeps the first `3`.
    let mut body = b"104;20;".to_vec();
    while body.len() < KEPT - 1 {
        body.extend(b"200;");
    }
    assert_eq!(body.len(), KEPT - 1, "the premise: the last `;` is whole");
    body.extend(b"33");
    let seq = [
        b"\x1b]4;3;rgb:12/34/56\x1b\\".as_slice(),
        b"\x1b]",
        &body,
        b"\x1b\\",
    ]
    .concat();
    fed(&seq, |t| {
        assert_eq!(
            t.palette(Chan::from_u8(3)),
            Some((0x12, 0x34, 0x56)),
            "a reset of `3` that was `33` cut reset entry 3"
        );
    });
}

/// `head`, then `a` until `é` (C3 A9) would end the body at `len` bytes.
fn ending_in_e_acute(head: &[u8], len: usize) -> Vec<u8> {
    let mut b = head.to_vec();
    b.extend(b"a".repeat(len - head.len() - 2));
    b.extend("é".as_bytes());
    b
}

/// The OSC with `body`, ended by `ST`.
fn osc(body: &[u8]) -> Vec<u8> {
    [b"\x1b]".as_slice(), body, b"\x1b\\"].concat()
}

/// **A cut title keeps its whole characters**: an `OSC 2` cut between the
/// two bytes of a final `é` is the `a`s before it, with no half character
/// to draw as U+FFFD. The control: the same title within the bound keeps
/// its `é`.
#[test]
fn a_cut_title_keeps_its_whole_characters() {
    fed(&osc(&ending_in_e_acute(b"2;", KEPT - 1)), |t| {
        assert!(t.title().ends_with("é".as_bytes()), "the control: whole");
    });
    fed(&osc(&ending_in_e_acute(b"2;", KEPT + 1)), |t| {
        assert_eq!(
            t.title(),
            b"a".repeat(KEPT - 3).as_slice(),
            "cut inside the `é`"
        );
    });
}

/// **A cut `OSC 3` window name keeps its whole characters**, as a title
/// does: `_NET_WM_NAME`'s `UTF8_STRING` cut inside its final `é` names the
/// window with the `a`s before it, where it used to be dropped as not
/// UTF-8. The control: within the bound, the `é` is kept.
#[test]
fn a_cut_osc_3_name_keeps_its_whole_characters() {
    let head = b"2;before\x1b\\\x1b]3;_NET_WM_NAME=";
    let at = b"2;before\x1b\\\x1b]".len();
    fed(&osc(&ending_in_e_acute(head, at + KEPT - 1)), |t| {
        assert!(t.title().ends_with("é".as_bytes()), "the control: whole");
    });
    fed(&osc(&ending_in_e_acute(head, at + KEPT + 1)), |t| {
        let name = b"3;_NET_WM_NAME=".len();
        assert_eq!(
            t.title(),
            b"a".repeat(KEPT - name - 1).as_slice(),
            "cut inside the `é`"
        );
    });
}

/// A term with a notifications area, fed `bytes`.
fn noticed(bytes: &[u8], f: impl FnOnce(&Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_notices(NoticeBudget::with_icon(4096));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_notice_support(Some(NoticeSupport {
        actions: Activation::FOCUS,
        close_events: CloseEvents::Tracked,
        occasions: Occasions::UnfocusedAndInvisible,
        urgencies: Urgencies::All,
        sounds: Sounds::Standard,
        expiry: ExpirySupport::Expires,
        icons: IconSupport::NamesAndImages,
        buttons: ButtonSupport::Shown,
    }));
    support::feed(&mut t, bytes);
    f(&t);
}

/// **A cut notification shows that it was cut**: an `OSC 9` past the bound
/// is its whole characters and `…`, and an `OSC 777` body the same. The
/// control: a message within the bound has no `…`.
#[test]
fn a_cut_notification_ends_in_an_ellipsis() {
    let a = |n: usize| b"a".repeat(n);
    noticed(&osc(&ending_in_e_acute(b"9;", KEPT - 1)), |t| {
        let n = t.notices().next().expect("posted");
        assert!(n.title.ends_with("é".as_bytes()), "the control: whole");
    });
    noticed(&osc(&ending_in_e_acute(b"9;", KEPT + 1)), |t| {
        let n = t.notices().next().expect("posted");
        assert_eq!(n.title, [a(KEPT - 3), "…".into()].concat(), "OSC 9 cut");
    });
    let head = b"777;notify;T;";
    noticed(&osc(&ending_in_e_acute(head, KEPT + 1)), |t| {
        let n = t.notices().next().expect("posted");
        assert_eq!(n.title, b"T");
        assert_eq!(
            n.body,
            [a(KEPT - head.len() - 1), "…".into()].concat(),
            "OSC 777 cut"
        );
    });
}

/// **A cut allow query is refused**: `OSC 62` past the bound gets no
/// answer, where its name, cut, was answered as a category baud lacks. The
/// control: within the bound it is answered.
#[test]
fn a_cut_allow_query_is_refused() {
    let ask = |len: usize| {
        let mut b = b"62;".to_vec();
        b.extend(b"x".repeat(len - 3));
        osc(&b)
    };
    fed(&ask(KEPT - 1), |t| {
        assert!(!t.reply().is_empty(), "the control: answered");
    });
    fed(&ask(KEPT + 1), |t| {
        assert_eq!(t.reply(), b"", "a cut OSC 62 was answered");
    });
}

/// **A cut prompt mark keeps its kind and its whole options**: a `D` whose
/// `err=7` is cut to `err=` ends its command with the code it gave, 3, and
/// not as a success, which an empty `err` means. The control: whole, the
/// `err` is read.
#[test]
fn a_cut_mark_option_is_not_applied() {
    let done = |len: usize| {
        let mut d = b"133;D;3;x=".to_vec();
        d.extend(b"a".repeat(len - d.len() - b";err=".len()));
        d.extend(b";err=7");
        [
            b"\x1b]133;A\x07$ \x1b]133;B\x07\r\n\x1b]133;C\x07out\r\n".as_slice(),
            &osc(&d),
            b"\x1b]133;A\x07$ ",
        ]
        .concat()
    };
    fed(&done(KEPT - 2), |t| {
        let s: Vec<ExitStatus> = t.prompts().map(|p| p.status).collect();
        assert!(
            s.contains(&ExitStatus::Failed(Some(ExitCode::of(7)))),
            "the control: {s:?}"
        );
    });
    fed(&done(KEPT), |t| {
        let s: Vec<ExitStatus> = t.prompts().map(|p| p.status).collect();
        assert!(
            s.contains(&ExitStatus::Exited(ExitCode::of(3))),
            "a cut `err=` was applied: {s:?}"
        );
    });
}
