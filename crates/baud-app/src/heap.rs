//! **The macOS app's allocator: the system's, but a block of [`OWN_MAP`]
//! or more is a mapping of its own**, zero-filled anonymous memory
//! ("Map zero-filled anonymous memory not associated with any specific
//! file", `[[source: darwin-mmap-2]]` 152-155) unmapped when it is freed.
//!
//! A window's image arena and its kitty PNG scratch are such blocks -- 95
//! and 190 MiB at the default budget, of which an idle window touches
//! none. Through the system's allocator, once the first window's close
//! had freed them, the second window's came back resident: twenty
//! windows' state opened and closed headless, the process's footprint was
//! 21.3 MB after the first and 316.1 to 316.4 MB from the second on; a
//! C program that only allocates the three blocks zero-filled, frees
//! them and allocates them again went from 1.4 to 286.6 MB the same
//! way. A mapping of its own is fresh zero pages each time, untouched
//! until an image writes it, and its pages go back to the system the
//! moment it is freed.
//!
//! Every resize of a block is the trait's own -- a new block, the bytes
//! copied, the old one freed -- rather than the system's in place. A read
//! reaches the main thread in a block made at its size (`pty.rs`), and the
//! core's parse and write path allocates nothing, so no read resizes one.
//!
//! `unsafe` is allowed here for the allocator itself: each block of
//! [`OWN_MAP`] or more is one `mmap` made for it and one `munmap` when it
//! is freed, told apart from the system's blocks by its layout's size,
//! which the caller passes back unchanged.
#![allow(unsafe_code)]

use core::alloc::{GlobalAlloc, Layout};
use core::ffi::c_void;
use std::alloc::System;

/// **The size from which a block is its own mapping**: above the grid and
/// history a window writes at its largest (under 8 MiB) and every frame
/// buffer a 4K display draws (33 MB), so those stay the system's, and
/// below the image arena and scratch.
pub const OWN_MAP: usize = 64 << 20;

/// The allocator ([`OWN_MAP`]).
pub struct Heap;

/// Whether a block of `layout` is its own mapping: one of at least
/// [`OWN_MAP`] bytes whose alignment the smallest page, 4 KiB, holds.
fn own(layout: Layout) -> bool {
    layout.size() >= OWN_MAP && layout.align() <= 1 << 12
}

/// A fresh mapping of `layout`'s size in zero-filled bytes, or null.
fn map(layout: Layout) -> *mut u8 {
    use crate::sys::{MAP_ANON, MAP_FAILED, MAP_PRIVATE, PROT_READ, PROT_WRITE};
    // SAFETY: a new private anonymous mapping at an address the kernel
    // chooses; nothing else is touched.
    let p = unsafe {
        crate::sys::mmap(
            core::ptr::null_mut(),
            layout.size(),
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANON,
            -1,
            0,
        )
    };
    match p == MAP_FAILED {
        true => core::ptr::null_mut(),
        false => p.cast(),
    }
}

// SAFETY: every block is the system allocator's with the caller's layout,
// or a mapping of at least the layout's size at a page's alignment, which
// holds every alignment `own` admits; each is freed by the path that made
// it, which the size passed back decides. A resize is the trait's own: a
// new block, the bytes copied, the old one freed, each by its path.
unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = match own(layout) {
            true => map(layout),
            // SAFETY: the caller's layout, passed on.
            false => unsafe { System.alloc(layout) },
        };
        counted(p, layout, Sign::Plus);
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = match own(layout) {
            // A fresh mapping reads zero, and its pages stay untouched.
            true => map(layout),
            // SAFETY: as above.
            false => unsafe { System.alloc_zeroed(layout) },
        };
        counted(p, layout, Sign::Plus);
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        match own(layout) {
            true => {
                // SAFETY: the whole mapping `alloc` made for this layout,
                // unmapped once, by the caller's free.
                let _ = unsafe {
                    crate::sys::munmap(ptr.cast::<c_void>(), crate::sys::MapLen(layout.size()))
                };
            }
            // SAFETY: a block the system gave for this layout.
            false => unsafe { System.dealloc(ptr, layout) },
        }
        counted(ptr, layout, Sign::Minus);
    }
}

