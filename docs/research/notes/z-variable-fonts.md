# Z. Variable fonts in baud-font

The owner's call, 2026-09-29: the plain JetBrains Mono styles and the Nerd
icon face now, then variable fonts, "the best for baud". This note scopes the
second half before any code: what the files need, where the deltas are
applied, what the oracle is, and in what order it lands.

## What JetBrains Mono's variable files carry

Both `JetBrainsMono[wght].ttf` (303,144 bytes) and
`JetBrainsMono-Italic[wght].ttf` (308,888), from the same v2.304 archive the
statics came from, read with fontTools:

| table | what it holds | needed |
|---|---|---|
| `fvar` | one axis, wght 100 / 400 / 800, eight named instances | yes |
| `avar` | wght remapped at 8 points (e.g. 0.5 -> 0.3) | yes |
| `gvar` | 1,382 and 1,370 glyphs with deltas; 2 shared tuples; no intermediate regions; 417 and 106 tuples leave points to be interpolated | yes |
| composite glyphs | 570 in each file, every one with deltas: a component's offset moves | yes |
| `HVAR` | 2 regions, 1 nonzero delta (the font is monospaced at 600) | yes |
| GDEF item variation store | behind 876 GPOS device tables: mark positions move with weight | yes |
| `STAT` | style names | no: nothing reads it |
| `MVAR` | absent | a not-built row |
| `cvar`, hinting | not read by baud | a not-built row |
| `CFF2` | absent | read since 2026-10-11, at any coordinates (`baud-font/src/cff2.rs`; the record, "CFF2 outlines are read, blends and all") |
| FeatureVariations in GSUB and GPOS | absent in both | a not-built row |

The chapters: `[[source: ms-opentype-otvaroverview]]`,
`[[source: ms-opentype-otvarcommonformats]]` (the tuple and item variation
stores), `[[source: ms-opentype-fvar]]`, `[[source: ms-opentype-avar]]`,
`[[source: ms-opentype-gvar]]`, `[[source: ms-opentype-hvar]]`.

## Where the deltas are applied: at build time for the embedded faces

baud is a compiled database with an indexing runtime: work moves into tables
built at compile time, and the runtime looks things up. The embedded faces
are known at compile time and need only four instances -- wght 400 and 700,
upright and italic -- so those are **baked at build time**: a build script
runs baud-font's own delta code over the embedded variable files and emits
the instances, and drawing them applies no deltas at runtime. A variable
font the user configures is not known at compile time, so it takes a
runtime path: deltas per glyph, on demand, into caller-provided scratch, as
HarfBuzz, ttf-parser and skrifa do -- baud-font is `no_std` with no
allocator, so baking a whole instance at load is not available.

Jev, 0.96, asked with baud's architecture and CLAUDE.md's rules as context.
A first ask without them offered only the two runtime shapes and picked
per-glyph (0.86); that answer is withdrawn because the question was
incomplete.

One detail was open until step 5 was built, and is decided there: what the
build step emits. Baking outlines and advances is a rewrite of `glyf`, `loca` and
`hmtx`; baking the 876 GPOS device deltas is a rewrite of GPOS value
records. The alternative for GPOS is to keep its variation store and read
it at runtime, which runs only when a run is shaped -- and the run cache
holds the answer.

A styled face from a variable file is a second `Face` over the same bytes
at other coordinates: upright at 400 as the regular face, upright at 700 as
Bold, italic at 400 and 700 as Italic and Bold Italic. Each is its own
`FaceIx`, so the atlas, the run cache, the window cache and the pair tables
keep them apart as they keep the static files apart.

Cell metrics stay those of the default instance: without `MVAR` there is
nothing to vary them by, and a grid's cell must not change with a style.

## The oracle

- **Outlines:** HarfBuzz through uharfbuzz, drawing each glyph with
  `set_variations` at wght 100, 400, 550, 700 and 800. It keeps fractional
  coordinates, which is what a rasteriser should see. fontTools' instancer
  rounds to integers and is a second check; the note on rounding is written
  when the code is.
- **Shaping:** the hb-shape fixtures extended with variation settings; at
  the default coordinates every existing fixture must stay identical, since
  every delta is then zero.
- **Not an oracle: the static files.** The variable file at 700 is not the
  static Bold point for point, and at 400 it is not even the static
  Regular: 732 of 1,743 glyphs differ, by up to 1,970 units of summed point
  distance. That is the size of a changed point order or removed overlaps
  rather than a different shape, which a raster comparison would settle.
  Either way, swapping the embedded statics for the variable files changes
  pixels, and the owner should see the difference before it lands.

## The tests to write first

- `avar` mapping at its segment edges and between them;
- a composite glyph, whose deltas move its components (its points are its
  components plus four phantom points);
- the four phantom points, and that `HVAR` wins over them when present;
- interpolation of untouched points: a contour with none touched, with one,
  and wrapping between a contour's last point and its first;
- the default coordinates leaving every outline, advance and shaping
  fixture exactly as it is.

## Order

1. `fvar` and `avar`: normalised coordinates on a `Face`. Built.
2. `gvar` with interpolation and composites: outlines. Built.
3. `HVAR`: advances. Built.
4. The item variation store: the 876 GPOS device tables. Built.
5. The build step that bakes the embedded instances. Built.
6. The embed swap, with the pixel difference shown to the owner first.

Each step lands with its own red-first tests and perturbations.

## What it is worth, stated honestly

Until faces can be named by family, the only consumer is the embed swap:
about 0.5 MB off the binary (two variable files, 612 KB, against four
statics, 1.1 MB), and every weight from 100 to 800 available. A user's own
variable font gets real bold only once a configured stack can name styled
faces, which waits on fonts by family name. Neither changes the owner's
order; both belong in the record.

## Step 1, built: normalised coordinates

`baud_font::var` reads `fvar`'s axes and `avar`'s segment maps, and
`Face::set_variations` normalises a setting into a fixed array of 2.14
coordinates on the face (sixteen axes; a font with more reads the rest at
their default), with no allocation. The arithmetic is the specification's
in 16.16, and its one open step -- how the two divisions round -- is
HarfBuzz's: to the nearest 16.16 value, a half rounded up
(`[[source: harfbuzz-12-3-0-ot-var]]`, `[[source: harfbuzz-12-3-0-avar]]`;
Jev, 0.99). Truncating instead differs by one 2.14 unit in 1,482 of the
fixture's values. ~~halves away from zero~~ -- this said so until step 3
found that HarfBuzz's `roundf` is its own `floorf (x + .5f)`; no fixture
value lands on a negative half, so the test could not tell, and step 3's
section records the correction.

The oracle is `tests/var/normalize.txt`: HarfBuzz 12.3.0's coordinate for
every wght value from 0 to 1000 in quarter units on the three committed
variable faces -- JetBrains Mono upright and italic, and Fira Code, whose
default sits at its minimum -- 12,003 values, written by
`scripts/var-oracle.py`. All agree. Three hand-built `avar` tables reach
what the real fonts cannot: a target past 1, a first record off (-1, -1),
and a map with no segment.

Thirteen perturbations, one a mechanism; twelve are red at their own
tests. The thirteenth is the clamp after the division, which exact integer
arithmetic cannot reach and which stays because the specification names it.


## Step 2, built: varied outlines

`baud_font`'s `gvar` module reads the table and moves one glyph's points by
its tuple variations; `glyf` walks the glyph, flattens its components and
draws the result through the same contour builder the default path uses.
Everything a glyph needs lives in the caller's scratch -- one slice of
`VarPoint`, sized by `Face::outline_need`, which walks the glyph the way the
draw does so the count is exact -- and the default instance and static
fonts never touch it.

