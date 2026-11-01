//! Notifications and progress from programs: `OSC 9`, `OSC 777 ; notify`,
//! kitty's `OSC 99` and ConEmu's `OSC 9 ; 4`, and `OSC 1337 ;
//! RequestAttention` (`docs/research/notes/ax-program-notifications.md`).
//!
//! `[[source: kitty-desktop-notifications]]` is the protocol with the most
//! to get wrong: chunks, ids, the query and its answer, the reports. Every
//! answer is pinned byte for byte, because a program parses whatever
//! arrives as the report it asked for.

use baud_core::*;
use baud_domain::{ItermKey, Messages};

mod support;

/// What a desktop that shows everything supports: both actions, close
/// events, every occasion, urgency, the standard sounds, expiry and icons.
const ALL: NoticeSupport = NoticeSupport {
    actions: Activation::FOCUS,
    close_events: CloseEvents::Tracked,
    occasions: Occasions::UnfocusedAndInvisible,
    urgencies: Urgencies::All,
    sounds: Sounds::Standard,
    expiry: ExpirySupport::Expires,
    icons: IconSupport::NamesAndImages,
    buttons: ButtonSupport::Shown,
};

fn all() -> NoticeSupport {
    NoticeSupport {
        actions: Activation::FOCUS,
        ..ALL
    }
}

/// Reporting too: `a=focus,report`.
fn reporting() -> NoticeSupport {
    let mut s = all();
    s.actions = activation(b"focus,report");
    s
}

/// The activation `a` names, from kitty's default of `focus`, read the way
/// a program's notification reads it.
fn activation(a: &[u8]) -> Activation {
    with(
        Some(all()),
        &[b"\x1b]99;a=", a, b";x\x1b\\"].concat(),
        Reads(4096),
        |t| t.notices().next().expect("published").activation,
    )
}

/// How many bytes each read hands the terminal.
#[derive(Clone, Copy)]
struct Reads(usize);

/// A term with a notifications area and `support` told, fed `bytes` in
/// reads of `chunk`.
fn with<R>(
    support: Option<NoticeSupport>,
    bytes: &[u8],
    chunk: Reads,
    f: impl FnOnce(&mut Term<'_>) -> R,
) -> R {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_notices(NoticeBudget::with_icon(4096));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_notice_support(support);
    feed_in(&mut t, bytes, chunk);
    f(&mut t)
}

fn feed_in(t: &mut Term<'_>, bytes: &[u8], chunk: Reads) {
    let mut at = 0;
    while at < bytes.len() {
        let n = chunk.0.min(bytes.len() - at);
        t.write_at(BytePos::ZERO, &bytes[at..at + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        at += n;
    }
}

/// What a notice says, owned, for comparing.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Seen {
    from: NoticeFrom,
    action: NoticeAction,
    title: Vec<u8>,
    body: Vec<u8>,
    id: Vec<u8>,
    buttons: Vec<Vec<u8>>,
    occasion: Occasion,
    urgency: Urgency,
}

fn seen(t: &Term<'_>) -> Vec<Seen> {
    t.notices()
        .map(|n| Seen {
            from: n.from,
            action: n.action,
            title: n.title.to_vec(),
            body: n.body.to_vec(),
            id: n.id.to_vec(),
            buttons: n.buttons().map(<[u8]>::to_vec).collect(),
            occasion: n.occasion,
            urgency: n.urgency,
        })
        .collect()
}

fn b64(data: &[u8]) -> Vec<u8> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for c in data.chunks(3) {
        let w = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            match i <= c.len() {
                true => out.push(A[usize::try_from((w >> (18 - 6 * i)) & 63).expect("six bits")]),
                false => out.push(b'='),
            }
        }
    }
    out
}

// ── OSC 9 and OSC 777 ───────────────────────────────────────────────────────

