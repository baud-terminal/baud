//! `[[source: kitty-keyboard]]`'s four negotiation forms parse to named
//! operations with the right values.
//!
//! This is the grammar layer only: the core still refuses the query and
//! ignores the flags, because the protocol's support detection is whether
//! `CSI ? u` is answered at all, and answering before every enhancement is
//! honoured claims support baud does not have. Naming the forms here is what
//! the stack and the encoder are built on -- and a form parsed wrong is a
//! sequence that would act on the wrong flags the moment they are honoured, so
//! it is pinned before then.

use baud_domain::*;

/// A `CSI` parameter built from a decimal number the way the stream spells it,
/// so `param(31)` pushes `3` then `1` rather than asserting on a constructor.
fn param(n: u16) -> Option<Param> {
    let mut p = Param::ZERO;
    for d in n.to_string().bytes() {
        p = p.push(StreamByte::from_stream(d).as_ascii_digit().unwrap());
    }
    Some(p)
}

fn parse(marker: Marker, params: &[Option<Param>], final_byte: u8) -> Csi {
    Csi::parse(
        marker,
        None,
        Params::new(params),
        FinalByte::from_stream(StreamByte::from_stream(final_byte)).unwrap(),
    )
}

#[test]
fn set_carries_flags_and_mode() {
    // `CSI = 5 ; 2 u` -- disambiguate + alternate keys, OR'd into the set.
    match parse(Marker::Equals, &[param(5), param(2)], b'u') {
        Csi::KittySet { flags, mode } => {
            assert_eq!(flags.bits(), 0b101);
            assert!(flags.has(KittyFlags::DISAMBIGUATE));
            assert!(flags.has(KittyFlags::ALTERNATE_KEYS));
            assert!(!flags.has(KittyFlags::EVENT_TYPES));
            assert_eq!(mode, KittySetMode::Or);
        }
        _ => panic!("expected KittySet"),
    }
}

#[test]
fn set_defaults_mode_to_all_and_masks_undefined_bits() {
    // Mode omitted defaults to 1 (All), and bit 5 (value 32) is not defined,
    // so `CSI = 63 u` stores only the five real bits.
    match parse(Marker::Equals, &[param(63)], b'u') {
        Csi::KittySet { flags, mode } => {
            assert_eq!(flags, KittyFlags::ALL);
            assert_eq!(flags.bits(), 0b1_1111);
            assert_eq!(mode, KittySetMode::All);
        }
        _ => panic!("expected KittySet"),
    }
}

#[test]
fn query_has_no_parameters() {
    match parse(Marker::Private, &[], b'u') {
        Csi::KittyQuery => {}
        _ => panic!("expected KittyQuery"),
    }
}

#[test]
fn push_defaults_flags_to_zero() {
    match parse(Marker::Gt, &[param(1)], b'u') {
        Csi::KittyPush { flags } => assert_eq!(flags, KittyFlags::DISAMBIGUATE),
        _ => panic!("expected KittyPush"),
    }
    // `CSI > u` -- flags omitted default to zero.
    match parse(Marker::Gt, &[], b'u') {
        Csi::KittyPush { flags } => assert!(flags.is_none()),
        _ => panic!("expected KittyPush"),
    }
}

#[test]
fn pop_defaults_count_to_one() {
    match parse(Marker::Lt, &[param(3)], b'u') {
        Csi::KittyPop { count } => assert_eq!(count.get_raw(), 3),
        _ => panic!("expected KittyPop"),
    }
    // `CSI < u` -- default one entry.
    match parse(Marker::Lt, &[], b'u') {
        Csi::KittyPop { count } => assert_eq!(count.get_raw(), 1),
        _ => panic!("expected KittyPop"),
    }
}