**The rules are HarfBuzz's wherever the specification is silent**, read from
its source rather than recalled (`[[source: harfbuzz-12-3-0-gvar]]`,
`[[source: harfbuzz-12-3-0-var-common]]`, `[[source: harfbuzz-12-3-0-open-type]]`,
`[[source: harfbuzz-12-3-0-glyph]]`, `[[source: harfbuzz-12-3-0-composite-glyph]]`,
`[[source: harfbuzz-12-3-0-limits]]`):

* the scalar is computed in `f64` and handed on as `f32`, its tests in
  HarfBuzz's order -- a coordinate of 0 ends a tuple before its region is
  asked whether it is valid;
* deltas stay `f32`, accumulate across tuples that cover every point, and
  are flushed into the points before each sparse tuple; the default
  positions inference reads are captured once, at the first sparse tuple;
* a tuple with no point numbers of its own and no shared list covers every
  point; a shared-tuple index past the table scores 0; a zero-scoring tuple
  is skipped before its data is looked at; a 0xC0 delta run is 32-bit;
* each component record is one offset point and its own contour end, so
  nothing is inferred across components; a child's points are transformed
  by its matrix, then offset; under USE_MY_METRICS the composite takes the
  child's phantom points;
* the glyph is shifted left by its left phantom point at the top level --
  ~~undocumented, as HarfBuzz's source calls it~~ the text's rule, found in
  step 8: the glyph is drawn from its left side bearing point.

**Four calls, made with Jev (the principles attached; every confidence under
0.6, so each took the rule written before asking):**

| question | taken | Jev |
|---|---|---|
| the API | `Face::outline` refuses a varied glyph with `Drawn::NeedsScratch(need)`; `Face::outline_at` draws it | B, 0.50 -- the same |
| malformed data part-way through a glyph | as HarfBuzz: the flushes already done stay, the pending deltas go | B (default outline), 0.26 |
| anchored components | ~~as the default path: offset 0, the delta read and not applied~~ matched on both paths, as the text defines it (step 7) | B (match points), 0.57 |
| SCALED_COMPONENT_OFFSET | ~~as the default path: ignored~~ honoured on both paths (step 7); neither flag, the text's open default, stays unscaled for Jev | B (honour), 0.18 |

The last two leave the varied path agreeing with the default one; neither
reaches any of the fixture fonts' 3,599 components, so neither is measured.
Honouring them is a change to both paths at once, and it is on TODO.
~~On TODO.~~ Built in step 7, against a fixture that reaches both.

~~**The shift is by the left phantom's delta, not its position.** Phantom
points start at 0, so the shift is the delta alone and a glyph whose deltas
are 0 draws where the default path draws it. HarfBuzz shifts by `xMin - lsb`
plus the delta; the two agree wherever `xMin` is the left side bearing,
which is every glyph of the three committed variable fonts (`head` bit 1
set, zero exceptions counted).~~ **The shift is the left phantom's
position**, `xMin - lsb` plus its delta, on both paths, where `head` bit 1
is clear (step 8): the texts place the origin there. ~~With bit 1 set it is
still the delta alone, for Jev.~~ With bit 1 set it is the position too,
by the owner's pick of 2026-10-13, "xMin - lsb (Recommended)" (step 8,
question 6).

**The oracle is HarfBuzz's own drawing.** `tests/var/outlines.txt` holds
HarfBuzz 12.3.0's draw commands for every 37th glyph of the three variable
fonts at their default and ten weights, and every glyph of
`tests/var/gvar-cases.ttf` at its default and 44 settings of two axes --
1,098 rows, `scripts/gvar-oracle.py`. The cases font (`scripts/gvar-cases-font.py`)
exists because the real fonts reach none of the hard rules: it has a left
phantom delta, an intermediate region, a two-axis peak, a one-point sparse
tuple, inference with an equal-position pair, a composite with its own
tuple under USE_MY_METRICS, a nested composite with a rotated component and
a sparse tuple on one offset, three sparse tuples scoring together after a
first that changes a contour's shape, and two composites at HarfBuzz's
graph-edge bound. Every row agrees within 1.2e-4 of a unit --
the fixture's four printed places and an `f32` step. The default rows are
the control that the two emit contours by one rule, and they found the one
place they do not: a contour that starts on two off-curve points, where
HarfBuzz starts at the midpoint of the last and first and baud (like
ttf-parser) at the midpoint of the first two. The shape is the same, and no
glyph of the real fonts has one; the cases font starts its contours
on-curve. HarfBuzz also draws a one-point contour as `M p L p Z`, which
baud's default path draws nothing for; the test drops such degenerate
contours and asserts exactly how many it dropped.

**The walk is bounded by visits on both paths, at HarfBuzz's count.**
`HB_MAX_GRAPH_EDGE_COUNT` is 2,048 and HarfBuzz checks it *before* counting
a visit, so 2,049 visits draw and the 2,050th refuses the glyph -- at the
default instance too. baud's first varied path counted first (one visit
fewer) and its default path had no visit bound at all, only depth 32: two
components a level, 32 levels deep, is 2^32 visits on a hostile font, which
random byte flips never reach. `edge2049` and `edge2050` in the cases font
sit on the boundary, and the default path now walks a composite once with
`Glyf::need` before drawing it, because it streams into the sink and a
refusal half-way would leave half a glyph there. Jev, both A: the bound on
both paths (0.61), HarfBuzz's count (0.95). `tests/oracle.rs` still holds
the default path to ttf-parser on every face on this machine; no real
composite comes near the bound.

**Two defects the perturbations found in the tests, not the code.** Taking
inference's original positions afresh at every sparse tuple was green: the
cases font's first co-scoring tuple moved each contour by one point, a
translation, and a re-taken position keeps a translated contour's ratios.
It moves two points of one contour apart now, and the perturbation is red.
And the visit bound had no gate at all until the boundary glyphs existed.

**Twenty-six perturbations, one a mechanism, each red at its own tests**,
run against the `gvar` unit tests and `tests/var.rs`:

| broken | what goes red |
|---|---|
| `apply` doing nothing; no inference; no flush before a sparse tuple, or none after the last; the original positions re-taken at every sparse tuple; the equal-position rule always the previous point's; the nearer point swapped outside a pair; no private points read as every point; referenced points not marked; the scalar's zero coordinate not ending the tuple | `every_sampled_outline_is_harfbuzzs` |
| an intermediate region's start and end swapped | that, and `the_scalar_is_the_overviews` |
| a component offset without its delta; no USE_MY_METRICS copy; no left-phantom shift; the matrix after the offset; a one-point glyph emitted; a composite's own tuples not applied; a simple glyph's tuples not applied | `every_sampled_outline_is_harfbuzzs` |
| the need without the stack for a composite's own points | that, and `a_varied_glyph_is_never_drawn_as_the_default` |
| `outline` drawing the default when varied; `outline_at` ignoring a short scratch | `a_varied_glyph_is_never_drawn_as_the_default` |
| a point run or a delta run past its count accepted | their unit tests |
| no visit bound; a visit counted before the check; the default path not walked first | `a_walk_past_harfbuzzs_edge_bound_draws_nothing_on_either_path`, and `every_sampled_outline_is_harfbuzzs` |

**The hostile-input property draws every glyph varied too**: at wght 700,
wdth 150, with exactly the scratch the need names -- the need is the number
a caller sizes a buffer from, so a hostile font must not make it short --
over byte flips and truncations of the 2.5 KB cases font as well as the
real faces, at 20,000 cases.