/// iTerm2's "Post a notification": `OSC 9 ; message ST`, either terminator,
/// shown while the window is not focused.
#[test]
fn osc_9_posts_its_message() {
    for end in [&b"\x07"[..], b"\x1b\\"] {
        with(
            Some(all()),
            &[b"\x1b]9;Build finished".as_slice(), end].concat(),
            Reads(4096),
            |t| {
                let s = seen(t);
                assert_eq!(s.len(), 1);
                assert_eq!(s[0].from, NoticeFrom::Osc9);
                assert_eq!(s[0].title, b"Build finished");
                assert_eq!(s[0].body, b"");
                assert_eq!(s[0].occasion, Occasion::Unfocused);
            },
        );
    }
}

/// ConEmu's other forms are ConEmu's: refused and counted, never posted as
/// text -- a shell that sends `9 ; 9` at every prompt posts nothing.
#[test]
fn conemus_other_forms_post_nothing() {
    for body in [
        &b"9;C:/dir"[..],
        b"2;box",
        b"7;cmd",
        b"8;PATH",
        b"12",
        b"1;100",
        b"3;tab",
        b"5",
        b"11;x",
        b"10;1",
        b"6;m",
    ] {
        with(
            Some(all()),
            &[b"\x1b]9;", body, b"\x07"].concat(),
            Reads(4096),
            |t| {
                assert_eq!(t.notices().count(), 0, "{}", String::from_utf8_lossy(body));
                assert_eq!(t.strings_ignored, 1);
            },
        );
    }
}

/// `OSC 777 ; notify ; title ; body`: the body is everything after the
/// title's `;`, and a title alone is a notification.
#[test]
fn osc_777_notify_takes_a_title_and_the_rest() {
    with(
        Some(all()),
        b"\x1b]777;notify;Command completed;make; make install\x1b\\",
        Reads(4096),
        |t| {
            let s = seen(t);
            assert_eq!(s[0].from, NoticeFrom::Osc777);
            assert_eq!(s[0].title, b"Command completed");
            assert_eq!(s[0].body, b"make; make install");
        },
    );
    with(
        Some(all()),
        b"\x1b]777;notify;Only a title\x07",
        Reads(4096),
        |t| {
            assert_eq!(seen(t)[0].title, b"Only a title");
        },
    );
    with(
        Some(all()),
        b"\x1b]777;precmd\x07\x1b]777;notify\x07",
        Reads(4096),
        |t| {
            assert_eq!(t.notices().count(), 0);
            assert_eq!(t.strings_ignored, 2);
        },
    );
}

// ── what refuses ────────────────────────────────────────────────────────────

/// A storage with no area, and an embedder that has said nothing (or has
/// notifications off), refuse every notification and count it, and leave
/// the query unanswered.
#[test]
fn no_area_or_no_support_refuses_and_stays_silent() {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    t.set_notice_support(Some(all()));
    support::feed(&mut t, b"\x1b]9;hi\x07\x1b]99;i=1:p=?;\x1b\\");
    assert_eq!(t.notices().count(), 0);
    assert_eq!(t.notice_refusals().unprovisioned, 2);
    assert!(t.reply().is_empty());

    with(
        None,
        b"\x1b]9;hi\x07\x1b]99;;hello\x1b\\\x1b]99;i=1:p=?;\x1b\\",
        Reads(4096),
        |t| {
            assert_eq!(t.notices().count(), 0);
            assert_eq!(t.notice_refusals().off, 3);
            assert!(t.reply().is_empty(), "no answer is the claim of no support");
        },
    );
}

// ── progress ────────────────────────────────────────────────────────────────

/// ConEmu's progress, every state, and iTerm2's bare `9 ; 4` that clears.
#[test]
fn progress_follows_every_state() {
    let cases: &[(&[u8], ProgressState, Option<u8>)] = &[
        (b"4;1;50", ProgressState::Normal, Some(50)),
        (b"4;1;150", ProgressState::Normal, Some(100)),
        (b"4;2", ProgressState::Error, None),
        (b"4;2;75", ProgressState::Error, Some(75)),
        (b"4;3", ProgressState::Indeterminate, None),
        (b"4;4;25", ProgressState::Paused, Some(25)),
        (b"4;0", ProgressState::Hidden, None),
        (b"4", ProgressState::Hidden, None),
    ];
    for (body, state, pct) in cases {
        // From a bar already shown, so a clear is seen to clear it.
        with(
            Some(all()),
            &[b"\x1b]9;4;1;20\x07\x1b]9;".as_slice(), body, b"\x07"].concat(),
            Reads(4096),
            |t| {
                let p = t.progress();
                assert_eq!(p.state, *state, "{}", String::from_utf8_lossy(body));
                assert_eq!(p.percent.map(Percent::get), *pct);
            },
        );
    }
}