#[global_allocator]
static HEAP: Heap = Heap;

/// Which way a count moves.
#[derive(Clone, Copy)]
enum Sign {
    Plus,
    Minus,
}

/// The counting a test reads ([`live`]) and the stage-3 row reads
/// ([`allocations`]): nothing in a shipping build.
#[cfg(not(any(test, feature = "effect-probe")))]
fn counted(_: *mut u8, _: Layout, _: Sign) {}

#[cfg(test)]
std::thread_local! {
    /// Bytes allocated on this thread and not yet freed.
    static LIVE: core::cell::Cell<isize> = const { core::cell::Cell::new(0) };
    /// The most [`LIVE`] has been since [`peak_during`] last began.
    static PEAK: core::cell::Cell<isize> = const { core::cell::Cell::new(0) };
    /// Whether a test marked this thread's work as baud's ([`marked`]).
    static MARKED: core::cell::Cell<Marked> = const { core::cell::Cell::new(Marked::No) };
}

#[cfg(any(test, feature = "effect-probe"))]
std::thread_local! {
    /// The blocks allocated on this thread while baud's work was on its
    /// stack ([`in_scope`]).
    static CALLS: core::cell::Cell<Calls> = const { core::cell::Cell::new(Calls(0)) };
}

/// **Blocks allocated**: one per `alloc` or `alloc_zeroed`, so one per
/// resize too, which is the trait's own new block. Bytes are not counted
/// here; a block freed is not taken off.
#[cfg(any(test, feature = "effect-probe"))]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Calls(pub u64);

#[cfg(any(test, feature = "effect-probe"))]
impl Calls {
    /// The blocks allocated since `before`.
    #[must_use]
    pub fn since(self, before: Calls) -> Calls {
        Calls(self.0.saturating_sub(before.0))
    }
}

#[cfg(any(test, feature = "effect-probe"))]
fn counted(p: *mut u8, layout: Layout, sign: Sign) {
    if p.is_null() {
        return;
    }
    #[cfg(test)]
    bytes(layout, sign);
    #[cfg(not(test))]
    let _ = layout;
    if matches!(sign, Sign::Plus) && in_scope() {
        let _ = CALLS.try_with(|c| c.set(Calls(c.get().0.saturating_add(1))));
    }
}

#[cfg(test)]
fn bytes(layout: Layout, sign: Sign) {
    let n = isize::try_from(layout.size()).unwrap_or(isize::MAX);
    let n = match sign {
        Sign::Plus => n,
        Sign::Minus => -n,
    };
    let _ = LIVE.try_with(|c| {
        c.set(c.get() + n);
        let _ = PEAK.try_with(|p| p.set(p.get().max(c.get())));
    });
}

/// **Whether this allocation is baud's own work on this thread**: under
/// the probe, a responder method or a turn on the stack
/// (`probe::held_now`, a `const` cell read with `try_with`, so it neither
/// allocates nor panics while the thread's locals are torn down); in a
/// test, a span the test marked ([`marked`]). The frame rig's harness
/// runs on the main thread between them, and is never counted.
#[cfg(any(test, feature = "effect-probe"))]
fn in_scope() -> bool {
    #[cfg(feature = "effect-probe")]
    let probe =
        baud_present_macos::probe::held_now() == Some(baud_present_macos::probe::Busy::Working);
    #[cfg(not(feature = "effect-probe"))]
    let probe = false;
    #[cfg(test)]
    let test = MARKED.try_with(core::cell::Cell::get) == Ok(Marked::Yes);
    #[cfg(not(test))]
    let test = false;
    probe || test
}

