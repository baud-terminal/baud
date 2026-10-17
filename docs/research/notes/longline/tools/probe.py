import sys
root = sys.argv[1]
def edit(path, pairs):
    p = root + "/" + path
    s = open(p).read()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:80], s.count(old))
        s = s.replace(old, new)
    open(p, "w").write(s)

# Counter table in baud-domain.
open(root + "/baud-domain/src/probe.rs", "w").write('''//! Scratch probe: counts of the work past the live edge.
use core::sync::atomic::{AtomicU64, Ordering};
pub const N: usize = 32;
pub static C: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
#[inline(always)]
pub fn add(i: usize, n: usize) { C[i].fetch_add(n as u64, Ordering::Relaxed); }
pub fn take() -> [u64; N] { core::array::from_fn(|i| C[i].swap(0, Ordering::Relaxed)) }
pub const NAMES: [&str; N] = [
 "copy_calls","copy_pairs","copy_cells","il_calls","dl_calls","ildl_cells",
 "repair_open_clear_cells","swept_paint_calls","swept_paint_rows","swept_paint_cells",
 "redraw_clears","swept_resize_calls","swept_resize_rows","swept_resize_cells",
 "incoming_calls","incoming_cells","erase1_calls","erase1_rows_cleared","erase1_cells",
 "eraseN_calls","eraseN_rows_cleared","eraseN_cells","reach_lowered",
 "held_whole","held_scanned","clear_one_cell","fill_past_calls","fill_past_cells",
 "measure_calls","clear_at_rows_cleared","redraw_erase_calls","spare"];
''')
edit("baud-domain/src/lib.rs", [("mod pos;\n", "mod pos;\npub mod probe;\n")])

edit("baud-domain/src/grid.rs", [
 # fill_past_edge
 ("        self.used[p] = self.mark_after_fill(base, to, cell, RowExtent::of(a));\n        if a >= to {\n            return 0;\n        }\n",
  "        self.used[p] = self.mark_after_fill(base, to, cell, RowExtent::of(a));\n        if a >= to {\n            return 0;\n        }\n        if to > self.cols.index() {\n            crate::probe::add(26, 1);\n            crate::probe::add(27, to - a.max(self.cols.index()));\n        }\n"),
 # copy_past_edge
 ("        let mut n = 0;\n        for y in (top.index() + 1..=bottom.index()).rev() {\n",
  "        let mut n = 0;\n        crate::probe::add(0, 1);\n        for y in (top.index() + 1..=bottom.index()).rev() {\n"),
 ("        self.cells.copy_within(a * s + w..a * s + end, b * s + w);\n        past.set(b, from);\n",
  "        self.cells.copy_within(a * s + w..a * s + end, b * s + w);\n        crate::probe::add(1, 1);\n        crate::probe::add(2, end - w);\n        past.set(b, from);\n"),
 ("        let n = (n.get_raw() as usize).min(bot + 1 - cy);\n        let mut written = 0;\n        for y in (cy + n..=bot).rev() {\n",
  "        let n = (n.get_raw() as usize).min(bot + 1 - cy);\n        let mut written = 0;\n        crate::probe::add(3, 1);\n        for y in (cy + n..=bot).rev() {\n"),
 ("        let n = (n.get_raw() as usize).min(bot + 1 - cy);\n        let mut written = 0;\n        for y in cy..bot + 1 - n {\n",
  "        let n = (n.get_raw() as usize).min(bot + 1 - cy);\n        let mut written = 0;\n        crate::probe::add(4, 1);\n        for y in cy..bot + 1 - n {\n"),
 # held_past_edge
 ("        let mut held = w;\n        let mut count = 0;\n        for p in 0..self.ring.index() {\n            let reach_p = match past.flags[p] {\n                PastEdge::Held => reach_of_row(self.cells, self.used, s, p, w, end),\n                PastEdge::Clear if w < was => match self.cells[p * s + was - 1] {\n",
  "        let mut held = w;\n        let mut count = 0;\n        crate::probe::add(28, 1);\n        for p in 0..self.ring.index() {\n            let reach_p = match past.flags[p] {\n                PastEdge::Held => {\n                    let r = reach_of_row(self.cells, self.used, s, p, w, end);\n                    let row = &self.cells[p * s + w.min(end)..p * s + end];\n                    let scanned = match row.iter().rposition(|c| *c != PackedCell::EMPTY) {\n                        Some(k) => row.len() - k,\n                        None => row.len(),\n                    };\n                    crate::probe::add(23, 1);\n                    crate::probe::add(24, scanned);\n                    r\n                }\n                PastEdge::Clear if w < was => match { crate::probe::add(25, 1); self.cells[p * s + was - 1] } {\n"),
 # clear_past_edge_at
 ("        if end <= from {\n            return 0;\n        }\n        self.cells[p * s + from..p * s + end].fill(PackedCell::EMPTY);\n        end - from\n",
  "        if end <= from {\n            return 0;\n        }\n        crate::probe::add(29, 1);\n        self.cells[p * s + from..p * s + end].fill(PackedCell::EMPTY);\n        end - from\n"),
])