/// A pause with no percentage keeps the one shown (iTerm2 3.7); a state no
/// text defines changes nothing and is counted; `RIS` hides the bar.
#[test]
fn a_pause_keeps_the_percentage_and_ris_hides_the_bar() {
    with(
        Some(all()),
        b"\x1b]9;4;1;40\x07\x1b]9;4;4\x07",
        Reads(4096),
        |t| {
            assert_eq!(t.progress().state, ProgressState::Paused);
            assert_eq!(t.progress().percent.map(Percent::get), Some(40));
            let before = t.progress_reports();
            support::feed(t, b"\x1b]9;4;9\x07\x1b]9;4;1;x\x07");
            assert_eq!(t.progress_reports(), before);
            assert_eq!(t.strings_ignored, 2);
            support::feed(t, b"\x1bc");
            assert_eq!(t.progress(), Progress::HIDDEN);
            assert_ne!(t.progress_reports(), before);
        },
    );
}

/// Progress needs no notifications: it is shown with the setting off too.
#[test]
fn progress_does_not_wait_on_support() {
    with(None, b"\x1b]9;4;1;10\x07", Reads(4096), |t| {
        assert_eq!(t.progress().percent.map(Percent::get), Some(10));
    });
}

// ── kitty's OSC 99 ──────────────────────────────────────────────────────────

/// The text's first example, and its title-and-body pair.
#[test]
fn kittys_examples_post_what_they_say() {
    with(
        Some(all()),
        b"\x1b]99;;Hello world\x1b\\",
        Reads(4096),
        |t| {
            let s = seen(t);
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].from, NoticeFrom::Kitty);
            assert_eq!(s[0].title, b"Hello world");
            assert_eq!(s[0].occasion, Occasion::Always, "o's default is always");
        },
    );
    with(
        Some(all()),
        b"\x1b]99;i=1:d=0;Hello world\x1b\\\x1b]99;i=1:p=body;This is cool\x1b\\",
        Reads(4096),
        |t| {
            let s = seen(t);
            assert_eq!(s.len(), 1, "d=0 holds the first chunk");
            assert_eq!(s[0].title, b"Hello world");
            assert_eq!(s[0].body, b"This is cool");
            assert_eq!(s[0].id, b"1");
        },
    );
}

/// "If a notification has no title, the body will be used as title. A
/// notification with not title and no body is ignored."
#[test]
fn the_body_stands_in_for_a_missing_title() {
    with(
        Some(all()),
        b"\x1b]99;p=body;only a body\x1b\\\x1b]99;;\x1b\\",
        Reads(4096),
        |t| {
            let s = seen(t);
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].title, b"only a body");
            assert_eq!(t.notice_refusals().empty, 1);
        },
    );
}

/// A chunk of 4096 encoded bytes, eight times what `pending` holds, arrives
/// whole; one past what a chunk may carry is refused, not cut.
#[test]
fn a_chunk_carries_four_kilobytes() {
    let body = vec![b'a'; 3000];
    let enc = b64(&body);
    assert!(enc.len() == 4000);
    let seq = [b"\x1b]99;e=1:p=body;".as_slice(), &enc, b"\x1b\\"].concat();
    with(Some(all()), &seq, Reads(4096), |t| {
        let s = seen(t);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title.len(), 1024, "a text is kept to 1024 bytes");
    });
    let plain = vec![b'b'; 4097];
    let seq = [b"\x1b]99;;".as_slice(), &plain, b"\x1b\\"].concat();
    with(Some(all()), &seq, Reads(4096), |t| {
        assert_eq!(t.notices().count(), 0);
        assert_eq!(t.notice_refusals().too_long, 1);
    });
}

