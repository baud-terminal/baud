# The per-row used extent — how four terminals decide what to blank

> **Group:** F · **Sources:** `[[source: alacritty-row]]`, `[[source: alacritty-grid]]`, `[[source: vte-rowdata]]`, `[[source: ghostty-page]]`, `[[source: ghostty-screen]]`, `[[source: kitty-line]]` · **Status:** partial

## Why this note exists

Group F was declared when the corpus was set up and stood empty for the life of
the project. Meanwhile `CLAUDE.md` refused a per-row high-water mark **twice**,
both times on the strength of a `baud-bench` number that has since turned out
to be noise, and both times with the same objection: *sixteen write sites to
maintain an invariant, for a saving that cannot be shown.*

The saving can now be shown — blanking is **51-61%** of the cells path on
scrolling text. So the objection reduces to the invariant, which is a design
question, and the design question has been answered four times in production.

This note is what those four do. It is deliberately not an argument that baud
should copy any of them.

## The four

| terminal | per-row extent? | what it is | what it buys |
|---|---|---|---|
| alacritty | **yes** | `Row::occ: usize`, a high-water mark over a fixed-width row | `reset` writes `occ` cells, not `cols` |
| VTE | **yes** | `VteRowData.len: guint16`, storage truncated at the last written cell | a short row is not stored at all |
| ghostty | no | per-row *boolean* summaries (`styled`, `grapheme`, `hyperlink`) | skips per-cell refcount work; still memsets the full range |
| kitty | no | `Line` carries `xnum` and nothing else | — |

Two of four do it. That is the honest shape of the finding: this is a real
design choice, not a universal, and a note that only cited the two would be
making the case rather than reporting it.

## alacritty: one write site, not sixteen

`[[source: alacritty-row]]` defines it:

> "Maximum number of occupied entries. This is the upper bound on the number of
> elements in the row, which have been modified since the last reset. **All
> cells after this point are guaranteed to be equal.**"

The invariant objection dissolves because `occ` is not maintained at the write
sites at all — it is maintained in **`IndexMut`**:

```rust
impl<T> IndexMut<Column> for Row<T> {
    fn index_mut(&mut self, index: Column) -> &mut T {
        self.occ = max(self.occ, *index + 1);
        &mut self.inner[index.0]
    }
}
```

with the same one line in the `Range`, `RangeTo` and `RangeFrom` impls. A write
site cannot forget, because a write site cannot obtain a `&mut` cell without
going through the operator that bumps the mark. Sixteen call sites, one place
that maintains the invariant.

baud already has the identical lever and `CLAUDE.md` already argues for it, in
another section and about another quantity: `RowRot` is maintained inside
`View::phys` so that the rotation is "one line rather than twenty-six chances
to forget it". A used extent maintained inside `View`'s mutable accessors is
the same move.

**The mark is monotone**, which is the second half of why it is cheap: `occ`
only ever grows between resets. Nothing has to notice that a cell became blank
again. An extent that is too large is slow; only one that is too small is
wrong, and `max` cannot produce one.

## The part that kills baud's scan, and how alacritty avoids it

baud tried **scan-before-blank** twice and it lost both times, most recently at
-24% with `cells_blanked` coming out *identical*. The cause is that the blank
carries the current style: a row full of cells left by a *previous* blank does
not compare equal to the blank in force now, so on any payload that emits SGR
the scan runs to the end of every row and saves nothing.

`[[source: alacritty-row]]`'s `reset` has exactly this problem and solves it in
O(1):

```rust
let len = self.inner.len();
if self.inner[len - 1].discriminant() != template.discriminant() {
    self.occ = len;
}
for item in &mut self.inner[0..self.occ] { item.reset(template); }
self.occ = 0;
```

One comparison — the **last** cell against the template — decides whether the
whole row needs rewriting. If the template changed, `occ` is forced to the full
width and the row is reset entirely; otherwise only the occupied prefix is.
That is the piece baud's scan was missing: a style change is detected by a
single load, not by a comparison per cell.