edit("baud-core/src/ops.rs", [
 # repair: opened row clear
 ("                moved += self.view.clear_past_edge(bot, reach, &mut past);\n",
  "                let c = self.view.clear_past_edge(bot, reach, &mut past);\n                baud_domain::probe::add(6, c);\n                moved += c;\n"),
 # paint clear_swept
 ("        let w = self.view.clear_swept_past_edge(n, reach, &mut past);\n        self.cells = self.cells.plus(w);\n",
  "        let w = self.view.clear_swept_past_edge(n, reach, &mut past);\n        baud_domain::probe::add(7, 1);\n        baud_domain::probe::add(8, n.index());\n        baud_domain::probe::add(9, w);\n        self.cells = self.cells.plus(w);\n"),
 # incoming
 ("        let n = self.view.clear_past_edge(r, reach, &mut past);\n        self.cells = self.cells.plus(n);\n        self.settle_reach();\n",
  "        let n = self.view.clear_past_edge(r, reach, &mut past);\n        baud_domain::probe::add(14, 1);\n        baud_domain::probe::add(15, n);\n        self.cells = self.cells.plus(n);\n        self.settle_reach();\n"),
 # erase single
 ("        let n = self.view.clear_past_edge_of(at, reach, &mut past, blank);\n        self.cells = self.cells.plus(n);\n        self.settle_reach();\n",
  "        let n = self.view.clear_past_edge_of(at, reach, &mut past, blank);\n        baud_domain::probe::add(16, 1);\n        baud_domain::probe::add(17, (n > 0) as usize);\n        baud_domain::probe::add(18, n);\n        self.cells = self.cells.plus(n);\n        self.settle_reach();\n"),
 # erase rows
 ("            n += self\n                .view\n                .clear_past_edge_of(rot.advance(y, ring), reach, &mut past, blank);\n        }\n",
  "            let c = self\n                .view\n                .clear_past_edge_of(rot.advance(y, ring), reach, &mut past, blank);\n            baud_domain::probe::add(20, (c > 0) as usize);\n            n += c;\n        }\n        baud_domain::probe::add(19, 1);\n        baud_domain::probe::add(21, n);\n"),
 # settle reach
 ("        if self.book.held.index() == 0 {\n            self.book.reach = self.cols;\n        }\n",
  "        if self.book.held.index() == 0 {\n            if self.book.reach.index() > self.cols.index() {\n                baud_domain::probe::add(22, 1);\n            }\n            self.book.reach = self.cols;\n        }\n"),
 ("        let w = match way {\n", "        let w = match way {\n"),
])
edit("baud-core/src/ops.rs", [
 ("        self.cells = self.cells.plus(w);\n        self.settle_reach();\n    }\n\n    /// Whether the primary's rows may hold anything past the live edge",
  "        baud_domain::probe::add(5, w);\n        self.cells = self.cells.plus(w);\n        self.settle_reach();\n    }\n\n    /// Whether the primary's rows may hold anything past the live edge"),
])
edit("baud-core/src/prompt.rs", [
 ("            if self.past_edge() {\n                self.clear_swept();\n            }\n",
  "            if self.past_edge() {\n                baud_domain::probe::add(10, 1);\n                self.clear_swept();\n            }\n"),
 ("            if self.past_edge() {\n                self.clear_erased_past_edge(at, blank);\n            }\n",
  "            if self.past_edge() {\n                baud_domain::probe::add(30, 1);\n                self.clear_erased_past_edge(at, blank);\n            }\n"),
])
edit("baud-core/src/term.rs", [
 ("        let n = book.top.since(book.swept).unwrap_or(Rows::ZERO);\n        book.swept = book.top;\n        self.cells_written += View::new(",
  "        let n = book.top.since(book.swept).unwrap_or(Rows::ZERO);\n        book.swept = book.top;\n        baud_domain::probe::add(11, 1);\n        baud_domain::probe::add(12, n.index());\n        let before = self.cells_written;\n        self.cells_written += View::new("),
 ("                held: &mut book.held,\n            },\n        );\n    }\n\n    /// Take a new window size.",
  "                held: &mut book.held,\n            },\n        );\n        baud_domain::probe::add(13, self.cells_written - before);\n    }\n\n    /// Take a new window size."),
])

# second pass: fill_row's and fill_at's cold half; the live screen's zone.
edit("baud-domain/src/grid.rs", [
 ("        let to = self.used[p].index();\n        #[cfg(not(feature = \"ceiling-no-rowfill\"))]\n        self.cells[base..base + to].fill(cell);\n",
  "        let to = self.used[p].index();\n        if to > self.cols.index() {\n            crate::probe::add(26, 1);\n            crate::probe::add(27, to - self.cols.index());\n        }\n        #[cfg(not(feature = \"ceiling-no-rowfill\"))]\n        self.cells[base..base + to].fill(cell);\n"),
])
