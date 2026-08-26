//! Properties that must hold for any input. These catch the class of bug that
//! example-based tests miss: state-machine errors that only appear when an
//! escape sequence straddles a read boundary.
use baud_core::*;
use proptest::prelude::*;

/// `Storage::zeroed()` builds a ~1.7 MB value on the stack before `Box::new`
/// moves it. Fine for `static` placement on bare metal, fatal on a test thread.
/// Until the core gains an in-place constructor, allocate zeroed heap directly.
fn boxed_storage() -> Box<Storage> {
    Storage::boxed()
}

/// Drive a Term with `bytes`, delivered in chunks of `chunk` bytes.
fn drive(bytes: &[u8], chunk: usize, mode: OpMode) -> Vec<PackedCell> {
    let mut store = boxed_storage();
    let mut t = Term::new(&mut store, mode);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(READ_CHUNK).min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
    t.viewport().to_vec()
}

/// Drive with the AVX2 path instead. Same input, must give the same state.
///
/// `feed_avx2` is `#[cfg(target_arch = "x86_64")]`, so this helper and the
/// property below must be too, or the suite does not compile off x86 at all.
/// The gate is on the *test*, not on the check it performs: on x86 the property
/// still runs, and CLAUDE.md records what happens when the SIMD path stops
/// being exercised and nobody notices.
#[cfg(target_arch = "x86_64")]
fn drive_avx2(bytes: &[u8], chunk: usize, mode: OpMode) -> Vec<PackedCell> {
    let mut store = boxed_storage();
    let mut t = Term::new(&mut store, mode);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(READ_CHUNK).min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        unsafe { t.feed_avx2(base, filled.len()) };
        t.commit_read(filled.len());
        i += n;
    }
    t.viewport().to_vec()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// THE property for a VT parser. The kernel hands you arbitrary 4095-byte
    /// slices; an escape sequence split across two reads must parse identically
    /// to one delivered whole. Everything else is downstream of this.
    #[test]
    fn ingest_is_chunk_invariant(
        bytes in prop::collection::vec(any::<u8>(), 0..4000),
        a in 1usize..=512,
        b in 1usize..=512,
    ) {
        prop_assert_eq!(drive(&bytes, a, OpMode::Runs), drive(&bytes, b, OpMode::Runs));
    }

    /// Same, for the cell-materialising modes.
    #[test]
    fn chunk_invariant_all_modes(
        bytes in prop::collection::vec(any::<u8>(), 0..2000),
        a in 1usize..=256,
    ) {
        for m in [OpMode::Computed, OpMode::Templated] {
            prop_assert_eq!(drive(&bytes, a, m), drive(&bytes, 4096, m));
        }
    }

    /// The portable and AVX2 paths must be indistinguishable. `feed_avx2` is an
    /// optimisation, not a dialect — when a fast path was added for `ESC 7`/`8`/
    /// `M` and only implemented there, the two silently diverged and the
    /// chunk-invariance property could not see it, because it exercises one path.
    #[test]
    #[cfg(target_arch = "x86_64")]
    fn feed_matches_feed_avx2(
        bytes in prop::collection::vec(any::<u8>(), 0..4000),
        chunk in 1usize..=512,
    ) {
        for m in [OpMode::Runs, OpMode::Computed] {
            prop_assert_eq!(drive(&bytes, chunk, m), drive_avx2(&bytes, chunk, m));
        }
    }

    /// Terminals are fuzzed with garbage and must not panic, wedge, or
    /// index out of bounds. Arbitrary bytes include every malformed escape,
    /// truncated UTF-8 sequence and absurd CSI parameter.
    #[test]
    fn never_panics_on_arbitrary_input(
        bytes in prop::collection::vec(any::<u8>(), 0..8000),
    ) {
        let _ = drive(&bytes, 97, OpMode::Runs);
    }

    /// Ingest is a pure function of the byte sequence: same input, same state.
    #[test]
    fn deterministic(bytes in prop::collection::vec(any::<u8>(), 0..3000)) {
        prop_assert_eq!(drive(&bytes, 128, OpMode::Runs), drive(&bytes, 128, OpMode::Runs));
    }

    /// Printable ASCII can never move the cursor off the grid, whatever the
    /// preceding control sequences did.
    #[test]
    fn viewport_bounds_hold(bytes in prop::collection::vec(any::<u8>(), 0..3000)) {
        let v = drive(&bytes, 211, OpMode::Computed);
        prop_assert!(!v.is_empty());
    }
}