/// **The blocks this thread has allocated inside baud's work** so far
/// ([`in_scope`]): what the event rig's still screen holds at none.
#[cfg(any(test, feature = "effect-probe"))]
pub fn allocations() -> Calls {
    CALLS.with(core::cell::Cell::get)
}

/// Whether a test marked this thread's work as baud's.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Marked {
    No,
    Yes,
}

/// **`f`, marked as baud's work** for [`allocations`]: the unit tests'
/// stand-in for a responder method, which a default build has no probe
/// to mark.
#[cfg(test)]
fn marked<R>(f: impl FnOnce() -> R) -> R {
    MARKED.with(|m| m.set(Marked::Yes));
    let r = f();
    MARKED.with(|m| m.set(Marked::No));
    r
}

/// **The bytes this thread has allocated and not freed**, so a test reads
/// what a window's close gave back on its own thread whatever other
/// tests' threads do.
#[cfg(test)]
pub fn live() -> isize {
    LIVE.with(core::cell::Cell::get)
}

/// **The most bytes `f` held at once on this thread** beyond what was
/// live when it began, and what it returned.
#[cfg(test)]
pub fn peak_during<R>(f: impl FnOnce() -> R) -> (isize, R) {
    let start = live();
    PEAK.with(|p| p.set(start));
    let r = f();
    (PEAK.with(core::cell::Cell::get) - start, r)
}

#[cfg(test)]
mod tests {
    use super::{OWN_MAP, live};

