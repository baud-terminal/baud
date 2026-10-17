//! Scratch probe: counts of the work past the live edge.
use core::sync::atomic::{AtomicU64, Ordering};
pub const N: usize = 40;
pub static C: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
#[inline(always)]
pub fn add(i: usize, n: usize) {
    C[i].fetch_add(n as u64, Ordering::Relaxed);
}
pub fn take() -> [u64; N] {
    core::array::from_fn(|i| C[i].swap(0, Ordering::Relaxed))
}
pub const NAMES: [&str; N] = [
    "copy_calls",
    "copy_pairs",
    "copy_cells",
    "il_calls",
    "dl_calls",
    "ildl_cells",
    "repair_open_clear_cells",
    "swept_paint_calls",
    "swept_paint_rows",
    "swept_paint_cells",
    "redraw_clears",
    "swept_resize_calls",
    "swept_resize_rows",
    "swept_resize_cells",
    "incoming_calls",
    "incoming_cells",
    "erase1_calls",
    "erase1_rows_cleared",
    "erase1_cells",
    "eraseN_calls",
    "eraseN_rows_cleared",
    "eraseN_cells",
    "reach_lowered",
    "held_whole",
    "held_scanned",
    "clear_one_cell",
    "fill_past_calls",
    "fill_past_cells",
    "measure_calls",
    "clear_at_rows_cleared",
    "redraw_erase_calls",
    "widened_fills",
    "widened_cells",
    "row_fills",
    "row_cells",
    "at_fills",
    "at_cells",
    "x",
    "x",
    "spare",
];
