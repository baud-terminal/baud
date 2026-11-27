//! The hold CRM's GRAPHIC state keeps a control sequence in until its final
//! byte says whether it is RESET MODE, performed, or another function,
//! shown: a fixed capacity that remote input cannot grow, and an over-long
//! sequence shown rather than performed.

use baud_domain::{CrmEnd, CrmHold, CrmStep};

/// How a held sequence ended.
#[derive(PartialEq, Eq, Debug)]
enum End {
    Whole(Vec<u8>),
    Shown,
}

/// What one sequence did to a hold: the bytes it kept, each spill's bytes,
/// the bytes it passed through, and its end.
struct Fed {
    kept: Vec<u8>,
    spills: Vec<Vec<u8>>,
    passed: Vec<u8>,
    end: End,
}

fn feed(h: &mut CrmHold, bytes: impl Iterator<Item = u8>) -> Fed {
    let (mut kept, mut spills, mut passed) = (Vec::new(), Vec::new(), Vec::new());
    for b in bytes {
        match h.hold(b) {
            CrmStep::Kept => kept.push(b),
            CrmStep::Spill(held) => spills.push(held.to_vec()),
            CrmStep::Pass => passed.push(b),
        }
        assert!(h.held().len() <= CrmHold::CAPACITY);
    }
    let end = match h.finish() {
        CrmEnd::Whole(held) => End::Whole(held.to_vec()),
        CrmEnd::Shown => End::Shown,
    };
    Fed {
        kept,
        spills,
        passed,
        end,
    }
}

/// **An endless sequence stays in the hold's capacity**: a million parameter
/// bytes after `CSI` are kept up to `CAPACITY`, given back once to be shown,
/// and every byte after passes through to be shown as it arrives. The hold
/// never grows and the sequence is never performed.
#[test]
fn an_endless_sequence_stays_in_the_holds_fixed_capacity() {
    let len = 1_000_000;
    let size = core::mem::size_of::<CrmHold>();
    let mut h = CrmHold::new();
    let fed = feed(
        &mut h,
        [0x9B]
            .into_iter()
            .chain(core::iter::repeat_n(b'0', len - 1)),
    );
    assert_eq!(fed.kept.len(), CrmHold::CAPACITY);
    assert_eq!(
        fed.spills,
        core::slice::from_ref(&fed.kept),
        "one spill, of what was kept"
    );
    assert_eq!(
        fed.passed.len(),
        len - CrmHold::CAPACITY - 1,
        "the spilling byte is shown with the spill"
    );
    assert_eq!(
        fed.end,
        End::Shown,
        "an over-long sequence is never performed"
    );
    assert_eq!(core::mem::size_of::<CrmHold>(), size);
    assert!(size <= CrmHold::CAPACITY + 8, "{size} bytes");
}

/// **A sequence that fits is given back whole at its end**, for the caller
/// to perform it if it is RESET MODE and show it otherwise.
#[test]
fn a_sequence_that_fits_is_given_back_whole() {
    let mut h = CrmHold::new();
    let fed = feed(&mut h, b"\x1b[3l".iter().copied());
    assert_eq!(fed.kept, b"\x1b[3l");
    assert!(fed.spills.is_empty() && fed.passed.is_empty());
    assert_eq!(fed.end, End::Whole(b"\x1b[3l".to_vec()));
}

/// **The bound is exact**: `CAPACITY` bytes are held whole, and the next one
/// spills them.
#[test]
fn capacity_bytes_are_held_whole_and_one_more_spills() {
    let mut h = CrmHold::new();
    let fed = feed(&mut h, core::iter::repeat_n(b'1', CrmHold::CAPACITY));
    assert_eq!(fed.kept.len(), CrmHold::CAPACITY);
    assert!(fed.spills.is_empty());
    assert_eq!(fed.end, End::Whole(fed.kept.clone()));

    let fed = feed(&mut h, core::iter::repeat_n(b'1', CrmHold::CAPACITY + 1));
    assert_eq!(fed.kept.len(), CrmHold::CAPACITY);
    assert_eq!(fed.spills.len(), 1);
    assert!(fed.passed.is_empty());
    assert_eq!(fed.end, End::Shown);
}

/// **Each sequence starts empty**: the end of a spilled one leaves nothing
/// behind, so the next sequence is held and can be performed.
#[test]
fn the_sequence_after_a_spill_is_held_again() {
    let mut h = CrmHold::new();
    feed(&mut h, core::iter::repeat_n(b'0', 3 * CrmHold::CAPACITY));
    assert!(h.held().is_empty());
    let fed = feed(&mut h, b"\x1b[3l".iter().copied());
    assert!(fed.spills.is_empty() && fed.passed.is_empty());
    assert_eq!(fed.end, End::Whole(b"\x1b[3l".to_vec()));
}