**Depth is not HarfBuzz's.** Both paths stop at 32 levels of nesting,
which the default path always had; HarfBuzz's `HB_MAX_NESTING_LEVEL` is 64
(`[[source: harfbuzz-12-3-0-limits]]`). No fixture nests past three, and
the visit bound makes a deep fan-out refuse long before either depth.

**What the pre-walk costs: nothing measurable.** The default path now walks
a composite's component headers once before drawing it, and it is reached
by the atlas and by the shaper, which reads a glyph's ink box to place
marks on a run-cache miss. `frame`'s convert column, `b75c1d0` against this
change and an A/A copy of it, built at one path, six rounds rotated with
the first dropped, on mains, us a frame:

| recording, faces | before | after | A/A |
|---|---|---|---|
| cat-cjk, JetBrains Mono | 1064.2 [1061.7-1065.6] | 1065.8 [1058.9-1070.1] | 1066.4 [1065.1-1069.5] |
| cat-cjk, the Menlo stack with CJK faces | 889.7 [888.2-894.4] | 894.3 [891.7-895.2] | 893.1 [888.6-899.0] |
| vim, JetBrains Mono | 227.3 [227.2-233.9] | 232.3 [227.3-233.9] | 230.0 [227.2-231.9] |
| vim, the Menlo stack | 207.5 [203.6-211.9] | 208.9 [207.5-210.1] | 207.7 [205.7-210.2] |

Every range overlaps. Nothing draws a varied face yet.

## Step 3, built: advances at a variation

`Face::advance` answers at the face's coordinates, and
`Face::default_advance` is the `hmtx` read it used to be, which the cell
metrics now call, so a styled face at another weight cannot move the cell.
Jev, A on both, 0.74 and 0.98. A first request asked three questions.
Its API question rested on a wrong premise -- that the no-`HVAR` advance
needs caller scratch -- and its answer (B, 0.39) is withdrawn; the
question was asked again with the facts corrected. Its other two stand,
decided by the rules written before asking because Jev was under the 0.6
bar on both: a walk HarfBuzz refuses gives HarfBuzz's `upem / 2` (Jev C,
0.11; the rule: an unspecified step follows the implementation the
platform renders with), and an advance stays a `u16` that saturates (Jev
B, 0.07; the rule: the range a real font uses decides the type).

**No scratch in any case, and HarfBuzz is why.** Its advances come from
`HVAR` when the face has one, from `gvar`'s phantom points when it has
only that, and from `hmtx` otherwise
(`[[source: harfbuzz-12-3-0-ot-font]]`). The phantom path walks the glyph
with `phantom_only` set (`[[source: harfbuzz-12-3-0-glyf-glyf]]`): a
simple glyph is counted from `endPtsOfContours` and nothing of it decoded
(`[[source: harfbuzz-12-3-0-simple-glyph]]`), only its four phantoms take
deltas, and a composite applies its own and then takes the phantoms of
each USE_MY_METRICS component -- the last one wins -- walking no other.
So `Glyf::phantom_extent` is four points a level on the stack, and
`gvar::Store::apply` gained a `Track` naming whether it moves every point
or the phantoms alone: one tuple loop, not two spellings of its flushes.

**The rules, each held by the oracle:**

* `HVAR`: the `hmtx` advance plus the store's delta rounded, floored at
  0 (`[[source: harfbuzz-12-3-0-hmtx]]`). The store is
  `baud_font::ivs`, kept apart from `HVAR` for step 4's device tables:
  `f32` scalars, a region past the list scoring 0, a region whose start,
  peak and end are out of order or straddle 0 scoring 1, byte, word and
  LONG_WORDS rows, and the delta-set map's clamp past its last entry and
  its repacking (`[[source: harfbuzz-12-3-0-layout-common]]`). An absent
  map is the glyph id; a map of a format other than 0 or 1 is too.
* The phantom path: the extent of the phantoms rounded and floored at 0,
  `upem / 2` when the walk is refused -- a malformed glyph on the
  USE_MY_METRICS chain, or one visit past the 2,049 HarfBuzz allows -- and
  0 for a glyph id past the face, on either path. A null data set is no
  items (HarfBuzz's `Null (VarData)`), so its glyphs' deltas are 0 and the
  rest of the store still varies. A component that is not
  USE_MY_METRICS is not walked, so a malformed one does not refuse its
  parent's advance.
* **Every `roundf` in HarfBuzz is its own `floorf (x + .5f)`**
  (`[[source: harfbuzz-12-3-0-hb]]`): a half rounds up, -0.5 to 0. The
  first build rounded halves away from zero and 110 of the oracle's rows
  said otherwise, every one a negative half. On the phantom path the two
  rules cannot differ -- an extent is an advance plus a delta, positive,
  and a positive half rounds up either way -- so that perturbation is not
  in the table below.
* **Step 1 had the same mistake.** Its normalisation rounded a negative
  half away from zero, and none of its 12,003 fixture values lands on one:
  an axis 0 / 2 / 3 at `2 - 5/65536` is -2.5 in 16.16, which HarfBuzz takes
  to 0 in 2.14 and the first version took to -1. It rounds a half up now,
  and `a_negative_half_rounds_up_as_harfbuzz_rounds_it` is the gate.
* **Malformed data refuses the whole table.** HarfBuzz 12.3.0's
  `sanitize_blob` replaces a table that fails any check with the empty
  blob and no longer rewrites a bad offset to null
  (`[[source: harfbuzz-12-3-0-sanitize]]`, `[[source: harfbuzz-12-3-0-open-type]]`):
  a bad store, data set, region list, advance map -- or side-bearing map,
  which nothing reads -- makes the face take its advances from the
  phantoms. Seven broken fixtures show it. An offset of 0 is null, not
  malformed: a null store, region list or data set leaves the table read
  and those deltas 0, and three fixtures show that too.
* **`regionCount` is sixteen bits.** The format calls it fifteen and
  HarfBuzz's `HBUINT15` does not mask the top bit (a TODO in
  `hb-open-type.hh`). The first fixture set it, read the struct's name as
  the rule, and every `HVAR` in the set was refused until the oracle said
  so.

**The oracle is `tests/var/advances.txt`**: HarfBuzz 12.3.0's advance for
every 11th glyph of the three variable fonts at their default and four or
five weights, and every glyph of the cases fonts at their default and 34
settings of two axes, plus one glyph id past each face at every setting
away from the default -- 13,252 rows, `scripts/advance-oracle.py`. All
agree exactly. The default rows are the control that the oracle's scale is
units per em. The real fonts reach almost nothing: JetBrains Mono's
`HVAR` has one nonzero delta and Fira Code's none. So
`scripts/hvar-cases-font.py` writes an `HVAR` byte for byte into sixteen
small fonts under `tests/var/hvar/` -- one with every rule above, five
maps (none, format 1 with three-byte entries and twelve inner bits, one-
and four-byte entries, an unknown format), seven broken tables and three
with a null store, region list or data set -- each
with `gvar` phantom deltas that disagree with `HVAR`, so a table not read
shows. `phantom.ttf` has no `HVAR`: lsb unequal to xMin, a composite whose
own deltas USE_MY_METRICS replaces, two such components, a nested chain, a
truncated USE_MY_METRICS child, a truncated child that is not one, a
composite with no USE_MY_METRICS child, and fan-outs at 2,049 and 2,050
visits.

**Thirty perturbations, one a mechanism, each red at its own test** -- `every_advance_is_harfbuzzs` for all but the last:

