# ceiling: no per-scroll damage marking

**A ceiling arm, not a fix, and applied only when `SFTE_CEILING_NO_DIRTY_ROWS`
is set in the environment at build time.** `build.rs` skips any patch whose
name starts with `ceiling-` unless that variable names it, so the default
build never carries it.

**What upstream does.** sfte's grid-scroll routine ends by calling
`_sfte_grid_dirty_rows(ctx, top, bot)` over the whole scrolling region,
whatever `lines` was. For a full-screen one-line scroll that is `dirty = 1`
written into every cell of all 50 rows -- 10,000 scattered byte stores at
stride 36, touching 360 KB -- on every newline that reaches the bottom row.
`cat-scroll.raw` scrolls once per ~80 bytes, so that is ~4.5 KB of dirty-byte
traffic per byte ingested, and `sample` puts that routine at 95% of
sfte's self time on that recording. `btop.raw`, which never scrolls, ingests
30x faster on the same build.

**What the arm does.** Makes that one call a no-op. Nothing else changes: the
rotation, the scrollback push and the blank of the incoming rows all still
run, so **every cell is byte-identical to the base build** -- which the
oracle's screen digest proves rather than assumes -- and only the *render*
damage sfte would report is wrong. That is what makes it a ceiling arm in
this repository's sense: it removes exactly one named component and its
counters say what it broke.

**What it prices.** The share of sfte's scroll-heavy throughput that is its
damage model rather than its parser or its cell write. baud computes row
damage at frame time by comparing cell words, so its ingest pays nothing for
this; the arm is what makes the two comparable on a scrolling recording.

## anchor

```c
        _sfte_grid_clear_rows(ctx, top, lines);
    }

    _sfte_grid_dirty_rows(ctx, top, bot);
}
```

## replacement

```c
        _sfte_grid_clear_rows(ctx, top, lines);
    }

    /* baud ceiling arm (baud-sfte-difftest/patches/ceiling-no-dirty-rows.md):
       the per-scroll damage marking is removed. Cells are unchanged; only the
       render damage sfte would report is wrong. */
    (void)bot;
}
```
