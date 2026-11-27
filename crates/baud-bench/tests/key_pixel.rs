//! `key-pixel`'s reading of a key, over a scripted screen and keyboard: no
//! compositor. What a compositor serves is the script's; the rules are the
//! tool's.

use std::collections::VecDeque;
use std::time::Duration;

use baud_bench::key_latency::poster::Evdev;
use baud_bench::linux::keypixel::{
    Frame, Frames, Key, Keys, Nanos, Screen, Stop, Tries, one_key, settle,
};

/// What the fakes saw, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Did {
    Request,
    Post(u32),
}

/// A screen that serves the frames it is given, one a request, and a
/// keyboard whose every post is at `post`.
struct Fake {
    frames: VecDeque<Option<Frame>>,
    did: Vec<Did>,
    post: Nanos,
    asked: Asked,
}

/// Requests outstanding.
#[derive(Debug)]
struct Asked(u32);

impl Fake {
    fn new(post: u128, frames: Vec<Option<(u128, &[u8])>>) -> Fake {
        Fake {
            frames: frames
                .into_iter()
                .map(|f| {
                    f.map(|(at, bytes)| Frame {
                        at: Nanos(at),
                        bytes: bytes.to_vec(),
                    })
                })
                .collect(),
            did: Vec::new(),
            post: Nanos(post),
            asked: Asked(0),
        }
    }
}

impl Screen for Fake {
    fn request(&mut self) -> Result<(), Stop> {
        self.did.push(Did::Request);
        self.asked.0 += 1;
        Ok(())
    }
    fn served(&mut self, _within: Duration) -> Result<Option<Frame>, Stop> {
        assert!(
            self.asked.0 > 0,
            "a frame is waited for with no request outstanding"
        );
        self.asked.0 -= 1;
        Ok(self.frames.pop_front().unwrap_or(None))
    }
}

impl Keys for Fake {
    fn post(&mut self, code: Evdev) -> Result<Nanos, Stop> {
        self.did.push(Did::Post(code.0));
        Ok(self.post)
    }
}

const BASE: &[u8] = b"....";
const A: &[u8] = b"a...";
const KEY_A: Evdev = Evdev(30);
const LIMIT: Duration = Duration::from_secs(1);

/// **The latency is the changed frame's commit minus the key's post.**
#[test]
fn a_key_shown_in_the_first_frame_reads_its_commit_minus_its_post() {
    let mut f = Fake::new(1_000_000, vec![Some((1_700_000, A))]);
    let got = one_key(&mut f, KEY_A, BASE, LIMIT).unwrap();
    assert_eq!(
        got,
        Key::Shown {
            post: Nanos(1_000_000),
            latency: Nanos(700_000),
            frames: Frames(1)
        }
    );
}

/// **A request is outstanding before the key goes out**: Hyprland serves
/// a request at its next output commit, so a commit with none pending is
/// never seen.
#[test]
fn the_request_is_outstanding_before_the_key() {
    let mut f = Fake::new(1_000_000, vec![Some((1_700_000, A))]);
    one_key(&mut f, KEY_A, BASE, LIMIT).unwrap();
    assert_eq!(f.did, [Did::Request, Did::Post(30)]);
}

/// **A frame with the baseline's bytes is passed over**, and a new request
/// goes out at once, so the key's own commit is never missed.
#[test]
fn a_frame_with_the_baseline_s_bytes_is_passed_over() {
    let mut f = Fake::new(
        1_000_000,
        vec![Some((1_100_000, BASE)), Some((5_000_000, A))],
    );
    let got = one_key(&mut f, KEY_A, BASE, LIMIT).unwrap();
    assert_eq!(
        got,
        Key::Shown {
            post: Nanos(1_000_000),
            latency: Nanos(4_000_000),
            frames: Frames(2)
        }
    );
    assert_eq!(f.did, [Did::Request, Did::Post(30), Did::Request]);
}

/// **No changed frame within the limit is a key not shown**, not a
/// latency.
#[test]
fn no_frame_within_the_limit_is_not_shown() {
    let mut f = Fake::new(1_000_000, vec![Some((1_100_000, BASE)), None]);
    let got = one_key(&mut f, KEY_A, BASE, LIMIT).unwrap();
    assert_eq!(got, Key::NotShown { frames: Frames(1) });
}

/// **A change committed before the key is not the key's**: the rect moved
/// on its own, so that key reads as not idle, no latency, and the run goes
/// on from a fresh baseline.
#[test]
fn a_change_committed_before_the_key_is_not_idle() {
    let mut f = Fake::new(1_000_000, vec![Some((900_000, A))]);
    let got = one_key(&mut f, KEY_A, BASE, LIMIT).unwrap();
    assert_eq!(got, Key::NotIdle { frames: Frames(1) });
}

/// **The baseline is a screen gone quiet**: after the reset's commit, a
/// request must go unserved for the quiet time; each commit in it starts
/// the wait again from its bytes.
#[test]
fn the_baseline_is_the_last_frame_before_a_quiet_request() {
    let b2: &[u8] = b"..,.";
    let mut f = Fake::new(
        1_000_000,
        vec![Some((1_200_000, BASE)), Some((1_300_000, b2)), None],
    );
    let (base, busy) = settle(
        &mut f,
        Evdev(14),
        Duration::from_millis(500),
        LIMIT,
        Tries(5),
    )
    .unwrap();
    assert_eq!(base, b2);
    assert_eq!(busy, Frames(1));
    assert_eq!(f.did[..2], [Did::Request, Did::Post(14)]);
}

/// **A screen that never goes quiet stops the run**: a blinking cursor in
/// the rect would fire the detection early.
#[test]
fn a_screen_that_never_goes_quiet_stops_the_run() {
    let mut f = Fake::new(
        1_000_000,
        vec![
            Some((1_200_000, BASE)),
            Some((1_300_000, A)),
            Some((1_400_000, BASE)),
            Some((1_500_000, A)),
        ],
    );
    let got = settle(
        &mut f,
        Evdev(14),
        Duration::from_millis(500),
        LIMIT,
        Tries(2),
    );
    assert!(
        matches!(got, Err(Stop::Failed(ref why)) if why.contains("hold still")),
        "{got:?}"
    );
}

/// **Commits that leave the rect as it was are quiet**: Hyprland commits the
/// whole output on any damage anywhere, so a quiet screen is one whose rect
/// holds its bytes, not one that never commits.
#[test]
fn commits_that_leave_the_rect_alone_are_quiet() {
    let mut f = Fake::new(
        1_000_000,
        vec![
            Some((1_200_000, BASE)),
            Some((1_300_000, BASE)),
            Some((1_400_000, BASE)),
            None,
        ],
    );
    let (base, busy) = settle(
        &mut f,
        Evdev(14),
        Duration::from_millis(500),
        LIMIT,
        Tries(1),
    )
    .unwrap();
    assert_eq!(base, BASE);
    assert_eq!(busy, Frames(0));
}