| broken | |
|---|---|
| a region axis: a peak of 0 scoring 0; a disordered region scoring 0; a straddling one interpolating; a region past the list scoring 1; `regionCount` masked to 15 bits | the store |
| a data set with more wide columns than regions accepted; LONG_WORDS columns read as 16 bits; a narrow byte read unsigned; a malformed data set not refusing the store | the rows |
| no clamp past the map; an entry one byte short; one inner bit short; format 1 counted in 16 bits; an unknown format refused | the map |
| the side-bearing map not checked; any major version read; a null store, region list or data set each refusing the table | the table |
| HVAR's delta rounded away from zero; no floor at 0; gvar tried before HVAR; HVAR ignored past the face | the HVAR path |
| the phantoms not read; a refused walk given the `hmtx` advance; the first USE_MY_METRICS component winning; a composite's own deltas not applied; no visit bound; the phantoms placed one point late | the phantom path |
| step 1's half away from zero | `a_negative_half_rounds_up_as_harfbuzz_rounds_it` |

Four were green or unbuilt on the first run and each was the fixtures:
no data set was in range and malformed (the broken offset ran past the
table, which is refused before the set is read), no narrow byte that
scored was negative, and the clamp's perturbation left a variable unused.
A fifth, inference on the phantom walk, cannot fire: phantoms are never
contour ends, which is why HarfBuzz never infers them either. The three
null perturbations were added after a review found no fixture set an
offset to 0: turning a null into a refusal would have stayed green.

**What is not HarfBuzz's, recorded:** an advance saturates at 65,535
where HarfBuzz's is an unsigned int; a face with no `hmtx` has no
advance, where HarfBuzz gives `upem / 2` at any coordinates -- baud's
default path, which `tests/oracle.rs` holds to ttf-parser, answers the
same; both walks stop at 32 levels where
HarfBuzz's limit is 64, and a cyclic component refuses the walk where
HarfBuzz's decycler skips it -- hostile fonts only; coordinates past the
sixteen axes `Coords` holds read 0; the side bearings are the default
instance's (`HVAR`'s side-bearing maps are checked, not read, and nothing
reads a side bearing at a variation); HarfBuzz's sanitizer also refuses a
table past an operation budget that baud does not count.

**Shaping at a variation stays partial until step 4**: advances move,
GPOS mark positions do not.

**What it costs: nothing measurable.** At the default instance `advance`
is one test of sixteen coordinates in front of the `hmtx` read, on the
shaper's per-glyph path. `frame`'s convert column, `304fc8d` against this
change and an A/A copy of it, built at one path, six rounds rotated with
the first dropped, on mains, us a frame:

| recording, faces | before | after | A/A |
|---|---|---|---|
| cat-cjk, JetBrains Mono | 1059.1 [1056.9-1062.6] | 1058.1 [1055.6-1065.5] | 1061.8 [1060.7-1064.2] |
| cat-cjk, the Menlo stack with CJK faces | 886.1 [882.6-890.8] | 885.5 [882.1-891.4] | 887.1 [884.7-888.4] |
| vim, JetBrains Mono | 228.9 [226.4-231.6] | 232.0 [229.4-236.2] | 232.5 [228.3-234.8] |
| vim, the Menlo stack | 209.7 [204.1-210.6] | 208.8 [204.0-210.5] | 208.6 [207.2-210.7] |
| git-log, JetBrains Mono | 968.0 [966.9-973.1] | 971.3 [966.4-975.2] | 971.8 [965.8-980.6] |
| git-log, the Menlo stack | 666.6 [664.6-669.6] | 670.3 [664.9-676.9] | 674.7 [667.3-688.0] |

Every range overlaps. Nothing draws a varied face yet.

## Step 4, built: mark anchors at a variation

Both fonts' `GPOS` is `mark` and `mkmk` only -- two mark-to-base and three
(Fira Code two) mark-to-mark lookups -- so what varies is anchors, and
nothing else in `GPOS` needs a store: JetBrains Mono upright carries 802
format-3 anchors with devices, the italic 785, Fira Code 1,433. `GDEF`
version 1.3 names the item variation store at offset 14, and
`Gpos::apply` takes the face's coordinates.

**The rule is HarfBuzz's**: a format-3 anchor's x and y device tables,
when of the VariationIndex format `0x8000`, name an item in `GDEF`'s store,
and the coordinate takes the delta rounded through `em_scalef` -- whole
units, so the mark offset's own `roundf` changes nothing
(`[[source: harfbuzz-12-3-0-anchor-format3]]`,
`[[source: harfbuzz-12-3-0-font]]`, `[[source: harfbuzz-12-3-0-mark-array]]`).
A hinting device moves nothing without a ppem, which a terminal's grid
never gives it. The store is step 3's `baud_font::ivs`, unchanged.

**Malformed variation data follows the tables' own rule, not
HarfBuzz's** (Jev B, 0.84). baud has read `GDEF` and `GPOS` subtable by
subtable since those readers landed: a malformed class definition or
lookup is absent and the rest is used. So a malformed store is no store,
and a malformed device is no device -- that coordinate does not vary --
where HarfBuzz refuses the whole `GDEF` or the whole `GPOS`
(`[[source: harfbuzz-12-3-0-gdef]]`). Step 3 refused a whole `HVAR`
because `HVAR` was new; here the table already had a rule and the new
field keeps it. A `GDEF` 1.3 header too short to hold the offset is read
as no store rather than no `GDEF`.

**The oracle is `tests/var/marks.txt`**: HarfBuzz 12.3.0 shaping every
Latin letter of the three variable fonts under each of sixteen combining
marks above and below, and thirty stacks of two, with the mark features on
-- 11,622 rows over the default and ten weights, `scripts/marks-oracle.py`.
All agree exactly on glyph, cluster and offset; 1,741 varied rows differ
from their default, so the devices are reached. What no real font reaches
is unit-tested over hand-built bytes in `gpos.rs` and `layout.rs`: the
rounding of a half, a hinting device whose sizes would name a real item if
misread, a device cut short or past the table, an index past the store,
formats 1 and 2, no store, `GDEF` below 1.3, a malformed store leaving the
classes, and a short header. `hostile.rs` shapes marks at a variation and
flips bytes inside the variable face's `GDEF` and `GPOS`.

**Twelve perturbations, one a mechanism, each red at its own test**: the
anchor never varying; formats 1 and 2 read for devices; x and y devices
swapped; a hinting device read as a variation index; the delta rounded
away from zero, or truncated; outer and inner swapped; `GDEF`'s store read
below 1.3, never read, or at the wrong offset; `GPOS` given no store; the
shaper handing default coordinates. The hinting one was green until its
fixture's sizes named a real item.

**What it costs is owed.** The new work is one format test and one
coordinate test per anchor, on the shaper's run-cache-miss path. Two
sweeps of `frame`'s convert column against `e092ebe` with an A/A copy were
taken while another process loaded the machine (load average 8 to 30):
the first overlapped on every row and the second spread over 30%, so
neither is quoted. It is to be retaken on a quiet machine.

**Not built, recorded:** value-record devices, which only kerning and
cursive lookups carry and baud's grid runs neither; a store's delta past
a 16-bit anchor clamps where HarfBuzz's float does not; `GSUB` and `GPOS`
FeatureVariations, absent from both fonts.

## Step 5, built: the bake

`baud_font::bake(font, wght, scratch, out)` writes one static TrueType
instance of a variable font into a caller's buffer, `no_std` and
allocation-free like the rest of the crate: the caller lends the point
scratch and the output, and a buffer too small is answered with the length
it needs. It is not yet called by a build script; that is step 6.

