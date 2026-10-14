//! Slow and rapid blink: `SGR 5`, `SGR 6` and `SGR 25`.
//!
//! `[[source: ecma48]]` § 8.3.117: "5 slowly blinking (less then 150 per
//! minute)", "6 rapidly blinking (150 per minute or more)", "25 steady (not
//! blinking)", and "The usable combinations of parameter values are
//! determined by the implementation". The owner decided on 2026-10-13 that 6
//! is a state of its own, that 5 and 6 exclude each other with the later one
//! winning, and that 25 clears both. Until then 6 set 5's bit.
//!
//! Observed through cells and through `DECRQSS`, not through `Attrs`, so each
//! test here compiles against the commit before 6 had a bit and is red there.

use crate::support;
use baud_core::*;
use baud_domain::ParamIdx;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, Vec<u8>) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    (support::screen(&t), t.reply().to_vec())
}

fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    run(bytes).0
}

/// What `DECRQSS` answers for the rendition `set` leaves.
fn rendition(set: &[u8]) -> Vec<u8> {
    run(&[set, b"\x1bP$qm\x1b\\"].concat()).1
}

/// **6 is its own rendition**: not 5's, and not plain text.
#[test]
fn rapid_blink_is_its_own_rendition() {
    assert_ne!(
        grid(b"\x1b[5mx"),
        grid(b"x"),
        "the control: 5 is a rendition"
    );
    assert_ne!(grid(b"\x1b[6mx"), grid(b"x"), "6 is a rendition");
    assert_ne!(grid(b"\x1b[6mx"), grid(b"\x1b[5mx"), "6 is not 5");
}

/// **The later of 5 and 6 wins**, in one sequence or in two, and whatever
/// else the rendition holds: one aspect, two values.
#[test]
fn the_later_blink_rate_wins() {
    for (set, want, why) in [
        (
            &b"\x1b[5;6m"[..],
            &b"\x1bP1$r0;6m\x1b\\"[..],
            "5;6 is rapid",
        ),
        (b"\x1b[6;5m", b"\x1bP1$r0;5m\x1b\\", "6;5 is slow"),
        (
            b"\x1b[5m\x1b[6m",
            b"\x1bP1$r0;6m\x1b\\",
            "5 then 6 is rapid",
        ),
        (b"\x1b[6m\x1b[5m", b"\x1bP1$r0;5m\x1b\\", "6 then 5 is slow"),
        (
            b"\x1b[1;6m\x1b[5m",
            b"\x1bP1$r0;1;5m\x1b\\",
            "and bold stays",
        ),
    ] {
        assert_eq!(
            String::from_utf8_lossy(&rendition(set)),
            String::from_utf8_lossy(want),
            "{why}"
        );
    }
    assert_eq!(grid(b"\x1b[5;6mx"), grid(b"\x1b[6mx"));
    assert_eq!(grid(b"\x1b[6;5mx"), grid(b"\x1b[5mx"));
}

/// **25, "steady (not blinking)", clears both rates.**
#[test]
fn steady_clears_both_blink_rates() {
    for set in [
        &b"\x1b[5;25m"[..],
        b"\x1b[6;25m",
        b"\x1b[5m\x1b[6m\x1b[25m",
        b"\x1b[6m\x1b[25m",
    ] {
        assert_eq!(rendition(set), b"\x1bP1$r0m\x1b\\", "{set:?}");
        assert_eq!(grid(&[set, b"x"].concat()), grid(b"x"), "{set:?}");
    }
    assert_eq!(
        rendition(b"\x1b[6;0m"),
        b"\x1bP1$r0m\x1b\\",
        "and 0 does, as it clears every aspect"
    );
}

/// **DECRQSS answers `6` for rapid blink, and the answer round-trips**:
/// sent back, it reproduces the rapid rendition and not the slow one.
#[test]
fn decrqss_answers_6_for_rapid_blink_and_it_round_trips() {
    let reply = rendition(b"\x1b[6m");
    assert_eq!(reply, b"\x1bP1$r0;6m\x1b\\");
    let body = &reply[5..reply.len() - 2];
    let replay = [b"\x1b[", body, b"x"].concat();
    assert_eq!(grid(&replay), grid(b"\x1b[6mx"), "the reply reproduces 6");
    assert_ne!(grid(&replay), grid(b"\x1b[5mx"), "and not 5");
}

/// **Every read size and every split into two reads gives the screen and
/// the answers of one whole read.** Each sequence appears twice, so the SGR
/// table's hit path (`sgr-table`) answers the second, and the program holds
/// all three values in each order.
#[test]
fn the_blink_rates_are_the_same_at_every_read_size() {
    const P: &[u8] = b"\x1b[5mA\x1b[6mB\x1b[5mC\x1b[6mD\x1b[5;6mE\x1b[6;5mF\
\x1b[6;25mG\x1b[6mH\x1bP$qm\x1b\\\x1b[25mI\x1b[6mJ\x1b[5;6mK\x1bP$qm\x1b\\";
    let whole = run(P);
    assert_eq!(
        String::from_utf8_lossy(&whole.1),
        "\x1bP1$r0;6m\x1b\\\x1bP1$r0;6m\x1b\\",
        "the control: the whole read answers rapid twice"
    );
    let ws = support::CORPUS;
    let fed = |reads: &mut dyn Iterator<Item = &[u8]>| {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        for r in reads {
            t.write_at(BytePos::ZERO, r);
            t.feed(BytePos::ZERO, ReadLen::from_index(r.len()));
        }
        (support::screen(&t), t.reply().to_vec())
    };
    for size in 1..=P.len() {
        assert_eq!(fed(&mut P.chunks(size)), whole, "read size {size}");
    }
    for at in 1..P.len() {
        let (a, b) = P.split_at(at);
        assert_eq!(fed(&mut [a, b].into_iter()), whole, "split at {at}");
    }
}

/// **The richest rendition with rapid blink still round-trips.**
/// `ParamIdx::CAPACITY` counts one blink rate, since 5 and 6 replace each
/// other; this is the reply that count has to hold, every attribute an
/// application can hold at once with 6 for its rate, the widest underline
/// spelling, a font and three truecolours, sent back as one sequence.
#[test]
fn the_richest_rapid_rendition_round_trips() {
    const RICH: &[u8] = b"\x1b[1;2;3;4:3;6;7;8;9;20;26;51;52;53;60;61;62;63;64;11m\
\x1b[58;2;255;255;255;38;2;255;255;255;48;2;255;255;255m";
    let reply = rendition(RICH);
    assert!(
        reply.starts_with(b"\x1bP1$r0;1;2;3;6;7;"),
        "{:?}",
        String::from_utf8_lossy(&reply)
    );
    let body = &reply[5..reply.len() - 2];
    // A colon joins a sub-parameter, which takes a slot of its own.
    let params = body.iter().filter(|&&b| b == b';' || b == b':').count() + 1;
    assert!(params <= ParamIdx::CAPACITY, "{params} parameters");
    let replay = [b"\x1b[", body, b"x"].concat();
    assert_eq!(
        grid(&replay),
        grid(&[RICH, b"x"].concat()),
        "the reply reproduces it"
    );
}
