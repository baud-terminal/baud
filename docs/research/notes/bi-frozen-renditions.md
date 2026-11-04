# BI. A full rendition table freezes what lies off the screen

**What changed.** A full rendition table used to evict a slot cells still named, and those cells took another rendition's colours. `[[source: dec-std-070]]`'s SGR, page 5-104, note 3: "In no way does the selection of current rendition affect the rendition of characters already entered into the display"; `[[source: itu-t416]]` § 8.1.7 gives each character its own foreground and background. Now the table holds the active screen. Everything off the screen is frozen (`baud-core/src/freeze.rs`, `baud-domain/src/frozen.rs`) when a sweep leaves the table past 7/8 load: the history, the rows a shorter window cut, and the screen not up. Frozen rows are rewritten to dense ids into generations of their own words, in a caller-lent pool. A row finds its generation from a tag one a ring row, believed only while the row still holds the tagged line where it stood. The line feed writes nothing new.

**The bound.** The table is sized to a screen of cells, the status row and three reserved slots: 61,200 + 408 + 3 = 61,611 of 65,536 at 400x150 on a 408 pitch. The pool is one word a ring cell, one a generation record, and one table: 944,888 words, 18.9 MB at the app's 2,150-row ring, from zeroed pages. No pool lent (`Owned::without_frozen`, or an embedder lending none) means no freeze. Neither does a screen of more than 65,536 less its pitch less three cells (65,125 at the 408 pitch) that names more renditions than the table holds. In both cases the next rendition is refused, counted in `Reclaims::refusals`, the text after it in the default, and no drawn cell changes. That is `[[source: ecma48]]` § 8.3.117: "The usable combinations of parameter values are determined by the implementation." What the text after a refused rendition takes is the implementation's choice there: the default is built, and keeping the rendition in effect before the refused SGR is the other reading, held open (below). Neither is reached at the app's provisioning.

**What moves rows across the boundary.** Each move is held by a test in `baud-core/tests/frozen.rs`, a differential against a 65,536-slot reference that never freezes. Every scenario must name more renditions than the 1,024-slot table holds.
- `?47`, `?1049` and the switch back thaw the incoming screen into the table.
- A reverse scroll clears the tags of the row it takes back and the row it pushes out; at line zero it renumbers every tag.
- The prompt's retreat clears the rows it takes back.
- A taller window thaws the rows it brings back.
- A re-cut first rewrites every line spanning two tables into one generation. A line too big for one is cut where its table changes, counted (`lines_split`).
- `ED 3` and `RIS` free their generations at once.
- The string table's sweep and kitty placeholders read a frozen row through its generation. That was a live bug: a frozen row's SDS/SRS string could be freed and reused.

**Readers.** `Viewport::styles`, `Scrollback::styles`, `Term::line_styles` and `Term::style_in` give a row's table. Every draw path draws a copy of its window, and each copy gives a frozen cell a slot of the frame's resolved table that no live cell of the frame names (`baud-render/src/frozen.rs`), put back before the table is brought up to date again: the cell path's set, and the CPU and instance paths' own row buffer, which the Linux driver draws. The shader is unchanged. A lent id means what it means only under its lending, so the table's stamp carries the lending's number, the same number for the same slots given for the same ids; a frame lent otherwise than the last repaints every row, since a scroll of one line can leave the copy's lent ids as they were and the words under them not.
- A reader of a word reads it where its id names it (`Words`): a stored row through its own table, a display line through each run of the stored lines it shows, a lent copy through the words its slots were given. So do the picture finder, a cell's marks under SAPV 16, SDS and SRS strings, SAPV's variants and slants, a composition's base and the trail's grounds. A joined letter's word keeps each frozen cell's word (`WordCell::word`).
- A window holding frozen rows is re-broken at its words as any other: the display records each line's runs of stored lines (`Display::line_sources`), and the lending and the layout read each cell through its own row's table.
- A double-width line the cell path converts from its lent copy is lent the frame's slots and words too (`Renderer::set_frame_lent`), and the leaving row of a smooth scroll is lent slots of its own; both are converted as the reference converts them (`frozen_rows.rs`).
- A freeze and a re-cut keep the default at id 0, so a blank cell stays `PackedCell::EMPTY` and a line reads to its mark (`a_recut_keeps_a_blank_cell_blank`).
- Copy and search read no rendition.

**Open.**
- The rendition the text after a refused one takes: the default, as built, or the one in effect before the refused SGR. `[[source: ecma48]]` § 8.3.117 leaves it to the implementation, and the review that weighed the two did not settle it.
- A lam-alef pair a soft wrap parts keeps its other letter's cell, and reads that cell's marks through the live table: wrong only for SAPV 16 on a frozen row.
- A copy in drawn order (`?96`) of a re-broken paragraph reads SDS and SRS strings through the live table, its rows handed over without their tables.
- The cell path's frame build, its forgetting a set's copy and its lending the leaving row before the map is walked, is held by the render tests of the pieces it calls, not by a test of the app's frame.

**Write amplification and working set.** The byte loop's only addition is one test in `unretire`, on the reverse scroll: of an `Option` with no pool lent, and of the generation count with one, as the app lends it. Once a generation exists a reverse scroll also clears two tags. A freeze rewrites each frozen cell's eight bytes once, plus 20 bytes a new word. The tags are 24 bytes a ring row. Nothing new is touched per byte while no generation exists. The renderer's lending runs once a frame, and only for a frame that shows a frozen cell; a frame that shows none asks one test of the window (`Orders::window_frozen`).