**What it emits was the open call, and the answer is a TrueType font with
its composites kept, point for point what fontTools' instancer writes.**
Jev put the choice at 0.37, under the bar, so the rule written before
asking decided it: an unspecified step follows the implementation the
platform renders with, and fontTools' instancer, the second oracle this
note names, is that implementation for a bake. Keeping `GPOS`'s variation
store for the runtime to read was the alternative; it would have kept a
variation path alive in a font that is otherwise static, and the anchors
bake in place for nothing.

**The oracle is fontTools 4.63.0, read at source rather than remembered**:
`[[source: fonttools-4-63-0-instancer]]`, `[[source: fonttools-4-63-0-solver]]`,
`[[source: fonttools-4-63-0-iup]]`, `[[source: fonttools-4-63-0-models]]`,
`[[source: fonttools-4-63-0-glyf]]`, `[[source: fonttools-4-63-0-tuplevariation]]`,
`[[source: fonttools-4-63-0-merger]]`, `[[source: fonttools-4-63-0-hhea]]`,
`[[source: fonttools-4-63-0-maxp]]`, `[[source: fonttools-4-63-0-os2]]` and
`[[source: fonttools-4-63-0-fixedtools]]`. Its rules, each of which the bake
follows and a test holds:

* **Deltas are `f64`, scaled before they are inferred.** A tuple's scalar
  at a pinned axis reduces, through `rebaseTent`, to `supportScalar` of its
  tent; an ill-formed tent is dropped (refused here). The explicit deltas
  are multiplied by it, *then* `iup_delta` fills the rest by `iup_segment`'s
  formula, `d1 + (x - x1) * ((d2 - d1) / (x2 - x1))`, then the tuples are
  summed in order. HarfBuzz scales after and writes the ratio the other way
  round; the two part in the last bit of a double, which lands on a
  different integer only at a half.
* **The weight is normalized in floats**: `normalizeValue`, `avar`'s
  `piecewiseLinearMap` over the 2.14 keys as floats -- a dictionary, so a
  repeated key keeps its last value -- then `floatToFixedToFloat`. This is
  not the runtime's rule (step 1 follows HarfBuzz's 16.16 integers), and the
  two part: the case font's wght 700 bends to 11,469 by HarfBuzz's rule and
  one unit lower by fontTools', so a delta of -5 is -3.5 exactly and rounds
  to -3 rather than -4. On JetBrains Mono at 400 and 700 they agree. Jev put
  HarfBuzz's rule at 0.49, under the bar, so the rule written before asking
  decided: a step the oracle defines follows the oracle, since the test is
  exact equality with it.
* **A glyph's points are the default plus the sum, rounded
  `floor(x + 0.5)` when written; a composite keeps its components**, each
  offset moved and rounded, pushed to words when it outgrows a byte.
* **Bounds are `recalcBounds`.** A composite whose offsets are all whole
  *before* rounding takes the union of its components' bounds, a degenerate
  component (a single point) skipped; otherwise it is flattened -- leaf
  points rounded, float offsets added, nested composites flattened too --
  and the extremes rounded. On JetBrains Mono at 700, 108 of the upright
  font's 570 composites and 180 of the italic's take the flattened path:
  gvar deltas are whole and the scalar is not.
* **Phantoms start at the stored `xMin` less the left side bearing, and the
  side bearing is the recomputed `xMin` less the left phantom.** The points
  are not shifted, so a left-phantom delta lands in `hmtx` and clears
  `head`'s flags bit 1. JetBrains Mono has none; the case font has one.
* **An anchor's device delta is rounded and added, and the anchor becomes
  format 1**; a hinting device is refused. `GDEF` loses its store: to
  version 1.2 when it names mark glyph sets, to 1.0 when not.
* **`head`, `hhea` and `OS/2` are recomputed** as fontTools recomputes them:
  `head`'s bounds and bit 1 from `maxp.recalc`, `hhea`'s extents over the
  drawn glyphs, `OS/2`'s weight class from the user weight and its average
  advance over the nonzero ones. The overlap flags are set on the first
  point of every simple glyph and the first component of every composite.

**Where it writes something else, and why each is so.** Six departures
from fontTools' bytes went to Jev in one request, with the rule written
before asking -- match the oracle unless it costs a second pass or a
feature nothing in baud uses:

| | Jev | decided |
|---|---|---|
| `loca` short when `glyf` fits | A, 0.26 | match, by the rule |
| `hmtx` trimmed to the trailing run | A, 0.10 | match, by the rule |
| `STAT` instanced | B, 0.55 | dropped, by the rule: nothing reads it |
| `name` pruned | B, 0.60 | copied whole |
| transformed components | B, 0.93 | refused |
| malformed data | B, 0.89 | refused -- fontTools raises too |

So `loca` and `hmtx` are fontTools', without a second pass: the glyphs
write them at full width, last in the file with `glyf`, and one move
compacts them after -- which is why a buffer the bake asks for can be
larger than the font it answers. What stays different changes no glyph,
metric or anchor: the coordinates are packed by this encoder, `STAT` is
dropped, `name` is copied whole, and `GDEF`'s store and `GPOS`'s devices
stay in their tables unreferenced. The four JetBrains Mono instances are
195-199 KB against fontTools' 191-195 KB, and each bakes in about
1 ms.

**What it refuses**: CFF outlines, an axis other than `wght`, `cvar`,
`MVAR`, `VVAR`, `vmtx`, `vhea`, `VARC`, a second `avar` version, feature
variations, a component anchored by point numbers or carrying a scale, a
device on a value record or a caret, and malformed data anywhere --
strictly, where the runtime forgives: a bake writes a font, and a guess
about a malformed tuple would be written into it.

**The tests**, in `tests/bake.rs`, read the bake's output and fontTools'
instances with a small `glyf` reader of their own, which shares no code
with the bake:

* every glyph -- contours, bounds, ends, program, every point's position
  and on-curve and overlap bits, every component's flags and offset -- and
  every advance and side bearing, over the four JetBrains Mono instances
  and four of a case font, `scripts/bake-cases-font.py`, built to reach
  what JetBrains Mono never makes the bake do: a glyph program, a
  degenerate component under whole offsets beside one under fractional
  ones, a nested composite, a left-phantom delta, a second advance, an
  intermediate region, an `avar`, and a trailing run of equal advances
  whose trim, like its twelve-glyph `loca`, leaves padding;
* `head` (but its checksum adjustment), `hhea`, `OS/2`, `maxp`, `post`,
  `cmap`, `prep` and `gasp` byte for byte, the table set (fontTools' less
  `STAT`), `GDEF`'s version and every anchor's format;
* every mark sequence step 4's oracle shapes -- 3,576, 281 of which move at
  700 -- shaped through both instances, equal;
* the directory's search fields, every table checksum and the file's,
  every table's padding zero, and `ttf-parser` reading it;
* determinism whatever the caller's buffer held, the length a short buffer
  is told, a short scratch and the refusals.

`tests/hostile.rs` bakes the case font flipped anywhere and cut anywhere,
and JetBrains Mono flipped inside `glyf`, `gvar`, `GPOS`, `GDEF` and
`loca`, at 20,000 cases each in a debug build with overflow checks. Four unit tests pin the
rules no font reaches, each against a double fontTools itself computed on
an input found by searching for one where the other order gives another
answer: the inference formula, scaling before inference, the intermediate
scalar kept in `f64`, and a half rounding up; three more pin a point named
twice keeping its last delta, `avar` read as a dictionary, and `loca`'s
threshold, which no fixture reaches from the long side. The inference is
fontTools' single walk round each contour, not a scan for each point's
neighbours.

