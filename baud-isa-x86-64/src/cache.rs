//! The x86-64 cache probe: `CPUID`, deterministic cache parameters.
//!
//! Leaf `4` describes one cache per sub-leaf until a sub-leaf whose type
//! field is zero, and every geometry field in it is stored as `n - 1`:
//! `EAX[4:0]` type (1 data, 2 instruction, 3 unified), `EAX[7:5]` level,
//! `EBX[11:0]` line, `EBX[21:12]` partitions, `EBX[31:22]` ways, `ECX`
//! sets, and `size = ways * partitions * line * sets`. AMD leaves leaf 4
//! empty and carries the identical layout at `0x8000001D`, so the same walk
//! runs there when the Intel leaf yields nothing.
//!
//! `CPUID` is baseline on x86-64 -- every processor that runs the ISA has it
//! -- so there is no feature to establish and the probe is a safe call. What
//! it cannot see is a hypervisor that reports the leaf as absent, and that
//! is the case the `std` feature's sysfs fallback exists for.

use baud_domain::{CacheGeometry, CacheLevel, CacheSize, LineSize, Ways};
use baud_isa::CacheProbe;
use core::arch::x86_64::{__cpuid, __cpuid_count};

/// This machine, as `CPUID` describes it.
pub struct Host;

impl CacheProbe for Host {
    fn detect() -> CacheGeometry {
        let g = walk(Leaf::INTEL);
        if g != CacheGeometry::UNKNOWN {
            return g;
        }
        // AMD: the same table under the extended range.
        let g = walk(Leaf::AMD);
        #[cfg(all(feature = "std", target_os = "linux"))]
        if g == CacheGeometry::UNKNOWN {
            return baud_isa::sysfs::detect();
        }
        g
    }
}

/// More sub-leaves than any part describes; the walk ends at the first
/// null-typed one long before this, and the bound is for a leaf that never
/// produces one.
const MAX_SUBLEAF: u32 = 16;

/// A `CPUID` leaf that carries the deterministic cache parameters, and the
/// leaf whose `EAX` says whether it exists: asking past the highest leaf
/// returns the highest leaf's data on Intel rather than zeros, so the range
/// is checked before the walk rather than trusted to end it.
#[derive(Clone, Copy)]
struct Leaf {
    leaf: Reg,
    highest: Reg,
}

/// A `CPUID` leaf number or register value.
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Reg(u32);

impl Leaf {
    const INTEL: Leaf = Leaf {
        leaf: Reg(0x4),
        highest: Reg(0),
    };
    const AMD: Leaf = Leaf {
        leaf: Reg(0x8000_001d),
        highest: Reg(0x8000_0000),
    };

    fn present(self) -> bool {
        Reg(__cpuid(self.highest.0).eax) >= self.leaf
    }
}

/// One walk over the deterministic-cache-parameters table at `leaf`, or
/// unknown when the leaf is out of range.
fn walk(leaf: Leaf) -> CacheGeometry {
    if !leaf.present() {
        return CacheGeometry::UNKNOWN;
    }
    let mut line = None;
    let mut l1d = None;
    let mut l2 = None;
    let mut l3 = None;
    let mut sub = 0u32;
    while sub < MAX_SUBLEAF {
        let r = __cpuid_count(leaf.leaf.0, sub);
        sub += 1;
        let kind = r.eax & 0x1f;
        if kind == 0 {
            break;
        }
        // Instruction caches are not what the grid is striding against.
        if kind != 1 && kind != 3 {
            continue;
        }
        let level = (r.eax >> 5) & 0x7;
        let bytes_per_line = u64::from((r.ebx & 0xfff) + 1);
        let partitions = u64::from(((r.ebx >> 12) & 0x3ff) + 1);
        let ways = u64::from((r.ebx >> 22) + 1);
        let sets = u64::from(r.ecx) + 1;
        let size = bytes_per_line * partitions * ways * sets;
        let entry = match (usize::try_from(size), usize::try_from(ways)) {
            (Ok(size), Ok(ways)) => Some(CacheLevel::new(CacheSize::of(size), Ways::of(ways))),
            _ => None,
        };
        match level {
            1 => {
                l1d = entry;
                line = usize::try_from(bytes_per_line).ok().map(LineSize::of);
            }
            2 => l2 = entry,
            3 => l3 = entry,
            _ => {}
        }
    }
    CacheGeometry::new(line, l1d, l2, l3)
}