/// Base64 chunked after encoding: a quantum straddles two chunks, and the
/// last is unpadded; and chunked before, each padded.
#[test]
fn base64_spans_chunks_either_way() {
    let text = b"The quick brown fox";
    let enc = b64(text);
    let (a, b) = enc.split_at(5);
    let after = [
        b"\x1b]99;i=x:d=0:e=1;".as_slice(),
        a,
        b"\x1b\\\x1b]99;i=x:e=1;",
        {
            let mut e = b;
            while let Some(x) = e.strip_suffix(b"=") {
                e = x;
            }
            e
        },
        b"\x1b\\",
    ]
    .concat();
    with(Some(all()), &after, Reads(4096), |t| {
        assert_eq!(seen(t)[0].title, text)
    });
    let before = [
        b"\x1b]99;i=y:d=0:e=1;".as_slice(),
        &b64(b"The quick "),
        b"\x1b\\\x1b]99;i=y:e=1;",
        &b64(b"brown fox"),
        b"\x1b\\",
    ]
    .concat();
    with(Some(all()), &before, Reads(4096), |t| {
        assert_eq!(seen(t)[0].title, text)
    });
}

/// Fed a byte at a time, every sequence here publishes what it publishes
/// whole: the chunk outlives the reads.
#[test]
fn every_read_size_publishes_the_same() {
    let seq = [
        b"\x1b]99;i=1:d=0:e=1;".as_slice(),
        &b64("Héllo wörld".as_bytes()),
        b"\x1b\\\x1b]99;i=1:p=body;a body\x1b\\\x1b]9;nine\x07\x1b]777;notify;t;b\x1b\\",
    ]
    .concat();
    let whole = with(Some(all()), &seq, Reads(4096), |t| seen(t));
    assert_eq!(whole.len(), 3);
    for chunk in [1, 2, 3, 7] {
        assert_eq!(
            with(Some(all()), &seq, Reads(chunk), |t| seen(t)),
            whole,
            "reads of {chunk}"
        );
    }
}

/// A chunk cancelled by `CAN` reaches no terminator, so it changes nothing:
/// the notification it would have finished is still open, and the next
/// chunk finishes it.
#[test]
fn a_cancelled_chunk_changes_nothing() {
    with(
        Some(all()),
        b"\x1b]99;i=1:d=0;Title\x1b\\\x1b]99;i=1:p=body;lost\x18\x1b]99;i=1:p=body;kept\x1b\\",
        Reads(4096),
        |t| {
            let s = seen(t);
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].body, b"kept");
        },
    );
}

/// A chunk of another id while one is open abandons it, counted, as kitty's
/// one pending notification is replaced.
#[test]
fn another_id_abandons_the_open_one() {
    with(
        Some(all()),
        b"\x1b]99;i=1:d=0;first\x1b\\\x1b]99;i=2;second\x1b\\",
        Reads(4096),
        |t| {
            let s = seen(t);
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].title, b"second");
            assert_eq!(t.notice_refusals().abandoned, 1);
        },
    );
}

/// The same id again updates the notification: the same token, `Replace`.
/// With no id, "no updating must take place".
#[test]
fn the_same_id_updates_and_no_id_never_does() {
    with(
        Some(all()),
        b"\x1b]99;i=job;50%\x1b\\\x1b]99;i=job;90%\x1b\\",
        Reads(4096),
        |t| {
            let n: Vec<_> = t.notices().map(|n| (n.token, n.action)).collect();
            assert_eq!(n[0].1, NoticeAction::Show);
            assert_eq!(n[1], (n[0].0, NoticeAction::Replace));
        },
    );
    with(
        Some(all()),
        b"\x1b]99;;a\x1b\\\x1b]99;;b\x1b\\",
        Reads(4096),
        |t| {
            let n: Vec<_> = t.notices().map(|n| (n.token, n.action)).collect();
            assert_ne!(n[0].0, n[1].0);
            assert_eq!(n[1].1, NoticeAction::Show);
        },
    );
}

