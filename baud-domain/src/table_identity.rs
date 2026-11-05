//! **The compiled tables are the committed ones**, element for element and
//! const for const: `build/colour_names.rs` and `build/placeholder.rs`'s
//! output in `OUT_DIR` against the files the Python generators wrote,
//! before those files are deleted.

#[path = "themefile/colour_names.rs"]
mod committed_kitty;
#[path = "placeholder_data.rs"]
mod committed_placeholder;
#[path = "vt/x_colour_names.rs"]
mod committed_x;

mod kitty {
    include!(concat!(env!("OUT_DIR"), "/kitty_colour_names.rs"));
}
mod x {
    include!(concat!(env!("OUT_DIR"), "/x_colour_names.rs"));
}

use crate::colour_name::ColourSlot;
use crate::placeholder_data as p1;
use committed_placeholder as p0;

fn same<T: PartialEq + core::fmt::Debug>(
    name: &'static str,
    a: &[T],
    b: &[T],
) -> (&'static str, usize) {
    assert_eq!(a.len(), b.len(), "{name}: lengths differ");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert_eq!(x, y, "{name}[{i}] differs");
    }
    (name, a.len())
}

/// The committed tuples as slots.
fn slots(t: &[(u16, u8, [u8; 3]); 1024]) -> alloc::vec::Vec<ColourSlot> {
    t.iter()
        .map(|&(at, len, rgb)| ColourSlot { at, len, rgb })
        .collect()
}

#[test]
fn the_compiled_tables_are_the_committed_ones() {
    assert_eq!(p1::DIACRITIC_SHIFT, p0::DIACRITIC_SHIFT);
    let n = [
        same("kitty SEEDS", &kitty::SEEDS, &committed_kitty::SEEDS),
        same(
            "kitty SLOTS",
            &kitty::SLOTS,
            &slots(&committed_kitty::SLOTS),
        ),
        same("kitty POOL", kitty::POOL, committed_kitty::POOL),
        same("x SEEDS", &x::SEEDS, &committed_x::SEEDS),
        same("x SLOTS", &x::SLOTS, &slots(&committed_x::SLOTS)),
        same("x POOL", x::POOL, committed_x::POOL),
        same("DIACRITICS", &p1::DIACRITICS, &p0::DIACRITICS),
        same(
            "DIACRITIC_INDEX",
            &p1::DIACRITIC_INDEX,
            &p0::DIACRITIC_INDEX,
        ),
        same(
            "DIACRITIC_BLOCKS",
            &p1::DIACRITIC_BLOCKS,
            &p0::DIACRITIC_BLOCKS,
        ),
    ];
    extern crate std;
    let line: std::vec::Vec<std::string::String> =
        n.iter().map(|(k, v)| std::format!("{k} {v}")).collect();
    std::println!("identical: {} elements", line.join(", "));
}