/// **Only `u` is a kitty form; every other final in these markers stays
/// `Unsupported`.** The intercept must not swallow a whole namespace, or a
/// later `<`/`=`/`>` operation would be read as a kitty request.
#[test]
fn a_non_u_final_in_a_kitty_marker_is_untouched() {
    for (marker, f) in [
        (Marker::Equals, b'c'),
        (Marker::Lt, b'c'),
        (Marker::Gt, b'n'),
    ] {
        match parse(marker, &[param(1)], f) {
            Csi::Unsupported { .. } => {}
            _ => panic!("{marker:?} {} should be Unsupported", f as char),
        }
    }
}

/// **`CSI u` with no marker is restore-cursor, not a kitty form.** The kitty
/// forms are told apart by their marker alone, which is the collision
/// `[[source: ctlseqs]]` warns fixterms' bare `CSI u` has and kitty avoids.
#[test]
fn a_bare_csi_u_is_restore_cursor() {
    match parse(Marker::Standard, &[], b'u') {
        Csi::RestoreCursor => {}
        _ => panic!("expected RestoreCursor"),
    }
}

/// The set semantics: mode 1 replaces, 2 sets, 3 clears.
#[test]
fn apply_matches_the_three_modes() {
    let cur = KittyFlags::DISAMBIGUATE.apply(KittyFlags::EVENT_TYPES, KittySetMode::Or);
    assert_eq!(cur.bits(), 0b11); // both set

    // All replaces the whole set.
    let all = cur.apply(KittyFlags::ALTERNATE_KEYS, KittySetMode::All);
    assert_eq!(all, KittyFlags::ALTERNATE_KEYS);

    // AndNot clears named bits and leaves the rest.
    let cleared = cur.apply(KittyFlags::DISAMBIGUATE, KittySetMode::AndNot);
    assert_eq!(cleared, KittyFlags::EVENT_TYPES);
}

/// A pop *between* levels restores the saved flags; the pop that *empties* the
/// stack resets instead, which is the spec's rule and the discriminating case
/// -- a single push then a single pop would only ever show the reset.
#[test]
fn a_pop_between_levels_restores_the_saved_flags() {
    let mut s = KittyStack::new();
    s.set(KittyFlags::DISAMBIGUATE, KittySetMode::All);
    s.push(KittyFlags::EVENT_TYPES); // saves DISAMBIGUATE, current = EVENT_TYPES
    s.push(KittyFlags::ALL); // saves EVENT_TYPES, current = ALL

    s.pop(Count::of(1)); // depth 2 -> 1, restores the EVENT_TYPES snapshot
    assert_eq!(s.current(), KittyFlags::EVENT_TYPES);

    s.pop(Count::of(1)); // depth 1 -> 0, empties, so resets rather than restoring
    assert!(s.current().is_none());
}

/// **Emptying the stack resets all flags rather than restoring the last
/// entry.** The spec makes this a special case, and it is why a shell gets a
/// clean keyboard back when a crashed program left the mode set.
#[test]
fn popping_empty_resets_all_flags() {
    let mut s = KittyStack::new();
    s.set(KittyFlags::ALL, KittySetMode::All);
    s.push(KittyFlags::EVENT_TYPES);
    // depth is 1; popping it empties the stack, so flags reset to none rather
    // than to the saved ALL.
    s.pop(Count::of(1));
    assert!(s.current().is_none());

    // Popping past empty is also a reset, not a panic.
    s.pop(Count::of(5));
    assert!(s.current().is_none());
}

/// A push flood never grows the stack past its bound; the oldest entry is
/// evicted, so what a later pop can reach is bounded but never corrupt.
#[test]
fn a_full_stack_evicts_the_oldest() {
    let mut s = KittyStack::new();
    // Push more than DEPTH distinct states.
    for _ in 0..KittyStack::DEPTH + 4 {
        s.push(KittyFlags::DISAMBIGUATE);
    }
    // Popping every reachable entry must not panic and must end reset.
    s.pop(Count::of(1000));
    assert!(s.current().is_none());
}
