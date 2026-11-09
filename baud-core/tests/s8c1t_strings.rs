//! **After S8C1T every string reply is framed in C1**, as xterm frames it:
//! `unparseputc1` sends `OSC` and `DCS` as 9D and 90 and an `ST` final as
//! 9C once `control_eight_bits` is set, and a `BEL` final as `BEL`
//! (`[[source: xterm-sgr]]` charproc.c 9356-9365, `ReportAnsiColorRequest` in
//! `[[source: xterm-misc]]`). DECRQSS already answered 90 ... 9C; the OSC
//! answers and XTVERSION answered `ESC ]`, `ESC P` and `ESC \`.

mod support;

use baud_core::*;
use baud_domain::BackingScale;

/// S8C1T: replies in 8-bit C1 from here on.
const S8C1T: &[u8] = b"\x1b G";

/// How an answer ends: as its query did, or with `ST` whatever the query
/// ended with -- kitty's `OSC 21`, as kitty ends every OSC, and iTerm2's
/// `Capabilities`, whose page defines `ST` as `ESC \` alone.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ends {
    AsQuery,
    St,
}

/// Every string query baud answers, each with what it needs said first: a
/// colour set before it is asked, a cell size, a font.
const QUERIES: &[(&str, &[u8], &[u8], Ends)] = &[
    (
        "OSC 4",
        b"\x1b]4;1;rgb:11/22/33\x1b\\",
        b"4;1;?",
        Ends::AsQuery,
    ),
    (
        "OSC 5",
        b"\x1b]5;0;rgb:11/22/33\x1b\\",
        b"5;0;?",
        Ends::AsQuery,
    ),
    (
        "OSC 10",
        b"\x1b]10;rgb:11/22/33\x1b\\",
        b"10;?",
        Ends::AsQuery,
    ),
    ("OSC 50", b"", b"50;?", Ends::AsQuery),
    ("OSC 60", b"", b"60", Ends::AsQuery),
    (
        "OSC 21",
        b"",
        b"21;foreground=#112233;foreground=?",
        Ends::St,
    ),
    (
        "OSC 1337 ReportVariable",
        b"",
        b"1337;ReportVariable=dXNlcg==",
        Ends::AsQuery,
    ),
    ("OSC 1337 Capabilities", b"", b"1337;Capabilities", Ends::St),
    (
        "OSC 1337 ReportCellSize",
        b"",
        b"1337;ReportCellSize",
        Ends::AsQuery,
    ),
];

/// The reply `query`, after `before`, gets in the replies `c1` names, ended
/// by `end`.
fn reply(c1: &[u8], before: &[u8], query: &[u8], end: &[u8]) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.set_cell_size(ImagePx::of(17), ImagePx::of(34));
    t.set_backing_scale(BackingScale::of(2.0));
    t.set_font(b"JetBrains Mono", FontSize::Configured);
    support::feed(&mut t, c1);
    support::feed(&mut t, before);
    t.clear_reply();
    support::feed(&mut t, &[b"\x1b]".as_slice(), query, end].concat());
    t.reply().to_vec()
}

fn shown(b: &[u8]) -> String {
    b.escape_ascii().to_string()
}

/// **Each OSC answer after S8C1T opens with 9D and ends as the query
/// ended**, 9C for `ST` and `BEL` for `BEL` (or 9C always, [`Ends::St`]),
/// with no 7-bit `ESC ]` or `ESC \` in it. The control: each answers in 7-bit without S8C1T, so the
/// query is one baud answers.
#[test]
fn every_osc_answer_after_s8c1t_is_framed_in_c1() {
    let mut wrong = Vec::new();
    for (name, before, query, ends) in QUERIES {
        for (end, close) in [(&b"\x1b\\"[..], &b"\x9c"[..]), (b"\x07", b"\x07")] {
            let close = match ends {
                Ends::AsQuery => close,
                Ends::St => b"\x9c",
            };
            let seven = reply(b"", before, query, end);
            assert!(
                seven.starts_with(b"\x1b]"),
                "the control: {name} answers in 7-bit: {}",
                shown(&seven)
            );
            let eight = reply(S8C1T, before, query, end);
            let framed = eight.starts_with(b"\x9d")
                && eight.ends_with(close)
                && !eight.windows(2).any(|w| w == b"\x1b]" || w == b"\x1b\\");
            if !framed {
                wrong.push(format!("{name} ended {}: {}", shown(end), shown(&eight)));
            }
        }
    }
    assert!(wrong.is_empty(), "not framed in C1:\n{}", wrong.join("\n"));
}

/// **XTVERSION after S8C1T is `DCS > | text ST` in C1**: 90 ... 9C. The
/// control: `ESC P` ... `ESC \` without it.
#[test]
fn xtversion_after_s8c1t_is_framed_in_c1() {
    let ask = |c1: &[u8]| {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        support::feed(&mut t, c1);
        support::feed(&mut t, b"\x1b[>q");
        t.reply().to_vec()
    };
    let seven = ask(b"");
    assert!(
        seven.starts_with(b"\x1bP>|") && seven.ends_with(b"\x1b\\"),
        "the control: {}",
        shown(&seven)
    );
    let eight = ask(S8C1T);
    assert!(
        eight.starts_with(b"\x90>|") && eight.ends_with(b"\x9c") && !eight.contains(&0x1b),
        "XTVERSION after S8C1T: {}",
        shown(&eight)
    );
}

/// **A kitty graphics reply after S8C1T is `APC G ... ST` in C1**: 9F ...
/// 9C, as S8C1T governs every C1 the terminal sends, APC among them. The
/// control: `ESC _ G` ... `ESC \` without it.
#[test]
fn a_kitty_reply_after_s8c1t_is_framed_in_c1() {
    let ask = |c1: &[u8]| {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        support::feed(&mut t, c1);
        support::feed(&mut t, b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\");
        t.reply().to_vec()
    };
    let seven = ask(b"");
    assert!(
        seven.starts_with(b"\x1b_Gi=31;") && seven.ends_with(b"\x1b\\"),
        "the control: {}",
        shown(&seven)
    );
    let eight = ask(S8C1T);
    assert!(
        eight.starts_with(b"\x9fGi=31;") && eight.ends_with(b"\x9c") && !eight.contains(&0x1b),
        "a kitty reply after S8C1T: {}",
        shown(&eight)
    );
}

/// **A notification answer after S8C1T is `OSC 99 ... ST` in C1**: 9D ...
/// 9C. The control: `ESC ]` ... `ESC \` without it.
#[test]
fn a_notification_answer_after_s8c1t_is_framed_in_c1() {
    let ask = |c1: &[u8]| {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws).with_notices(NoticeBudget::with_icon(4096));
        let mut t = Term::new(s.storage(), &ws);
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
        support::feed(&mut t, c1);
        support::feed(&mut t, b"\x1b]99;i=1:p=?;\x1b\\");
        t.reply().to_vec()
    };
    let seven = ask(b"");
    assert!(
        seven.starts_with(b"\x1b]99;i=1:p=?;") && seven.ends_with(b"\x1b\\"),
        "the control: {}",
        shown(&seven)
    );
    let eight = ask(S8C1T);
    assert!(
        eight.starts_with(b"\x9d99;i=1:p=?;") && eight.ends_with(b"\x9c") && !eight.contains(&0x1b),
        "an OSC 99 answer after S8C1T: {}",
        shown(&eight)
    );
}
