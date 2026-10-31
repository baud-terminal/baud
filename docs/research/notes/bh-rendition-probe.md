# BH. A full rendition table's probe stops at its bucket's reach

**Status: done 2026-10-31.** baud no longer falls behind chafa printing
pictures into the shell: chafa wrote 138 thumbnails in about 1.2 seconds,
baud took 8.4 to 9.8 seconds to ingest them, and takes 0.25 to 0.40 now.
Every cell shows what it showed before.

**The cause.** A linear probe stops at its rendition or at an empty slot.
A table whose cells name more renditions than it has slots holds no empty
slot, only renditions and tombstones, so every new colour read all 65,536
slots at the app's provisioning; `sample` put 95% of that ingest in
`apply_sgr`, where the probe is inlined.

**The fix.** Each slot has a two-byte `Reach` beside it, in the caller's
memory with the table: how far past that bucket its farthest rendition
lies. `Paint::write_slot` raises it and `Paint::sweep_styles` sets it again
from the renditions it keeps. A probe that has read that far has proved a
miss, and `Paint::intern_far` takes the slot the walk to the table's end
took. The inline probe reads at most `SweepBook::INLINE` (64) slots, so a
hit that near its bucket loads nothing new. The table grows 128 KB at
65,536 slots, 8% of its 1.5 MB. One bound for the whole table reached
64,843 slots on the thumbnails and left the ingest at 8.3 s; on their final
table a bound per 64 buckets would read 17,213 slots a miss on average, and
one per bucket 426.

| | base | change |
|---|---|---|
| 138 thumbnails into a 160x45 shell, ingest, 4 runs | 8,376-9,804 ms | 245-401 ms |
| 512 new renditions on a full 1,024-slot table, slots read by probes past 64 (the base with the counter added) | 537,600 | 34,660 |

On five chafa captures the table, every retained cell's slot, the writes,
the reclaims and the cells that differ from alacritty_terminal are as
before (69 of 7,200 on screen and 82,047 of 163,553 in the history of the
thumbnails). A bound one short changes them, so the comparison can fail.

`ingest2`, five rotated rounds with the first dropped, 1,000 reps, cells
written equal on every arm, MB/s:

| | base | copy of base | change |
|---|---|---|---|
| cat-scroll | 1457.5 [1440.2-1520.9] | 1446.4 [1365.1-1483.7] | 1486.9 [1440.2-1497.8] |
| vim-unicode | 557.5 [544.9-572.0] | 564.0 [550.5-574.7] | 549.7 [540.5-557.4] |
| btop | 831.5 [824.0-852.3] | 838.6 [818.9-854.6] | 840.2 [828.5-862.8] |
| less-pager | 1318.2 [1296.9-1371.7] | 1334.8 [1268.3-1382.8] | 1314.3 [1294.6-1369.0] |
| asciiquarium | 471.0 [470.4-484.3] | 473.1 [461.3-480.4] | 476.4 [468.2-480.8] |

Every range overlaps: flat. Instructions a rep against the base, on the
first and last rounds: asciiquarium -0.5%, cat +0.1 to +0.4%,
btop -0.2 to -0.4%, vim-unicode -0.0 to -0.4%, less-pager within the
copy's spread. The timed build differs from this commit's only in cold
functions, once branch targets that layout moved are set aside;
`Paint::apply_sgr` is the same instruction for instruction, branch targets
and immediates included. This commit's own run, under a load of 8 to 20,
overlaps on every recording. Tests:
`a_miss_on_a_full_table_reads_to_its_buckets_reach`,
`a_full_tables_renditions_are_found_however_far_they_lie`,
`a_sweep_keeps_every_kept_rendition_findable`.

**For embedders.** A `StyleTable` takes a `reach` as well, one
`Reach::ZERO` a slot with a new table, and baud-domain's `SweepBook` has a
`probes` field, so a struct literal of either adds one. Also new: `Reach`
and `SlotsRead` (both from baud-domain), `SweepBook::INLINE`,
`SweepBook::probed` and `Term::style_probes`.

**Open:** a colour plane took 18 to 30 ms on the thumbnails when it was
prototyped, a tenth of this. The overflow itself, cells recoloured once the
table is full, is unchanged.
