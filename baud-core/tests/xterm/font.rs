//! `OSC 50 ; Pt`, "Set Font to Pt", and its `?` query (`[[source: ctlseqs]]`).
//!
//! The core answers the query from what the embedder said it draws in
//! (`Term::set_font`) and queues each change for the embedder
//! (`Term::font_requests`). Entry 0 is the configured size, `#+n` and `#-n`
//! steps of the size, entries 1-7 none; a query of a relative step names no
//! numbered entry and gets xterm's empty reply (`QueryFontRequest`,
//! `[[source: xterm-misc]]` 3798-3846).

use crate::support;
use baud_core::*;

/// **`OSC 50` is acted on, not counted as unsupported.** The bytes alone,
/// so the test reads the same against a core that counted it.
#[test]
fn osc_50_is_acted_on_not_counted() {
    support::with_corpus(|t| {
        let before = t.unsupported;
        support::feed(t, b"\x1b]50;Menlo\x07\x1b]50;#+1\x1b\\\x1b]50;?\x07");
        assert_eq!(t.unsupported, before);
    });
}

fn with_font(size: FontSize, f: impl FnOnce(&mut Term<'_>)) {
    support::with_corpus(|t| {
        t.set_font(b"JetBrains Mono", size);
        f(t)
    })
}

fn reply(t: &mut Term<'_>, bytes: &[u8]) -> Vec<u8> {
    support::feed(t, bytes);
    let r = t.reply().to_vec();
    t.clear_reply();
    r
}

/// **The query names the family the embedder draws in**, with the query's
/// terminator, `#0 ` before it when anything follows the `?` or the sign, and
/// `OSC 50` alone, no `;`, where the expression names no entry: 1-7, or a
/// relative step.
#[test]
fn the_query_names_the_font_in_use_in_xterms_shape() {
    with_font(FontSize::Configured, |t| {
        let cases: &[(&[u8], &[u8])] = &[
            (b"\x1b]50;?\x07", b"\x1b]50;JetBrains Mono\x07"),
            (b"\x1b]50;?\x1b\\", b"\x1b]50;JetBrains Mono\x1b\\"),
            (b"\x1b]50;?0\x07", b"\x1b]50;#0 JetBrains Mono\x07"),
            (b"\x1b]50;?+\x07", b"\x1b]50;JetBrains Mono\x07"),
            (b"\x1b]50;?+0\x07", b"\x1b]50;#0 JetBrains Mono\x07"),
            (b"\x1b]50;?1\x07", b"\x1b]50\x07"),
            (b"\x1b]50;?7\x07", b"\x1b]50\x07"),
            (b"\x1b]50;?8\x07", b"\x1b]50\x07"),
            (b"\x1b]50;?+1\x07", b"\x1b]50\x07"),
            (b"\x1b]50;?-2\x1b\\", b"\x1b]50\x1b\\"),
        ];
        for (q, want) in cases {
            assert_eq!(reply(t, q), *want, "{q:?}");
        }
        assert!(t.font_requests().is_empty(), "a query changes nothing");
    });
}

/// **At a size a step took it to, the current font has no entry**: a
/// query that writes a number for it is answered empty, one that does not
/// with the name; entry 0 is still the configured size.
#[test]
fn a_stepped_font_has_no_numbered_entry() {
    with_font(FontSize::Stepped, |t| {
        assert_eq!(reply(t, b"\x1b]50;?\x07"), b"\x1b]50;JetBrains Mono\x07");
        assert_eq!(reply(t, b"\x1b]50;?+0\x07"), b"\x1b]50\x07");
        assert_eq!(reply(t, b"\x1b]50;?-\x07"), b"\x1b]50;JetBrains Mono\x07");
        assert_eq!(
            reply(t, b"\x1b]50;?0\x07"),
            b"\x1b]50;#0 JetBrains Mono\x07"
        );
    });
}

/// **A core no embedder told is silent**, as an `OSC 10` query is with no
/// colour: naming a font it does not draw in would be the wrong answer.
#[test]
fn an_untold_core_does_not_answer() {
    support::with_corpus(|t| {
        assert_eq!(reply(t, b"\x1b]50;?\x07"), b"");
    });
}

/// **A change is queued for the embedder**: a family by name, a step of
/// the size, entry 0's configured size, or a step and a name together.
#[test]
fn a_change_is_queued_for_the_embedder() {
    with_font(FontSize::Configured, |t| {
        let refused = t.fonts_refused();
        support::feed(
            t,
            b"\x1b]50; Menlo \x07\x1b]50;#+2\x07\x1b]50;#-1 STIX Two Math\x1b\\\x1b]50;#\x07",
        );
        let got: Vec<_> = t
            .font_requests()
            .iter()
            .map(|c| (c.size, c.name().map(<[u8]>::to_vec)))
            .collect();
        assert_eq!(
            got,
            [
                (SizeStep::Keep, Some(b"Menlo".to_vec())),
                (SizeStep::By(2), None),
                (SizeStep::By(-1), Some(b"STIX Two Math".to_vec())),
                (SizeStep::Configured, None),
            ]
        );
        assert_eq!(t.fonts_refused(), refused);
        assert_eq!(reply(t, b""), b"", "a change has no reply");
        t.clear_font_requests();
        assert!(t.font_requests().is_empty());
    });
}

/// **A change that names no entry, changes nothing or cannot be a family is
/// refused and counted**, and so is one past a full queue: entries 1-7, an
/// empty `Pt` (xterm rings its bell), the current entry with no name, a
/// name longer than any family.
#[test]
fn a_change_that_cannot_be_made_is_refused_and_counted() {
    with_font(FontSize::Configured, |t| {
        let long = [b'x'; FONT_NAME_MAX + 1];
        let mut bad = Vec::new();
        for pt in [
            &b"#3 Menlo"[..],
            b"#8",
            b"",
            b"#+",
            b"#-0",
            b"   ",
            &long[..],
        ] {
            bad.extend_from_slice(b"\x1b]50;");
            bad.extend_from_slice(pt);
            bad.push(0x07);
        }
        bad.extend_from_slice(b"\x1b]50\x07");
        let mut seen = t.fonts_refused();
        for one in bad.split_inclusive(|&b| b == 0x07) {
            support::feed(t, one);
            assert_ne!(t.fonts_refused(), seen, "each counted: {one:?}");
            seen = t.fonts_refused();
        }
        assert!(t.font_requests().is_empty(), "and none queued");
        for _ in 0..FONT_QUEUE {
            support::feed(t, b"\x1b]50;#+1\x07");
        }
        let full = t.fonts_refused();
        support::feed(t, b"\x1b]50;#+1\x07");
        assert_eq!(t.font_requests().len(), FONT_QUEUE);
        assert_ne!(t.fonts_refused(), full, "past a full queue");
    });
}

/// **A change the embedder could not make is counted with the core's**:
/// the core queues a family it cannot check, and the embedder that has no
/// such face says so, so the change is counted, not lost.
#[test]
fn a_change_the_embedder_refuses_is_counted() {
    with_font(FontSize::Configured, |t| {
        let before = t.fonts_refused();
        support::feed(t, b"\x1b]50;Nowhere Mono\x07");
        assert_eq!(t.font_requests().len(), 1, "queued");
        assert_eq!(t.fonts_refused(), before, "not refused by the core");
        t.clear_font_requests();
        t.refuse_font_request();
        assert_ne!(t.fonts_refused(), before, "refused by the embedder");
    });
}

/// **With changes off, every change is refused and the query is still
/// answered**: the owner's option D, "a setting turns changes off".
#[test]
fn with_changes_off_a_change_is_refused_and_the_query_answered() {
    with_font(FontSize::Configured, |t| {
        t.set_font_changes(FontChanges::Off);
        let before = t.fonts_refused();
        support::feed(t, b"\x1b]50;Menlo\x07");
        assert!(t.font_requests().is_empty());
        assert_ne!(t.fonts_refused(), before);
        assert_eq!(reply(t, b"\x1b]50;?\x07"), b"\x1b]50;JetBrains Mono\x07");
        support::feed(t, b"\x1bc");
        assert_eq!(t.font_changes(), FontChanges::Off, "RIS keeps the setting");
        assert_eq!(
            reply(t, b"\x1b]50;?\x07"),
            b"\x1b]50;JetBrains Mono\x07",
            "and the font the embedder told"
        );
    });
}

/// **`OSC 60` and `61` say what `OSC 50` does here**: `allowFontOps` is
/// allowed whole once the embedder has told the font with changes on;
/// with them off `SetFont` is disallowed, and with no font told both.
#[test]
fn the_feature_queries_say_what_osc_50_does() {
    with_font(FontSize::Configured, |t| {
        assert_eq!(
            reply(t, b"\x1b]60\x07"),
            b"\x1b]60;allowFontOps,allowMouseOps,allowTitleOps\x07"
        );
        assert_eq!(reply(t, b"\x1b]61;allowFontOps\x07"), b"\x1b]61\x07");
        t.set_font_changes(FontChanges::Off);
        assert_eq!(
            reply(t, b"\x1b]60\x07"),
            b"\x1b]60;allowMouseOps,allowTitleOps\x07"
        );
        assert_eq!(
            reply(t, b"\x1b]61;allowFontOps\x07"),
            b"\x1b]61;SetFont\x07"
        );
    });
    support::with_corpus(|t| {
        assert_eq!(
            reply(t, b"\x1b]61;allowFontOps\x07"),
            b"\x1b]61;SetFont,GetFont\x07",
            "no font told"
        );
    });
}

/// **A query after a change in the same read names the font from before
/// it**: the embedder performs the change after the read, where xterm
/// changes the font before it parses the query. Pinned as the departure it
/// is.
#[test]
fn a_query_after_a_change_in_one_read_names_the_font_before_it() {
    with_font(FontSize::Configured, |t| {
        let r = reply(t, b"\x1b]50;Menlo\x07\x1b]50;?\x07");
        assert_eq!(r, b"\x1b]50;JetBrains Mono\x07");
        assert_eq!(t.font_requests().len(), 1);
    });
}

/// **The same at every split into two reads.**
#[test]
fn osc_50_is_the_same_at_every_split() {
    let bytes = b"\x1b]50;#-1 Menlo\x1b\\\x1b]50;?0\x07";
    for split in 0..=bytes.len() {
        with_font(FontSize::Configured, |t| {
            support::feed(t, &bytes[..split]);
            support::feed(t, &bytes[split..]);
            assert_eq!(t.reply(), b"\x1b]50;#0 JetBrains Mono\x07", "split {split}");
            let c = t.font_requests();
            assert_eq!(c.len(), 1, "split {split}");
            assert_eq!(c[0].size, SizeStep::By(-1));
            assert_eq!(c[0].name(), Some(&b"Menlo"[..]));
        });
    }
}