**Against the runtime**: the bake is fontTools' instance and the runtime
draws HarfBuzz's variation, and over every glyph of both JetBrains Mono
files at 700 the two agree -- `the_baked_outline_is_the_runtimes_within_a_unit`,
which gates each kind: simple glyphs to **0.4998** of a unit, the rounding,
and composites to **0.905**, because their offsets are rounded before their
components' points are.

**Forty-eight perturbations, one a mechanism, each red at its own test**,
among them: no variation; no inference; the inference in HarfBuzz's form;
scaling after inference; the scalar through `f32`; always and never
flattening; no degenerate skip; rounding a half away from zero; the side
bearing from the stored `xMin`; each overlap flag; USE_MY_METRICS dropped;
each of `head`'s bounds and bit 1, `hhea`'s three extents, `OS/2`'s two
fields; `GDEF` not downgraded; an anchor unpatched, left format 3, or its
delta truncated; the checksum; `STAT` kept; each program dropped; the
buffer not zeroed; HarfBuzz's normalization; no `avar`; `avar`'s repeated
keys read first-wins at each of three places; no word offset; an ill-formed
tent accepted; a point named twice keeping its first delta; the inference
walk's gap starting on its named point or reading one reference twice;
`hmtx` untrimmed, its run start stuck, its side bearings not moved;
`loca` always long or its threshold off by two; `glyf` not moved; the
padding after `hmtx` or `loca` not zeroed. Ten were green on JetBrains
Mono alone, which is what the case font and the unit tests are for. One
stays green, and it is stated: the scalar cast to `f32` after `tent` has
worked it out, since every scalar that reaches the cast in these fonts is
exact in `f32`; the unit test pins `tent`'s own result.

**Not built, recorded**: the runtime cache for a configured variable font
-- the owner asked for one ("in runtime create a cache for next time") and
it has no consumer until fonts can be named by family, so it waits for that
and for the owner's reading of what it caches; transformed components, which
fontTools flattens with the transform applied and no font here carries;
value-record devices.



## Step 6, built: the renderer draws a varied face

`Face::outline` refuses a glyph that varies at the face's coordinates
(step 2), and the atlas and the shaper's glyph boxes took that refusal for
a glyph with no ink: a varied face drew blanks, memoised as blank, and a
face with no `GPOS` placed none of its marks. Nothing set a variation on a
renderer's faces, so nothing showed it; the embed swap and a configured
variable font would have.

* **The atlas** draws every outline through `Face::outline_at`. Its
  scratch grows to the need a refusal names and the glyph is drawn again;
  a static face never asks, so its glyphs pay one call. A need past
  `VAR_POINTS_MAX`, 200,000 points (HarfBuzz's `HB_GLYF_MAX_POINTS`
  (`[[source: harfbuzz-12-3-0-limits]]`) as a memory budget, about 5.6 MB),
  is refused, counted once in `AtlasStats::refused` and memoised as
  nothing. A `COLR` glyph with a layer refused is refused whole rather
  than painted in part -- none of its layers is painted, and it falls to
  its base glyph's own outline; the fixture's base glyphs are empty, so
  that fallback is untested.
* **The shaper** takes scratch from its caller (`Shaper::scratch`; the
  crate allocates nothing) and counts a refused glyph box in
  `Shaped::ink_refused` with the largest need, `Shaped::ink_need`. The
  renderer grows its scratch to that need, capped at the budget, and
  shapes the run once more; a box still refused is counted in
  `Renderer::ink_refused`. `frame` prints both counters.

`var/ink-cases.ttf` (`scripts/ink-cases-font.py`) is the fixture: a
variable font with no `GPOS`, two bases and six marks whose boxes move
with the weight, a 240,000-point composite, and two `COLR` glyphs. Its
fallback placements at four weights are HarfBuzz's
(`tests/var/marks.txt`, 80 varied rows, `every_mark_position_is_harfbuzzs`),
and `baud-render/tests/varied_face.rs` holds the atlas, the layered glyph,
the renderer's placement and the two refusals.


## Step 7, built: matched points and scaled offsets, on both paths

Step 2 left two component rules as the default path had them -- a
component placed by points sat at offset 0, and SCALED_COMPONENT_OFFSET was
ignored -- because no component of any font here reached either. None
does still: of about 2.11 million components in `/System/Library/Fonts` and the
fonts `baud-font/tests` held before this step, none is placed by points and
none carries bit 11 alone (132 carry bit 12). That scan's positive control is the fixture built for
this step, `var/component-cases.ttf` (`scripts/component-cases-font.py`),
in which it finds exactly the 16 matched, 6 scaled, 1 unscaled and 1
both-flags components of the 58 the script builds.

**What the texts say, and what is built**
(`[[source: ms-opentype-glyf]]`, `[[source: ms-opentype-gvar]]`,
`[[source: ms-opentype-tt-instructing-glyphs]]`, `[[source: apple-ttrm-glyf]]`,
`[[source: apple-ttrm-glyf-formulas]]`):