/// "To close a previous notification, send ... p=close": a close entry for
/// the live one; an unknown id or none is a no-op.
#[test]
fn p_close_closes_a_live_notification() {
    with(
        Some(all()),
        b"\x1b]99;i=a;hello\x1b\\\x1b]99;i=a:p=close;\x1b\\\x1b]99;i=zz:p=close;\x1b\\\x1b]99;p=close;\x1b\\",
        Reads(4096),
        |t| {
            let n: Vec<_> = t.notices().map(|n| (n.token, n.action)).collect();
            assert_eq!(n.len(), 2);
            assert_eq!(n[1], (n[0].0, NoticeAction::Close));
        },
    );
}

/// Buttons, separated by U+2028; urgency, occasion, the keys a program
/// sends.
#[test]
fn buttons_and_keys_reach_the_embedder() {
    with(
        Some(all()),
        "\x1b]99;i=b:d=0:u=2:o=unfocused;Pick\x1b\\\x1b]99;i=b:p=buttons;Yes\u{2028}No\x1b\\"
            .as_bytes(),
        Reads(4096),
        |t| {
            let s = seen(t);
            assert_eq!(s[0].buttons, vec![b"Yes".to_vec(), b"No".to_vec()]);
            assert_eq!(s[0].urgency, Urgency::Critical);
            assert_eq!(s[0].occasion, Occasion::Unfocused);
        },
    );
}

/// An icon by name, an icon key, and an image: what an embedder needs to
/// show one.
#[test]
fn icons_reach_the_embedder() {
    let png = [0x89u8, b'P', b'N', b'G', 1, 2, 3, 4, 5];
    let seq = [
        b"\x1b]99;i=ic:d=0:n=".as_slice(),
        &b64(b"error"),
        b":n=",
        &b64(b"warn"),
        b":g=icon-1;Title\x1b\\\x1b]99;i=ic:p=icon:e=1;",
        &b64(&png),
        b"\x1b\\",
    ]
    .concat();
    with(Some(all()), &seq, Reads(4096), |t| {
        let n = t.notices().next().expect("published");
        assert_eq!(
            n.icon_names().collect::<Vec<_>>(),
            vec![&b"error"[..], b"warn"]
        );
        assert_eq!(n.icon_key, b"icon-1");
        assert_eq!(n.icon, png);
    });
}

// ── the answers ─────────────────────────────────────────────────────────────

/// `p=?` answered with exactly what the embedder delivers, in order with
/// the read's other replies: the query, then DA1, as the text's detection
/// sends them.
#[test]
fn the_query_answers_what_the_embedder_delivers() {
    with(
        Some(reporting()),
        b"\x1b]99;i=q1:p=?;\x1b\\\x1b[c",
        Reads(4096),
        |t| {
            let r = String::from_utf8_lossy(t.reply()).into_owned();
            let want = "\x1b]99;i=q1:p=?;a=focus,report:c=1:o=always,unfocused,invisible:\
                    p=title,body,close,icon,?,alive,buttons:\
                    s=system,silent,error,warn,warning,info,question:u=0,1,2:w=1\x1b\\";
            assert!(r.starts_with(want), "{r:?}");
            assert!(r[want.len()..].starts_with("\x1b[?"), "DA1 after it: {r:?}");
        },
    );
    let macos = NoticeSupport {
        actions: Activation::FOCUS,
        close_events: CloseEvents::Untracked,
        occasions: Occasions::UnfocusedAndInvisible,
        urgencies: Urgencies::LowAndNormal,
        sounds: Sounds::SystemAndSilent,
        expiry: ExpirySupport::Expires,
        icons: IconSupport::NamesAndImages,
        buttons: ButtonSupport::Shown,
    };
    with(Some(macos), b"\x1b]99;p=?;\x1b\\", Reads(4096), |t| {
        assert_eq!(
            t.reply(),
            b"\x1b]99;i=0:p=?;a=focus:o=always,unfocused,invisible:p=title,body,close,icon,?,alive,buttons:\
              s=system,silent:u=0,1:w=1\x1b\\"
        );
    });
}

