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

/// The counting a test reads ([`live`]): nothing outside a test.
#[cfg(not(test))]
fn counted(_: *mut u8, _: Layout, _: Sign) {}

#[cfg(test)]
std::thread_local! {
    /// Bytes allocated on this thread and not yet freed.
    static LIVE: core::cell::Cell<isize> = const { core::cell::Cell::new(0) };
}

#[cfg(test)]
fn counted(p: *mut u8, layout: Layout, sign: Sign) {
    if p.is_null() {
        return;
    }
    let n = isize::try_from(layout.size()).unwrap_or(isize::MAX);
    let n = match sign {
        Sign::Plus => n,
        Sign::Minus => -n,
    };
    let _ = LIVE.try_with(|c| c.set(c.get() + n));
}

/// **The bytes this thread has allocated and not freed**, so a test reads
/// what a window's close gave back on its own thread whatever other
/// tests' threads do.
#[cfg(test)]
pub fn live() -> isize {
    LIVE.with(core::cell::Cell::get)
}

#[cfg(test)]
mod tests {
    use super::{OWN_MAP, live};

    /// **A window's arenas, freed and made again, come back untouched**:
    /// three rounds of the image arena, the query probe and the PNG
    /// scratch at the default budget's sizes, made and freed in a window's
    /// order, and none of the later rounds is charged -- through the
    /// system's allocator the second round came from the first's freed
    /// blocks, cleared with a write, 286 MiB charged. The control: a block
    /// written whole is charged while it lives and given back when freed.
    /// Reads the process's footprint, so it holds only with one process a
    /// test, as nextest runs it.
    #[test]
    fn a_windows_arenas_freed_and_made_again_come_back_untouched() {
        let footprint = || {
            use crate::sys::{TASK_VM_INFO, TaskVmInfo, mach_task_self_, task_info};
            let mut info = TaskVmInfo::default();
            let words = core::mem::size_of::<TaskVmInfo>() / core::mem::size_of::<i32>();
            let mut count = u32::try_from(words).unwrap();
            // SAFETY: the flavor's struct and its length in `natural_t`s,
            // as the probe holds them.
            let kr = unsafe {
                task_info(
                    mach_task_self_,
                    TASK_VM_INFO,
                    (&raw mut info).cast(),
                    &raw mut count,
                )
            };
            assert_eq!(kr, 0, "task_info");
            info.phys_footprint.0
        };
        let margin = u64::try_from(OWN_MAP).unwrap();
        let start = live();
        let base = footprint();
        let mut written = vec![0u8; 2 * OWN_MAP];
        written.fill(1);
        std::hint::black_box(&mut written);
        assert!(
            footprint() >= base + margin,
            "the control: a block written whole is charged"
        );
        drop(written);
        assert!(footprint() < base + margin, "and given back when freed");
        for round in 0..3 {
            let arena = vec![0u8; 95 << 20];
            let probe = vec![0u8; 16 << 10];
            let scratch = vec![0u8; 190 << 20];
            assert!(
                footprint() < base + margin,
                "round {round}: the arenas are untouched"
            );
            assert_eq!((arena[0], probe[0], scratch[0]), (0, 0, 0));
            drop((arena, probe, scratch));
        }
        assert_eq!(live(), start, "every byte counted back");
    }
}