**ingest2, 200x50, with a pool lent** (`BAUD_FROZEN=1`), as the app lends one, though to ingest2's own 1,024-slot table and not the app's 65,536: the arm prices the lent pool's tests on the byte loop, not the app's table size. Against the base built from the commit before this one and a copy of the change, the change and its copy byte-identical (`cmp`). Each built at one path from `git archive`, touched. Six rotated rounds, round 1 dropped, load 26-29. `cells_written` was equal on every arm. MB/s, median [range], and instructions retired per rep, the whole process's over its reps:

| | base | change | copy | instructions a rep, base / change / copy |
|---|---|---|---|---|
| asciiquarium | 495.4 [491.1-497.4] | 499.6 [494.9-499.7] | 498.1 [496.3-499.6] | 19,986,303-19,988,776 / 19,985,124-19,992,876 / 19,985,576-19,992,198 |
| btop | 855.5 [852.8-861.0] | 857.6 [853.0-860.6] | 853.8 [853.0-856.4] | 17,226,541-17,231,604 / 17,228,292-17,230,966 / 17,227,437-17,231,914 |
| cat-cjk | 507.0 [498.5-508.1] | 500.2 [497.9-507.0] | 501.9 [494.9-506.0] | 19,143,346-19,145,873 / 19,145,135-19,148,490 / 19,145,067-19,148,408 |
| vim | 581.5 [579.1-585.3] | 582.4 [578.2-583.9] | 580.8 [574.4-583.0] | 7,871,635-7,872,697 / 7,871,887-7,873,270 / 7,872,517-7,873,752 |
| less-pager | 1,360.9 [1,355.6-1,369.0] | 1,358.2 [1,345.0-1,366.4] | 1,366.3 [1,355.6-1,371.7] | 676,081-676,396 / 676,289-676,483 / 676,282-676,654 |

- Every range overlaps base's, in MB/s and in instructions: no result.
- The feed loops' disassembly differs by more than struct offsets. With immediates and comments normalised, `timed` (8,945 instructions on both arms) differs on 370 lines, and `feed_read_outlined` (8,881 against 8,865) on 158. What shows no cost is the instruction count per rep.
- None of the recordings fills the table, so this prices the lent pool's tests, not a freeze; a freeze's cost is the captures' below.

**The captures** (chafa 1.18.3, at the app's 400x150 provisioning, in 4096-byte reads):

| | refused, with / without generations | words at the peak | max RSS, with / without |
|---|---|---|---|
| C1 | 0 / 163,691 | 303,912 (6.1 MB) | 31.6 / 25.6 MB |
| C3 | 0 / 3,838,493 | 944,663 (18.9 MB) | 192.6 / 173.7 MB |
| C5 | 0 / 11,727 | 38,247 (0.8 MB) | 17.3 / 16.5 MB |

- C3 froze 1,175 times, freed 1,112 generations and compacted the pool 6 times, ingesting in 1.26 s against 5.19 s refusing.
- The words at the peak and the refusals are this build's too; the RSS is the build's before the renderer's lending, whose core differs from this one only in the default's id, which keeps the words alike.

**Against alacritty and sfte.** Each capture fed in its own reads to a storage provisioned as the app provisions it (`Owned::with_geometry` at 400x150, the pool lent), and compared at 17 points through the run with alacritty_terminal 0.26 at the capture's window: every cell's foreground and background, read through its row's table, on the screen and on every retained line.

| | screen, wrong | history, wrong | history through the live table alone |
|---|---|---|---|
| C1 (160x45) | 0 of 122,336 | 0 of 2,297,192 | 1,782,846 |
| C2 | 0 of 1,019,735 | 0 of 2,161,013 | 1,736,270 |
| C3 | 0 of 1,019,445 | 0 of 10,464,514 | 10,177,944 |
| C4 | 0 of 959,950 | 0 of 193,077 | 137,658 |
| C5 | 0 of 1,019,964 | no history kept | none |

- The last column is the control: the frozen history read without its generations. With no pool lent, C1 reads 21,161 screen and 1,152,146 history cells wrong, each after a refused rendition.
- Against sfte at its fixed 200x50, through each row's table, at every 256th of the reads: C1 and C2 0 wrong of 2,570,000 and 2,600,000 cells. C3, C4 and C5 differ on 499,544, 576,215 and 557,344 cells, in the background alone, the character the same; of C3's, 1,326 are an explicit black that sfte reads as its default. The base differs on 499,579, 576,215 and 557,344 there, and on 51 and 45 for C1 and C2, its evictions: the three are not the freeze's.

**What would have proven it wrong:** a wrong cell in a differential, a capture or a draw path's frame, a refusal with a pool lent, or a disjoint ingest2 range with its A/A pair overlapping. None happened.
- The base's eviction, put back as a perturbation with the freeze off, fails all 21 tests in `frozen.rs`: 12 on a cell read in another rendition, and the other 9 on their controls (nothing froze, or nothing refused).
- The renderer's tests that compile without the fix (the three in `frozen_cpu.rs`, the three in `frozen_words.rs` and the first in `frozen_pictures.rs`) fail on the core's freeze without the renderer's readers, all seven. The three that name what the fix adds (`frozen_pictures.rs`'s lent copy and `frozen_rows.rs`'s two sized lines) fail under the perturbations below. Each of nine perturbations fails its own tests: the CPU path lending nothing, the damage blind to the lending's number, a display line read through one row's table, marks, strings and pictures read through the live table, the lent words dropped, a sized line not lent the frame's slots, and the leaving row not lent its own (`frozen_rows.rs`).
- `frozen_properties.rs` passes 20,000 cases; of a fixed 200, 153 froze and 15 thawed a frozen row. Its first run at 20,000 found a re-cut giving the default a new id, so a blank cell past a line's text was no longer blank and the line read to the window's width: pinned by `a_recut_keeps_a_blank_cell_blank`.