`[[source: alacritty-grid]]` puts `reset` on the scroll path
(`Grid::scroll_up`, "when rotating the entire region with fixed lines at the
top, just reset everything"), which is the same position as baud's
`fill_row_counted`.

## VTE: the stronger form

`[[source: vte-rowdata]]` does not mark a fixed row; it shortens the row:

```c
typedef struct _VteRowData {
	VteCell *cells;
	guint16 len;
	VteRowAttr attr;
} VteRowData;

static inline const VteCell *
_vte_row_data_get (const VteRowData *row, gulong col)
{
	if (G_UNLIKELY (row->len <= col))
		return NULL;
	return &row->cells[col];
}
```

A read past `len` returns `NULL` and the caller substitutes a blank. Trailing
blanks are not stored, so blanking a row is `len = 0`.

This is closer to baud's *span* representation than to its cell grid, and it is
worth naming for that reason: baud's `OpMode::Runs` already gets this effect
for free — after the startup clear, a scrolled row equals the blank and nothing
is written at all. What the extent would buy baud is the `Computed` path, which
is the one a renderer reads.

## ghostty: conservative summaries, and what they are *not* for

`[[source: ghostty-page]]` carries a per-row flag whose comment is the clearest
statement of the discipline in any of the four:

> "True if any of the cells in this row have a ref-counted style. **This can
> have false positives but never a false negative.** […] Why have this weird
> false positive flag at all? This makes VT operations that erase cells (such
> as insert lines, delete lines, erase chars, etc.) MUCH MUCH faster in the
> case that the row was never styled. At the time of writing this, the speed
> difference is **around 4x**."

The clearing rule is equally instructive: a flag is cleared only when
`cells.len == page.size.cols`, i.e. only when the operation is known to have
covered the whole row; otherwise the page recomputes it. Never guess downward.

But ghostty is **not** prior art for shortening the write. `[[source:
ghostty-screen]]`'s `clearCells` ends, after all the flag handling, with

```zig
@memset(cells, self.blankCell());
```

over the full requested range. The flags exist to skip **reference counting**,
which is a cost baud does not have — its styles are an interned table indexed
by a `StyleId`, with no refcount to release. So ghostty's 4x is not a number
baud can expect to inherit, and citing it as if it were would be the
cross-implementation version of comparing across machines.

## What this changed for baud

**Implemented.** `RowExtent` is the mark, `Grid::fill_row` reads it, and the
result is less-pager **+37%**, cat-scroll **+16%**, git-log **+6%**, with
cat-scroll's write amplification going 6.1x to 1.9x. `MEASUREMENTS.md` carries
the table.

Three things this note got right, and one it did not.

**Right: the invariant is maintainable in one place.** The marks live in
`View`, so every mutator bumps them and the line moves permute rows and marks
in the same loop. `Term::viewport` is the single site outside `Grid` that
changes which physical row holds which content, and it rotates the marks with
the rows.

**Right: alacritty's one-cell template test is what makes it work.** baud's two
scan attempts lost at -24% and -66% with identical `cells_blanked`; a single
load of the row's last cell has no such failure mode.

**Wrong, and it is the interesting part: "it records where text reached" is not
the invariant.** The mark records **where the row stops being uniform**, and
the two differ. `ECH` at the last column writes a *styled* blank: no glyph, but
it does not match the default blanks in front of it. Treating the mark as a
glyph position let it fall to zero there, leaving the row non-uniform behind
its own mark, so the next full clear skipped the row.
core2's cleared-screen property caught it at 20,000 cases. The rule that
holds is that a fill running to the end of a row sets the mark to exactly where
that fill started — which *raises* it when the fill starts late, and no
description of the mark as "how far the text goes" would ever suggest that.

**The cost was underestimated.** This note treated the saving as the only
question. Maintaining the mark with nothing reading it measures **-7 to -8%**:
`View` gains a sixth field and every write touches a second array. On
`payload::streaming` that exactly cancels the 16% of blanking it skips, so the
synthetic payload measures flat.

## What it removed, which was the point

The reason the change was refused:

* The invariant is maintainable in **one** place, not sixteen, and baud already
  uses that pattern for `RowRot`.
* A monotone `max` cannot produce the dangerous error, only the slow one.
* The style-carrying blank, which defeated the scan, is handled by **one**
  comparison of the last cell rather than a scan.

What remains genuinely open is whether it pays *here*. baud's `fill_row` is a
`slice::fill` that lowers to vector stores over 1600 contiguous bytes; the
saving is the shortened store minus a load of `occ` and a compare, and 60 of
200 columns used is the headroom core2's row-extent census measured.
That is a measurement to make, and the ceiling for it is already known: the
`ceiling-no-blank` arm says the whole of blanking is worth 51-61%, so a mark
that eliminates 70% of it is worth up to about 40%.

## Claims relied on

| claim | source | where |
|---|---|---|
| `occ` is a high-water mark; all cells after it are guaranteed equal | `[[source: alacritty-row]]` | `Row::occ` doc comment |
| It is maintained in `IndexMut`, not at the write sites | `[[source: alacritty-row]]` | the four `IndexMut` impls |
| A changed template forces a full reset via one comparison of the last cell | `[[source: alacritty-row]]` | `Row::reset` |
| `reset` is called on the scroll path | `[[source: alacritty-grid]]` | `Grid::scroll_up` |
| A VTE row stores only `len` cells; reads past it return NULL | `[[source: vte-rowdata]]` | `_VteRowData`, `_vte_row_data_get` |
| ghostty's per-row flags may have false positives, never false negatives, and are worth ~4x on erase | `[[source: ghostty-page]]` | `Row.styled` doc comment |
| A flag is cleared only when the operation covered the full row width | `[[source: ghostty-page]]`, `[[source: ghostty-screen]]` | `clearCells` |
| ghostty memsets the full range regardless | `[[source: ghostty-screen]]` | `clearCells`, final statement |
| kitty's `Line` carries no used extent | `[[source: kitty-line]]` | `Line` struct |

## Gaps and open questions

- **No measurement.** Everything here is a reading of other people's code. The
  ceiling is measured (51-61%); the mark itself is not implemented.
- **ghostty's 4x is not transferable** and is quoted only as evidence that
  per-row summaries are worth maintaining, not as a number baud would see.
- **Neither alacritty nor VTE publishes a measurement** for their extent. Both
  are load-bearing production code, which is evidence of a different kind, but
  it is not a number and should not be quoted as one.
- **The interaction with `RowRot` is unexamined.** baud's rows move by rotating
  an index; an extent is per *physical* row and would have to rotate with it,
  or be indexed the same way `nrun` and `promoted` already are.
- **Selection, reflow and search** all read past the extent by nature, and
  `CLAUDE.md` already lists them as unmeasured. VTE's NULL-return convention is
  the shape of the answer; baud has not needed one yet.
- **Not read:** alacritty's history for why `occ` was added, and whether it was
  ever measured. The code is pinned; the rationale is not in it.