/// **`buttons` is claimed only where they are shown**: "supported payload
/// types", and a freedesktop server that does not list `actions` shows
/// none -- kitty's own reference leaves `buttons` out then
/// (`[[source: kitty-notifications-py]]`, its `payload_type_supported`).
#[test]
fn the_query_claims_buttons_only_where_they_are_shown() {
    let no_buttons = NoticeSupport {
        actions: Activation::NONE,
        close_events: CloseEvents::Tracked,
        occasions: Occasions::Unfocused,
        urgencies: Urgencies::All,
        sounds: Sounds::SystemAndSilent,
        expiry: ExpirySupport::Expires,
        icons: IconSupport::NamesAndImages,
        buttons: ButtonSupport::NotShown,
    };
    with(Some(no_buttons), b"\x1b]99;p=?;\x1b\\", Reads(4096), |t| {
        assert_eq!(
            t.reply(),
            b"\x1b]99;i=0:p=?;c=1:o=always,unfocused:p=title,body,close,icon,?,alive:\
              s=system,silent:u=0,1,2:w=1\x1b\\"
        );
    });
    let names_only = NoticeSupport {
        icons: IconSupport::NamesOnly,
        ..no_buttons
    };
    with(Some(names_only), b"\x1b]99;p=?;\x1b\\", Reads(4096), |t| {
        let r = String::from_utf8_lossy(t.reply()).into_owned();
        assert!(r.contains(":p=title,body,close,?,alive:"), "{r:?}");
    });
    let shown = NoticeSupport {
        buttons: ButtonSupport::Shown,
        ..names_only
    };
    with(Some(shown), b"\x1b]99;p=?;\x1b\\", Reads(4096), |t| {
        let r = String::from_utf8_lossy(t.reply()).into_owned();
        assert!(r.contains(":p=title,body,close,?,alive,buttons:"), "{r:?}");
    });
}

/// `p=alive` names the live ids; a closed one is gone from it.
#[test]
fn alive_lists_the_live_ids() {
    with(
        Some(all()),
        b"\x1b]99;i=a;one\x1b\\\x1b]99;i=b;two\x1b\\\x1b]99;;three\x1b\\",
        Reads(4096),
        |t| {
            let a = t.notices().next().expect("one").token;
            support::feed(t, b"\x1b]99;i=m:p=alive;\x1b\\");
            assert_eq!(t.reply(), b"\x1b]99;i=m:p=alive;a,b\x1b\\");
            t.clear_reply();
            t.notice_closed(a);
            support::feed(t, b"\x1b]99;i=m:p=alive;\x1b\\");
            assert_eq!(t.reply(), b"\x1b]99;i=m:p=alive;b\x1b\\");
        },
    );
}

/// A click reports only with `a=report`: `OSC 99 ; i=id ; ST`, a button's
/// number after the `;`, and `i=0` with no id.
#[test]
fn a_click_reports_only_when_asked() {
    with(
        Some(reporting()),
        b"\x1b]99;i=r:a=report;x\x1b\\\x1b]99;a=report;y\x1b\\\x1b]99;i=n;z\x1b\\",
        Reads(4096),
        |t| {
            let tok: Vec<_> = t.notices().map(|n| n.token).collect();
            assert!(t.notice_activated(tok[0], None));
            assert!(t.notice_activated(tok[0], NoticeButton::of_index(1)));
            assert!(t.notice_activated(tok[1], None));
            assert!(
                !t.notice_activated(tok[2], None),
                "must not send a response unless report is requested"
            );
            assert_eq!(
                t.reply(),
                b"\x1b]99;i=r;\x1b\\\x1b]99;i=r;2\x1b\\\x1b]99;i=0;\x1b\\"
            );
            assert!(t.notice_focuses(tok[0]) && t.notice_focuses(tok[2]));
        },
    );
}

/// `c=1`: a close reported where the platform tracks closes, and
/// `untracked` at once where it does not.
#[test]
fn closes_are_reported_tracked_or_untracked() {
    with(Some(all()), b"\x1b]99;i=c:c=1;x\x1b\\", Reads(4096), |t| {
        let tok = t.notices().next().expect("one").token;
        assert!(!t.notice_shown(tok));
        assert!(t.notice_closed(tok));
        assert_eq!(t.reply(), b"\x1b]99;i=c:p=close;\x1b\\");
    });
    let mut untracked = all();
    untracked.close_events = CloseEvents::Untracked;
    with(
        Some(untracked),
        b"\x1b]99;i=c:c=1;x\x1b\\\x1b]99;i=d;y\x1b\\",
        Reads(4096),
        |t| {
            let tok: Vec<_> = t.notices().map(|n| n.token).collect();
            assert!(t.notice_shown(tok[0]));
            assert!(!t.notice_shown(tok[1]), "no c=1, nothing said");
            assert_eq!(t.reply(), b"\x1b]99;i=c:p=close;untracked\x1b\\");
        },
    );
}