    /// How much of a block a write has touched.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Touched {
        No,
        /// This many of its pages.
        Part(usize),
        Whole,
    }

    /// **The pages of `block` a write has touched**: in memory, or
    /// written and since compressed or paged out -- `<sys/mman.h>`'s
    /// `MINCORE_INCORE` and `MINCORE_PAGED_OUT`, which XNU's `mincore` sets
    /// from the page's query (`[[source: xnu-kern-mman-c]]` 1612-1623), a
    /// compressed page's query answering paged out (`[[source:
    /// xnu-vm-map-c]]` 18372-18387). A zero-filled page nothing has
    /// written is neither. The block's own pages, so no other thread's
    /// memory is in the count.
    fn touched(block: &[u8]) -> Touched {
        use crate::sys::{MINCORE_INCORE, MINCORE_PAGED_OUT, MapLen, getpagesize, mincore};
        // SAFETY: no arguments.
        let page = usize::try_from(unsafe { getpagesize() }).expect("a page size");
        assert_eq!(block.as_ptr().addr() % page, 0, "a mapping starts a page");
        let mut pages = vec![0; block.len().div_ceil(page)];
        // SAFETY: a mapped block from its first page, and a byte a page
        // of its length to fill.
        let r = unsafe {
            mincore(
                block.as_ptr().cast(),
                MapLen(block.len()),
                pages.as_mut_ptr(),
            )
        };
        assert_eq!(r, 0, "mincore: {}", std::io::Error::last_os_error());
        let n = pages
            .iter()
            .filter(|&&p| p & (MINCORE_INCORE | MINCORE_PAGED_OUT) != 0)
            .count();
        match n {
            0 => Touched::No,
            n if n == pages.len() => Touched::Whole,
            n => Touched::Part(n),
        }
    }

    /// **A window's arenas, freed and made again, come back untouched**:
    /// three rounds of the image arena, the query probe and the PNG
    /// scratch at the default budget's sizes, made and freed in a window's
    /// order, and no page of a round's arena or scratch has been written --
    /// through the system's allocator the second round came from the
    /// first's freed blocks, cleared with a write, 286 MiB charged. The
    /// control: a block written whole has every page touched, and the
    /// first round, made once it is freed, has none.
    ///
    /// The blocks' own pages are read, not the process's footprint: the
    /// footprint counts every thread's memory, and under `cargo test`
    /// another test's thread that writes 64 MiB during a round made the
    /// round read charged. That thread is here, holding [`OWN_MAP`]
    /// written bytes of its own for the whole test. Named break: `own`
    /// that answers no, every block the system's.
    #[test]
    fn a_windows_arenas_freed_and_made_again_come_back_untouched() {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let other = std::thread::spawn(move || {
            let mut b = vec![0u8; OWN_MAP];
            b.fill(1);
            std::hint::black_box(&mut b);
            ready_tx.send(()).expect("the test waits");
            let _ = done_rx.recv();
        });
        ready_rx.recv().expect("the other thread's block");
        let start = live();
        let mut written = vec![0u8; 2 * OWN_MAP];
        written.fill(1);
        std::hint::black_box(&mut written);
        assert_eq!(
            touched(&written),
            Touched::Whole,
            "the control: a block written whole is touched whole"
        );
        drop(written);
        for round in 0..3 {
            let arena = vec![0u8; 95 << 20];
            let probe = vec![0u8; 16 << 10];
            let scratch = vec![0u8; 190 << 20];
            assert_eq!(
                (touched(&arena), touched(&scratch)),
                (Touched::No, Touched::No),
                "round {round}: the arenas are untouched"
            );
            assert_eq!((arena[0], probe[0], scratch[0]), (0, 0, 0));
            drop((arena, probe, scratch));
        }
        assert_eq!(live(), start, "every byte counted back");
        done_tx.send(()).expect("the other thread waits");
        other.join().expect("the other thread");
    }

    /// **A block allocated inside baud's work is counted, one outside it
    /// is not, and a resize is a block**: the stage-3 call count's unit
    /// controls, marked by the test ([`super::marked`]) where the probe
    /// marks a responder method or a turn. Named break: [`super::in_scope`]
    /// answering yes always, and the block outside is counted.
    #[test]
    fn a_block_is_counted_inside_baud_s_work_and_not_outside() {
        use super::{Calls, allocations, marked};
        use std::hint::black_box;
        let before = allocations();
        let inside: Vec<u8> = marked(|| black_box(Vec::with_capacity(1)));
        let in_work = allocations().since(before);
        let before = allocations();
        let outside: Vec<u8> = black_box(Vec::with_capacity(1));
        let out_of_work = allocations().since(before);
        let before = allocations();
        let resized = marked(|| {
            let mut v: Vec<u8> = black_box(Vec::with_capacity(1));
            v.reserve_exact(4096);
            black_box(v)
        });
        let with_resize = allocations().since(before);
        drop(black_box((inside, outside, resized)));
        assert_eq!(
            (in_work, out_of_work, with_resize),
            (Calls(1), Calls(0), Calls(2)),
            "inside, outside, and a block resized: its first and its new one"
        );
    }

    /// **What a settle owes, written over a kept text in place, is no
    /// block; `format!` is one each time**: the mechanism of the one block
    /// a bare wake's settle made on the frame rig's counted still screen,
    /// where the probe kept a fresh `format!` of `Owes::Nothing` each
    /// settle. Counted through [`super::counted`] as the rig counts.
    #[test]
    fn a_report_written_in_place_allocates_nothing() {
        use super::{Calls, allocations, marked};
        use core::fmt::Write;
        use std::hint::black_box;
        let owes = crate::frames::Owes::Nothing;
        let mut kept = String::new();
        let _ = write!(kept, "{owes:?}");
        let before = allocations();
        let fresh = marked(|| black_box(format!("{owes:?}")));
        let formatted = allocations().since(before);
        let before = allocations();
        marked(|| {
            kept.clear();
            let _ = write!(kept, "{owes:?}");
        });
        let in_place = allocations().since(before);
        assert_eq!(kept, fresh, "the same text");
        assert_eq!(
            (formatted, in_place),
            (Calls(1), Calls(0)),
            "format!'s block, and none in place"
        );
    }

    /// **The settle tells the probe what it owes in place**: its call
    /// lends the value, and the probe writes it over the text it keeps
    /// (`probe::owed`), so a settle at quiet makes no block on baud's turn
    /// ([`a_report_written_in_place_allocates_nothing`]). Named break: the
    /// settle's `format!` back, and `owed` taking the `String` again.
    #[test]
    fn the_settle_reports_what_it_owes_in_place() {
        const APP: &str = crate::app_source::app_source();
        const PROBE: &str = include_str!("../../baud-present-macos/src/probe.rs");
        // Spelled apart, so this test's own text is not what it finds.
        const SETTLE: &str = concat!("    fn settle(&mut ", "self) {");
        const CALL: &str = concat!("probe::owed(", "&owes);");
        const OWED: &str = concat!("pub fn owed(what: ", "&dyn core::fmt::Debug) {");
        assert_eq!(APP.matches(SETTLE).count(), 1, "one settle");
        let settle = crate::app_source::split(SETTLE).nth(1).expect("the settle");
        let settle = &settle[..settle.find("\n    }\n").expect("the settle ends")];
        assert!(settle.contains(CALL), "the settle lends what it owes");
        assert!(
            !settle.contains(concat!("format", "!(")),
            "the settle formats"
        );
        assert_eq!(
            PROBE.matches(OWED).count(),
            1,
            "owed borrows what it writes"
        );
        let owed = PROBE.split(OWED).nth(1).expect("owed");
        let owed = &owed[..owed.find("\n}\n").expect("owed ends")];
        assert!(owed.contains("o.clear();"), "owed writes in place");
        assert!(
            !owed.contains(concat!("format", "!(")),
            "owed formats afresh"
        );
    }

    /// **The call count is built only for tests and the probe**: every
    /// item of it in this file is under `cfg(any(test, feature =
    /// "effect-probe"))`, the shipping arm of `counted` is the no-op, and
    /// `effect-probe` is not a default feature of the binary. The release
    /// build's symbols are the release gate's check. Named break: the
    /// count's items built for every macOS build, the no-op for none.
    #[test]
    fn the_call_count_is_built_only_for_tests_and_the_probe() {
        const SRC: &str = include_str!("heap.rs");
        const MANIFEST: &str = include_str!("../Cargo.toml");
        const GATE: &str = "#[cfg(any(test, feature = \"effect-probe\"))]";
        let (code, _) = SRC
            .split_once("#[cfg(test)]\nmod tests {")
            .expect("the tests module");
        let lines: Vec<&str> = code.lines().collect();
        for item in [
            "static CALLS:",
            "pub struct Calls(",
            "impl Calls {",
            "fn counted(p: *mut u8",
            "fn in_scope(",
            "pub fn allocations(",
        ] {
            let at = lines
                .iter()
                .position(|l| l.trim_start().starts_with(item))
                .unwrap_or_else(|| panic!("{item} in heap.rs"));
            // Its attributes: up over its docs, its other attributes and
            // the `thread_local!` that holds a static.
            let gated = lines[..at]
                .iter()
                .rev()
                .map(|l| l.trim())
                .take_while(|l| {
                    l.starts_with("///")
                        || l.starts_with("#[")
                        || l.starts_with("std::thread_local!")
                })
                .any(|l| l == GATE);
            assert!(gated, "{item} is built only under {GATE}");
        }
        assert!(
            code.contains(concat!(
                "#[cfg(not(any(test, feature = \"effect-probe\")))]\n",
                "fn counted(_: *mut u8, _: Layout, _: Sign) {}"
            )),
            "a shipping build's count is the no-op"
        );
        let features = MANIFEST
            .split_once("[features]")
            .map(|(_, f)| f.split("\n[").next().unwrap_or(f))
            .expect("the features");
        let default = features
            .lines()
            .find(|l| l.trim_start().starts_with("default"))
            .unwrap_or("");
        assert!(
            !default.contains("effect-probe"),
            "the probe is not a default feature: {default}"
        );
        assert!(
            features.contains("effect-probe = ["),
            "the feature is read where it is: {features}"
        );
    }
}