* **Points.** The parent's point is "from contours incorporated and
  re-numbered from previous component glyphs", the child's is "prior to
  re-numbering", "phantom points from the parent or the child may be
  referenced", and "the transformation is applied to the child's point
  before the points are aligned"; a point-matched slot's deltas "have no
  effect". Built: the child, varied and through its matrix, is translated
  to lay its point on the parent's, numbered from the composite's own first
  point; a child's left and right phantoms, numbered after its points
  (n and n+1), are reachable, and so are the parent's own, numbered after
  the points incorporated so far ("Phantom point numbers follow after the
  last point number of the last contour"), as the component finds them --
  an earlier USE_MY_METRICS component's, not yet its own, which the gvar
  text's order sets after the component is placed. A composite's own left
  and advance phantoms take only their X deltas and its vertical ones only
  their Y ("For AW and LSB points, apply the net X delta adjustments, and
  ignore Y deltas. For AH and TSB points, apply the net Y delta
  adjustments, and ignore X deltas"), which shows once one is a point a
  component aligns on. No fixture case has a component align on phantoms
  an earlier USE_MY_METRICS component installed; `anchpadv` holds only
  that its own do not count yet. Its drawing at 650 and 900 rests on that
  order: the process applies the composite's phantom deltas first ("note
  that the phantom point positions may be later overridden by a
  component") and replaces them at the USE_MY_METRICS component's own
  step, after it is positioned, which the summary states as "delta values
  for the composite glyph's phantom points are not used". The phantoms now
  start where HarfBuzz starts them, `(xMin - lsb, 0)` and an advance along,
  and the top-level shift is the left one's movement, as before.
* **Scaled offsets.** Bit 11 alone: the offset is "in the component glyph's
  coordinate system, and the scale transformation is applied to both
  values", and "it is the delta-adjusted offsets that are scaled". Built:
  the offset, moved by its delta, goes through the matrix with the points.
* **The default instance** cannot stream a glyph whose alignment reads
  points already emitted, so `Face::outline` refuses a tree that matches
  points with `Drawn::NeedsScratch`, found by the walk composites already
  took for the edge bound, and `Face::outline_at` draws it by the walk a
  varied glyph takes, without deltas. The atlas and the shaper's boxes
  draw through `outline_at` since step 6, so they draw these too.

**Against HarfBuzz 12.3.0 and FreeType 2.14.3**
(`tests/var/components.txt`, `scripts/component-oracle.py`;
`[[source: harfbuzz-12-3-0-glyph]]`, `[[source: harfbuzz-12-3-0-composite-glyph]]`,
`[[source: freetype-2-14-3-ttgload]]`), every glyph at the default and at
three weights. The two agree, within FreeType's half-unit rounding of a
varied point, on 19 of the fixture's 33 glyphs, and baud with both on 17 of
those (the other two are `dot2` and `umm`, below); where they part, the
text decides:

| case | HarfBuzz | FreeType | the text, and baud |
|---|---|---|---|
| a child's left or advance phantom | aligned | refused, `invalid composite glyph` | "may be referenced": aligned |
| a nested composite after another component | numbers the parent across the outer glyph | from the composite's own start | "a point number in the parent glyph": FreeType's |
| bit 11 with a rotation, or a negative scale | the matrix | each column's magnitude, `FT_Hypot` | "the scale transformation is applied": the matrix |
| bits 11 and 12 both | unscaled | scaled (reads bit 11 only) | "invalid ... default behavior": unscaled |
| a composite child's left phantom with a Y delta | moved by it | refused | "ignore Y deltas": not moved |
| the parent's left or advance phantom, n or n+1 after the points so far | the child's own points, aligned | refused | "Phantom points from the parent ... may be referenced", numbered "after the last point number of the last contour": the parent's |
| a parent point past every point HarfBuzz holds | offset 0 | refused | open: offset 0, as before (Jev, below) |
| a parent point past the parent's left and advance phantoms but within the child's, or the first component matched | the child's own points, aligned | refused | open: offset 0, as before (Jev, below) |
| a child's top phantom | aligned, `(0, yMax)` | refused | open: offset 0 (Jev, below) |

Both engines also shift a top-level glyph whose xMin is not its lsb by the
difference -- `dot2`, and `umm`, which takes `dot2`'s phantoms under
USE_MY_METRICS -- which baud has never done (step 2's shift is the
movement); ~~the test holds those two to baud's own default drawing, and the
difference is recorded, not changed here. Not changed.~~ Built in step 8
where `head` bit 1 is clear; this fixture sets it, so ~~`dot2` and `umm` keep
baud's drawing, for Jev (step 8, question 6)~~ both are drawn from 20 as
both engines draw them, and the test holds both to both engines (step 8,
question 6).

**For Jev, where the texts leave it open (each kept as it was; 2 is
built where the text turned out to define it):**

1. *Neither scaled-offset flag, and both.* (A) unscaled: the default the
   Microsoft text recommends and says Microsoft and Apple platforms take,
   HarfBuzz's and FreeType's default; conforms to the Microsoft text.
   (B) Apple's: the offset scaled by `m` and `n` (x' = a·x + c·y + m·e,
   y' = b·x + d·y + n·f, m₀ = max(|a|, |b|), doubled when ||a| - |c|| ≤
   33/65536, and n₀ = max(|c|, |d|), doubled when ||b| - |d|| ≤
   33/65536); conforms to Apple's text, which has no bits 11
   and 12. (C) the full matrix, as bit 11; conforms to neither for this
   case. Kept: A. Both flags set follow the same answer, the text calling
   them invalid and giving the default.
2. *A parent point past the earlier components' points.* (A) no
   alignment, the component where its matrix put it -- as before, and
   HarfBuzz's for a number past everything it holds. (B) the glyph refused,
   FreeType's. (C) the parent's own phantoms numbered after the points so
   far, n to n+3, a reading of "phantom points from the parent may be
   referenced" with the instructing chapter's numbering. (D) HarfBuzz's,
   the points it holds including the child's own; departs, since those are
   not the parent's. ~~None is the text's. Kept: A.~~ C is the text's
   most direct reading ("Phantom points from the parent or the child may
   be referenced"; "Phantom point numbers follow after the last point
   number of the last contour"), the parent's points being those "from
   contours incoporated and re-numbered from previous component glyphs".
   **Built: C**, for the left and advance phantoms, n and n+1 (`anchself`,
   `anchpadv`, `anchplx`). What stays open is a number past those: n+2 and
   n+3 are the parent's vertical phantoms, under 4 with the child's
   (`anchptop`), and a number past n+3 names no point the text defines --
   (A) offset 0, as before, (B) refused, (D) HarfBuzz's, the child's own
   point where it holds one; the text neither asks for nor forbids any of
   the three. ~~Kept: A, for Jev.~~ Jev picked A at 0.88 and 0.77 (round
   2026-10-12, g2), under the bar; **the owner kept A on 2026-10-13**,
   picking "Take Jev's leading pick (Recommended)", whose description gave
   "g2 offset 0 and c4 FontMatrix not read (both as built)". Since 4 is
   built, A covers a number past n+3 alone.
3. *The first component placed by points*, which the text says must not
   be ("This flag must always be set for the first component of a
   composite glyph"). It has no parent points but the parent's phantoms,
   which C would number 0 to 3; (A) offset 0, as before, (C) aligned on
   the parent's phantoms by C's numbering, or (E) the glyph refused as
   malformed, the text's "must" read as a condition of validity. The text
   says what a font must do and not what a reader does with one that does
   not, so none conforms or departs. ~~Kept: A.~~ The fonts lane added
   Microsoft's "cannot be used for the first component glyph", which read as a
   condition of validity makes E conforming; Jev tied A and E at 0.23 both
   times (g3), and **the owner picked E on 2026-10-13**, by the same pick,
   whose description gave "g3 (a first component placed by points)
   refused": the glyph is refused at any depth of its tree and nothing of
   its outline is drawn; its cell shows the next face that carries the
   character, else a `.notdef`, and a colour glyph still draws from its
   image or layers (the record, "A component may align on a vertical
   phantom, and a first component placed by points is refused").
4. *A child's or the parent's vertical phantoms.* (A) no alignment, as
   before. (B) `vmtx` read, and HarfBuzz's positions: `(0, yMax + tsb)`
   and an advance height below, `(0, yMax)` and `(0, yMax - upem)` with no
   `vmtx`; the text names the points (top origin, advance height) but no x
   and no rule without `vmtx`. (C) refused, FreeType's for every phantom.
   ~~Kept: A.~~ Jev picked B at 0.70 and 0.62 (g4); **the owner picked B on
   2026-10-13**, by the same pick, whose description gave "g4 read vmtx
   positions", and it is built: `[[source: ms-opentype-vmtx]]` puts the
   vertical origin's y at "the sum of the glyph's top side bearing ... and
   the top (i.e. maximum y) of the glyph's bounding box", with the advance
   height below it, and says its x "is not specified"; the x of 0 and the
   positions without `vmtx` are HarfBuzz's.
5. *Where a USE_MY_METRICS component's phantoms are taken from*, now that
   a later component can align on them. The gvar text says "the positions
   of phantom points for the composite glyph are set to the hinted
   positions of the component's phantom points", and not whether those are
   the component's own, before its offset and matrix, or where they land
   once it is placed. (A) before it is placed: as built, and HarfBuzz's.
   (B) after its offset, matrix and alignment. The text reads either way,
   so neither conforms or departs. Kept: A. The two part through the
   top-level shift, which follows the composite's left phantom: copied
   after placement, `anchpadv` shifts left by its dot's placement (580 to
   610 units) and 44 of the 1,098 sampled outlines of `var.rs` part from
   HarfBuzz's (A); a form of (B) that moves the phantom's start with it
   shows on neither, since the shift is then the phantom's movement before
   it was placed, as under (A). No case has a later component align on
   those phantoms.

`ROUND_XY_TO_GRID` is a grid-fitting rule, and nothing here fits to a grid.
The bake still refuses a matched or scaled component; it writes a font,
and these would need writing out as offsets first.

## Step 8, built: a glyph is drawn from its left side bearing point

HarfBuzz and FreeType draw a `glyf` glyph whose xMin is not its lsb moved
left by the difference, and Core Text does in the one case measured, `dot2`
in fonts that set `head` bit 1 (flags 0x3); baud drew its stored coordinates, moved
only by its left phantom's delta at a variation (step 2), and the fonts
lane found Core Text a third engine on it (round 2026-10-12, `dot2`).
HarfBuzz's source calls the shift "undocumented rasterizer behavior". The
texts define it:

* `[[source: ms-opentype-hmtx]]`: "If pp1 and pp2 are TrueType phantom
  points used to control lsb and rsb, their initial position in the
  X-direction is calculated as follows: pp1 = xMin - lsb; pp2 = pp1 + aw".
* `[[source: ms-opentype-tt-instructing-glyphs]]`: point n "will be placed
  at the glyph left sidebearing point; point “n+1” will be placed at the advance width point".
* `[[source: apple-ttrm-ch2]]`: those two points "represent the origin and
  advance width of the glyph. The origin point is the original pen
  position. Imagine that the pen moves the left-side bearing distance,
  draws the glyph, moves the pen the advance width from the origin point",
  and "the origin point would have the number n".
* `[[source: ms-opentype-head]]`, flags bit 1: "Left sidebearing point at
  x=0", and "The left sidebearing point for each glyph corresponds to the
  first phantom point ... pp1 = xMin - lsb". With the bit clear the point is not held at x = 0, and the glyph is drawn from it.

So the outline is drawn relative to the origin: every x less the left
phantom's final position -- its start, `xMin - lsb` from the glyph's header
and `hmtx`, or a USE_MY_METRICS component's start (the last one's, taken
before it is placed, as Jev decided step 7's question 5 on 2026-10-12, at
0.90 and 0.92), plus its `gvar` delta. `[[source: apple-ttrm-gvar]]`
says the same of the deltas: the phantoms "represent the glyph's
side-bearings". **Built, where `head` bit 1 is clear**, on the stream (the
start found by a walk down the USE_MY_METRICS chain, taken from x after each point is placed, as HarfBuzz takes it; the
midpoint between two off-curve points is taken before the shift here and
after it in HarfBuzz and the varied walk, the same value wherever the
points are integers) and on the varied walk (the
left phantom's position, where it was its movement).

**Where the texts give two answers: bit 1 set, xMin not the lsb.** The
`head` text says "If bit 1 of the flags field is set, then pp1 = 0 for all
glyphs and the xMin and left sidebearing for each glyph must be equal", and
Apple's (`[[source: apple-ttrm-head]]`) that the leftmost black bit "is
assumed to be the left side bearing". A font that sets the bit and breaks
the equality has `pp1 = 0` by one sentence and `xMin - lsb` by the other.
16 glyphs in 6 faces on this Mac do (ArialHB, Arial, Baskerville,
ZapfDingbats), and 546 in the embedded Symbols Nerd Font Mono (flags 0xb,
every one at -1). FreeType ignores the bit on purpose ("independent on the value
of bit 1 ... at least major applications like Acroread indicate that",
`[[source: freetype-2-14-3-ttgload]]`), HarfBuzz never reads it, and Core
Text draws `dot2` from 20 in a font with flags 0x3 (the fonts lane's
`ct-*.ttf`). ~~Kept as baud drew it, for Jev:~~ Decided by the owner on
2026-10-13 (below):

6. *Bit 1 set and xMin not the lsb.* (A) `pp1 = 0`: no shift at the
   default instance, the delta alone at a variation, and the phantoms a
   matched component aligns on starting at 0 -- the `head` text's sentence
   and Apple's "assumed", read as what a reader does; moves `anchlsb`'s dot
   in `component-cases.ttf`. (B) `pp1 = xMin - lsb`: the shift as with the
   bit clear -- the `hmtx` formula, read as holding regardless, the bit's
   sentence as a consequence of the font's "must"; HarfBuzz's, FreeType's
   and Core Text's; moves the 546 Nerd Font glyphs a unit, 16 system
   glyphs, and `dot2` and `umm`. (C) as built: phantoms starting at
   `xMin - lsb` for alignment and the advance's extent, but no static
   shift -- neither reading, a hybrid of the two, kept only because it is
   what was there. ~~Kept: C, until Jev decides between A and B; each sets
   pp1 in one place.~~ Jev split A 0.50 / B 0.43 and B 0.54 / A 0.35
   (round 2026-10-12c), ranking C last both times, and measured Core Text
   drawing every simple glyph from `xMin - lsb` whatever the bit says (555
   of 555) and every composite as its components each from their own start,
   with no shift by the composite's (171 of 171). **The owner picked, on
   2026-10-13,** "xMin - lsb (Recommended)", described "What HarfBuzz,
   FreeType and Core Text do (Core Text: 555/555 simple glyphs). The flag
   is a hint the font got wrong; hmtx is the measured value.", and
   declined "Keep the hybrid", "Today's mix, which matches neither
   reading." Over the 16 glyphs the question counted, Arial's 7 composites
   among them, that is B for a composite too: drawn from its own
   `xMin - lsb`, or its last USE_MY_METRICS component's, as HarfBuzz and
   FreeType draw it; C is the hybrid declined. Built (the record, "With
   head bit 1 set a glyph is drawn from xMin - lsb"): the bit is no longer
   read, and the 546 Nerd Font glyphs and the 16 system glyphs move. Core
   Text's composite rule, each component from its own start, departs from
   the glyf composite text, which moves a component by its offset alone.
   It differs from what is built on Arial's 7 composites, which Core Text
   draws unshifted (round 2026-10-12c), on Baskerville's
   `uni0149.nApostrophe`, whose `quoteright` it draws 93 units right, and
   on bit-clear composites (the record's Open (1)).

The census (fontTools 4.63.0, `/System/Library/Fonts`, a composite's start
taken from its last USE_MY_METRICS component): 648 `glyf` faces; 8,131
glyphs with contours whose left side bearing point is not at 0 in the 42
faces with bit 1 clear (Apple Color Emoji's two faces 3,708 each, Times
New Roman Italic 118, Hoefler Text's 56 and 37, Arial Bold 40, the rest
under 36), 11 of them composites with at most one point that draw
nothing; 16 in the 6 with bit 1 set. JetBrains Mono, Fira Code, Cascadia Mono, the built Regular and Bold, and
the three variable fonts the outline oracle samples
(`JetBrainsMono-Variable`, `JetBrainsMono-Italic-Variable`, `FiraCode-VF`)
have none. Of the committed fixtures, `component-cases.ttf` (`dot2`, `umm`)
and `hvar/phantom.ttf` (13 glyphs) have some and set bit 1, and the bake's
fontTools instances `cases-*.ttf` (`sq`, `instc`) clear it. So the embedded
faces drew as before under that change; a glyph that a bit-1-clear system
face supplies through fallback is now drawn from its left side bearing
point. (Since 2026-10-14 the bit-set Symbols Nerd Font Mono moves too: step
8, question 6.)

**The fixtures**, `tests/var/bearing-cases.ttf` and
`bearing-cases-bit1.ttf` (`scripts/bearing-cases-font.py`): the same ten
glyphs with bit 1 clear and set -- a simple glyph with its origin at 20 and
at -30, one at 0 whose left phantom moves, one at 20 whose left phantom
and first point move, a composite with its own start and own delta, three
USE_MY_METRICS cases (one, the last of two, nested), and two matched
composites, one aligned on a child's left phantom; `tests/var/bearings.txt`
(`scripts/bearing-oracle.py`) holds HarfBuzz 12.3.0's and FreeType 2.14.3's
drawings at the default and wght 100, 650 and 900, and each glyph's start
as fontTools reads it. Both engines draw both fonts alike, bit 1
regardless; FreeType refuses the phantom match, as in step 7.