/// "Terminals **must** sanitize ids ... before sending them back": an id
/// spelling an escape sequence comes back without its bytes.
#[test]
fn ids_are_sanitised_before_they_are_echoed() {
    with(
        Some(reporting()),
        b"\x1b]99;i=a$(rm)b:a=report;x\x1b\\\x1b]99;i=k[1]2:p=?;\x1b\\",
        Reads(4096),
        |t| {
            let tok = t.notices().next().expect("one").token;
            t.notice_activated(tok, None);
            let r = t.reply();
            assert!(
                r.starts_with(b"\x1b]99;i=k12:p=?;"),
                "{:?}",
                String::from_utf8_lossy(r)
            );
            assert!(
                r.ends_with(b"\x1b]99;i=armb;\x1b\\"),
                "{:?}",
                String::from_utf8_lossy(r)
            );
        },
    );
}

/// A program cannot fill the queue past what one read publishes.
#[test]
fn a_read_publishes_at_most_its_queue() {
    let flood: Vec<u8> = (0..20)
        .flat_map(|i| format!("\x1b]9;n{i}\x07").into_bytes())
        .collect();
    with(Some(all()), &flood, Reads(4096), |t| {
        assert_eq!(t.notices().count(), 8);
        assert_eq!(t.notice_refusals().queue_full, 12);
        t.clear_notices();
        support::feed(t, b"\x1b]9;again\x07");
        assert_eq!(t.notices().count(), 1);
    });
}

// ── OSC 1337 ────────────────────────────────────────────────────────────────

/// `RequestAttention`: yes, once and no kept for the embedder, fireworks
/// too (`fireworks_reach_the_embedder_at_the_cursor`); every other
/// subcommand counted.
#[test]
fn request_attention_reaches_the_embedder() {
    with(
        Some(all()),
        b"\x1b]1337;RequestAttention=once\x07",
        Reads(4096),
        |t| {
            assert_eq!(t.attention(), Some(Attention::Once));
            let n = t.attention_requests();
            support::feed(t, b"\x1b]1337;RequestAttention=no\x07");
            assert_eq!(t.attention(), Some(Attention::No));
            assert_ne!(t.attention_requests(), n);
            support::feed(
                t,
                b"\x1b]1337;RequestAttention=fireworks\x07\x1b]1337;AddNote=eA==\x07",
            );
            assert_eq!(t.strings_ignored, 1, "the note alone");
        },
    );
}

