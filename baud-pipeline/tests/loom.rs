//! **The handoff under every interleaving of its two threads**, run by
//! loom: the ingest thread fills and publishes while the frame thread
//! takes. The frame thread never reads a slot the ingest thread is still
//! filling, and the last slot published is always taken. No preemption
//! bound is set, so each model runs every interleaving.
//!
//! Built only under `--cfg loom`:
//! `RUSTFLAGS="-D warnings --cfg loom" cargo test --release -p baud-pipeline --test loom`.

#![cfg(loom)]

use baud_pipeline::handoff::{IngestEnd, handoff};
use loom::thread;

/// A slot written with its number first and last: a take that reads two
/// different numbers read a slot half written.
#[derive(Default)]
struct Seq {
    first: u64,
    last: u64,
}

/// Fills the back slot with `n` and publishes it.
fn fill(ingest: &mut IngestEnd<Seq>, n: u64) {
    let mut s = ingest.filling();
    s.first = n;
    s.last = n;
    s.publish();
}

/// **Two publishes race one take**: the take reads nothing, the first or
/// the second, each whole; after both publishes the second is taken,
/// by that take or the next.
#[test]
fn two_publishes_race_one_take() {
    loom::model(|| {
        let (mut ingest, mut frame) = handoff(Seq::default, || ());
        let writer = thread::spawn(move || {
            fill(&mut ingest, 1);
            fill(&mut ingest, 2);
            ingest
        });
        let raced = frame.take().map(|s| {
            assert_eq!(s.first, s.last, "a slot half written");
            s.first
        });
        assert!(matches!(raced, None | Some(1 | 2)), "{raced:?}");
        let ingest = writer.join().unwrap();
        let after = frame.take().map(|s| {
            assert_eq!(s.first, s.last, "a slot half written");
            s.first
        });
        if raced == Some(2) {
            assert_eq!(after, None, "the second taken twice");
        } else {
            assert_eq!(after, Some(2), "the second publish lost");
        }
        assert!(ingest.taken(), "the last publish taken");
    });
}

/// **One publish races a take**: the take reads nothing or the slot
/// whole, and the ingest end sees the take once it happened.
#[test]
fn one_publish_races_a_take() {
    loom::model(|| {
        let (mut ingest, mut frame) = handoff(Seq::default, || ());
        let writer = thread::spawn(move || {
            fill(&mut ingest, 1);
            ingest
        });
        let raced = frame.take().map(|s| {
            assert_eq!(s.first, s.last, "a slot half written");
            s.first
        });
        let ingest = writer.join().unwrap();
        if raced.is_none() {
            assert!(!ingest.taken(), "published, not taken");
            assert_eq!(frame.take().map(|s| s.first), Some(1), "the publish lost");
        } else {
            assert_eq!(raced, Some(1));
        }
        assert!(ingest.taken(), "the publish taken");
    });
}
