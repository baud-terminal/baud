//! Replies matched to their calls by REPLY_SERIAL in one slot read
//! (`[[source: dbus-spec]]`, "Message Types": a reply's REPLY_SERIAL is
//! its call's serial; "Message Format": a serial is never 0).

use baud_domain::dbus::{Awaiting, Serial, Serials};

/// The serials a fresh connection draws: 2, 3, ...
fn drawn(n: usize) -> Vec<Serial> {
    let mut s = Serials::new();
    (0..n).map(|_| s.next()).collect()
}

/// **A reply takes what its call left, once**: the value comes back by
/// the call's serial, and a second reply to it takes nothing.
#[test]
fn a_reply_takes_its_call_s_value_once() {
    let mut a: Awaiting<&str, 4> = Awaiting::new();
    let s = drawn(3);
    a.insert(s[0], "caps").expect("a free slot");
    a.insert(s[1], "notify").expect("a free slot");
    a.insert(s[2], "close").expect("a free slot");
    assert_eq!(a.take(s[1]), Some("notify"));
    assert_eq!(a.take(s[1]), None, "taken");
    assert_eq!(a.take(s[0]), Some("caps"));
    assert_eq!(a.take(s[2]), Some("close"));
    assert_eq!(a.refusals().get(), 0);
}

/// **A reply to an unawaited serial takes nothing**, and leaves the slot
/// it reads to its own call: the stored serial is confirmed, not the slot.
#[test]
fn a_reply_to_an_unawaited_serial_takes_nothing() {
    let mut a: Awaiting<u8, 4> = Awaiting::new();
    let s = drawn(5);
    a.insert(s[0], 7).expect("a free slot");
    // s[4] is s[0] + 4: the same slot of four.
    assert_eq!(a.take(s[4]), None, "the slot's serial is another");
    assert_eq!(a.take(s[1]), None, "an empty slot");
    assert_eq!(a.take(Serial::HELLO), None);
    assert_eq!(a.take(s[0]), Some(7), "still waiting");
}

/// **A held slot refuses the new call and counts it**, handing its value
/// back; the call that holds it is not evicted.
#[test]
fn a_held_slot_refuses_and_counts() {
    let mut a: Awaiting<&str, 4> = Awaiting::new();
    let s = drawn(9);
    a.insert(s[0], "first").expect("a free slot");
    let refused = a.insert(s[4], "second").expect_err("the slot is held");
    assert_eq!(refused.held(), s[0]);
    assert_eq!(refused.into_inner(), "second");
    let again = a.insert(s[8], "third").expect_err("still held");
    assert_eq!(again.into_inner(), "third");
    assert_eq!(a.refusals().get(), 2);
    assert_eq!(a.take(s[4]), None, "never inserted");
    assert_eq!(a.take(s[0]), Some("first"), "not evicted");
    a.insert(s[8], "third").expect("free again");
    assert_eq!(a.take(s[8]), Some("third"));
    assert_eq!(a.refusals().get(), 2);
}

/// **Thirty-two calls in flight at once, the contract's size**: every
/// serial of a run of 32 has its own slot.
#[test]
fn thirty_two_consecutive_serials_fit() {
    let mut a: Awaiting<usize, 32> = Awaiting::default();
    let s = drawn(32);
    for (i, &serial) in s.iter().enumerate() {
        a.insert(serial, i).expect("its own slot");
    }
    for (i, &serial) in s.iter().enumerate().rev() {
        assert_eq!(a.take(serial), Some(i));
    }
    assert_eq!(a.refusals().get(), 0);
}