/// **`RequestAttention=fireworks` reaches the embedder with the cursor's
/// cell**: "fireworks explode at the cursor's location"
/// (`[[source: iterm2-escape-codes]]`). At every read size the request is
/// counted for the embedder and not as ignored, under no key; the cell is
/// the cursor's as it arrived, not where the read left it; a second moves
/// the count and the cell; `yes` before it in the same read is kept for the
/// Dock; and `flash`, which iTerm2's source has and the page does not name,
/// and `maybe` stay counted under the key.
#[test]
fn fireworks_reach_the_embedder_at_the_cursor() {
    let body = b"\x1b[5;12Hab\x1b]1337;RequestAttention=fireworks\x07\r\n$ ";
    let none = with(Some(all()), b"", Reads(4096), |t| t.fireworks_requests());
    for n in [1, 2, 5, 4096] {
        with(Some(all()), body, Reads(n), |t| {
            assert_eq!(t.strings_ignored, 0, "reads of {n}: not ignored");
            assert_eq!(
                t.iterm_counts().of(ItermKey::RequestAttention),
                Messages::NONE,
                "reads of {n}: not counted under its key"
            );
            assert_eq!(
                t.fireworks(),
                Some((Row::from_index(4), Col::at_index(13))),
                "reads of {n}: the cursor's cell as it arrived"
            );
            assert_eq!(t.cursor(), (Row::from_index(5), Col::at_index(2)));
            let first = t.fireworks_requests();
            assert_ne!(first, none, "reads of {n}: counted");
            support::feed(t, b"\x1b]1337;RequestAttention=fireworks\x07");
            assert_ne!(t.fireworks_requests(), first, "a second moves it");
            assert_eq!(t.fireworks(), Some((Row::from_index(5), Col::at_index(2))));
        });
    }
    with(
        Some(all()),
        b"\x1b]1337;RequestAttention=yes\x07\x1b]1337;RequestAttention=fireworks\x07",
        Reads(4096),
        |t| {
            assert_eq!(t.attention(), Some(Attention::Yes), "the bounce kept");
            assert!(t.fireworks().is_some());
        },
    );
    with(
        Some(all()),
        b"\x1b]1337;RequestAttention=flash\x07\x1b]1337;RequestAttention=maybe\x07",
        Reads(4096),
        |t| {
            assert_eq!(t.fireworks(), None);
            assert_eq!(t.fireworks_requests(), none);
            assert_eq!(t.strings_ignored, 2);
            assert_eq!(
                t.iterm_counts().of(ItermKey::RequestAttention),
                Messages::of(2)
            );
        },
    );
}

/// **iTerm2's `Notification`, posted as an `OSC 9` notification is**: its
/// `title` and `message` decoded from base64, at every read size; a
/// `subtitle` and an `image` dropped and counted; a value that does not
/// decode is none; with the user's notifications off it is refused as an
/// `OSC 9` is.
#[test]
fn iterm2_notification_posts_its_title_and_message() {
    let mut body = b"\x1b]1337;Notification=title=".to_vec();
    body.extend(b64(b"Build"));
    body.extend(b";subtitle=");
    body.extend(b64(b"main"));
    body.extend(b";message=");
    body.extend(b64(b"finished in 3 s"));
    body.extend(b";image=");
    body.extend(b64(b"/tmp/x.png"));
    body.extend(b"\x07");
    for n in [1, 3, 7, 4096] {
        with(Some(all()), &body, Reads(n), |t| {
            let s = seen(t);
            assert_eq!(s.len(), 1, "reads of {n}");
            assert_eq!(s[0].from, NoticeFrom::Iterm);
            assert_eq!(s[0].title, b"Build", "reads of {n}");
            assert_eq!(s[0].body, b"finished in 3 s", "reads of {n}");
            assert_eq!(s[0].occasion, Occasion::Unfocused);
            assert_eq!(t.unsupported, 2, "the subtitle and the image");
        });
    }
    // A message alone stands as the title, as an `OSC 9`'s text does; a
    // value that is not base64 is none, and with nothing left the
    // notification is refused as empty.
    let alone = [
        b"\x1b]1337;Notification=message=".as_slice(),
        &b64(b"done"),
        b"\x07",
    ]
    .concat();
    with(Some(all()), &alone, Reads(4096), |t| {
        assert_eq!(seen(t)[0].title, b"done");
    });
    with(
        Some(all()),
        b"\x1b]1337;Notification=title=no!\x07",
        Reads(4096),
        |t| {
            assert_eq!(t.notices().count(), 0);
        },
    );
    with(None, &alone, Reads(4096), |t| {
        assert_eq!(t.notices().count(), 0, "notifications off");
    });
}

/// A notification the embedder did not show is forgotten: not alive, and
/// no report when it is closed.
#[test]
fn a_dropped_notification_is_forgotten() {
    with(Some(all()), b"\x1b]99;i=d:c=1;x\x1b\\", Reads(4096), |t| {
        let tok = t.notices().next().expect("one").token;
        t.notice_dropped(tok);
        assert!(!t.notice_closed(tok));
        support::feed(t, b"\x1b]99;i=m:p=alive;\x1b\\");
        assert_eq!(t.reply(), b"\x1b]99;i=m:p=alive;\x1b\\");
    });
}
