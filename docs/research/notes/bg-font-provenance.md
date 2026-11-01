# BG. Where baud-font's code came from

**Status: 2026-10-30, at `0dc9a368`.** What a user sees does not change.
What changes is what the published `baud-font` crate carries. HarfBuzz's
notice now covers one more file, `layout.rs`, whose glyph-set digest is
HarfBuzz's. The crate ships three more notices: Apache-2.0 for the
rasteriser, whose line, curve and coverage code is transcribed from
ab_glyph_rasterizer and font-rs; fontTools' MIT notice for the build-time
bake, whose variation arithmetic is transcribed from fontTools; and
ttf-parser's MIT notice for three files that follow ttf-parser 0.25.1,
baud-font's test oracle, function for function. Those are `cff.rs`, whose
charstring interpreter is CharStringParser's operators and
_parse_char_string() and whose charset is ttf-parser's Charset; `glyf.rs`,
whose contour builder and coordinate decoding are Builder::push_point(),
finish_contour() and CoordsIter's; and `outline.rs`, whose sink and
transform are OutlineBuilder and Transform. ttf-parser is offered under MIT
or Apache-2.0, and baud takes it under MIT. Every file section 2 does not
class (a) was written from its specification, is generated, or is baud's
own. Where one of them answers an open question the way HarfBuzz or
ttf-parser does, it cites that project and copies no routine of it.

Each file is in one of three classes:

- **(a)** a transcription or close port: another project's structure,
  names, control flow or constants, step for step. The file ships under
  that project's notice: a row in `docs/research/third-party.toml`, the
  text in the crate's `LICENSES/`, and an entry in its `NOTICE`;
- **(b)** written from the specification, with behaviour matched to
  another project for choices the text leaves open, such as a
  rounding, a malformed table, an order or an oracle's conventions. It is
  cited, and no notice is needed;
- **(c)** neither: the specification's own, or generated data that has its
  own row.

`scripts/gen-third-party.py --check`, which `scripts/check-sources.sh`
runs, reads the table in section 2. It fails when a file classed (a) has
no row under the notice that table names, and when a file in
`baud-font/src` is in no class.

## 1. How it was read

The pins are HarfBuzz 12.3.0, the 27 files committed under
`docs/research/sources/`, each identical to the 12.3.0 tarball, and
`hb-set-digest.hh` ([[source: harfbuzz-12-3-0-set-digest]]), plus the
64e2b53 files the manifest records. Also fontTools 4.63.0,
ab_glyph_rasterizer ([[source: ab-glyph-raster]], whose transcribed
functions are the same in the 0.1.10 crate the tests use as their oracle;
the two copies differ only in reset() and clear()), font-rs
([[source: font-rs-raster]], [[source: font-rs-accumulate]]),
ttf-parser 0.25.1, which is baud-font's test oracle
([[source: ttf-parser-0-25-1-cff-charstring]],
[[source: ttf-parser-0-25-1-cff1]], [[source: ttf-parser-0-25-1-cff-mod]],
[[source: ttf-parser-0-25-1-cff-charset]], [[source: ttf-parser-0-25-1-cff2]],
[[source: ttf-parser-0-25-1-glyf]], [[source: ttf-parser-0-25-1-gvar]],
[[source: ttf-parser-0-25-1-cmap]], [[source: ttf-parser-0-25-1-var-store]],
[[source: ttf-parser-0-25-1-lib]]), FreeType 2.14.3 and `stb_truetype.h`.

A scratch screen, which is not kept, compared every file in
`baud-font/src` and `baud-render/src` with all of those trees. It looked
for comment phrases of five words or more that the two share, and for
runs of ten code tokens, camelCase folded to snake_case and
punctuation dropped. It finds Rust copied from Rust only where the tokens
survive the copy. Shared runs start at 35 lines of `raster.rs`, and 34 of
them are ab_glyph_rasterizer's or font-rs's. It flagged `cff.rs`'s charset
and reserved operators and `glyf.rs`'s flag tests against ttf-parser. Its
token runs missed ttf-parser's charstring operators, which `cff.rs` follows
statement for statement, because `self.stack.at(i)` reads differently from
`a[i]`, and they missed `glyf.rs`'s contour builder, whose names changed. So
it is not reliable on Rust either. It finds some of Python: shared runs start at four lines of `bake.rs`, all of
them fontTools'. **It all but misses C++ ported to Rust.**
On the five files already classed (a), it found one of `morx.rs`'s two
paraphrased HarfBuzz comments and none of its code. It found one HarfBuzz
run in `gvar.rs`, three in `ivs.rs`, none in `glyf.rs`, and in `shape.rs`
only the table comments. So no file's silence on the screen counts as a
reading. Every passage that names HarfBuzz, fontTools, ttf-parser or a
rasteriser, and every routine the screen flagged, was compared with the
upstream function by hand. So was every function of `cff.rs`, `cff2.rs`,
`glyf.rs`, `gvar.rs` and `outline.rs` against the ttf-parser file that does
the same job, and every other baud-font file with a ttf-parser counterpart
was listed against it item by item, its bodies opened where the names, the
decomposition or the order of match arms lined up. Section 2 gives each
file's reading. Section 3 gives the
elements of the files this note classes (a), and of `hvar.rs`, `var.rs` and
`cff2.rs`, whose comments say they answer as HarfBuzz does; section 3.6
says what each of the screen's ttf-parser hits is.

## 2. Every file

| file | class | notice | upstream | reading |
|---|---|---|---|---|
| `baud-font/src/bake.rs` | a | `LICENSES/fonttools-MIT.txt` | [[source: fonttools-4-63-0-models]] 55-85, 144-222, 581-598; [[source: fonttools-4-63-0-iup]] 40-88, 91-140 | the variation arithmetic is fontTools': section 3.2. The table writer is baud's |
| `baud-font/src/bitmap.rs` | b | -- | FreeType's sbix loader, ttf-parser, HarfBuzz | named as practice beside CoreText's measured drawing; the strike and record readers are the `sbix`, `CBLC` and `EBLC` texts'. Against ttf-parser's sbix.rs, cblc.rs and cbdt.rs: another decomposition, which walks the index records where baud searches them, and is not followed |
| `baud-font/src/buffer.rs` | c | -- | -- | baud's glyph buffer |
| `baud-font/src/bytes.rs` | c | -- | -- | baud's bounds-checked reader |
| `baud-font/src/cff.rs` | a | `LICENSES/ttf-parser-MIT.txt` | [[source: ttf-parser-0-25-1-cff-charstring]] 175-580; [[source: ttf-parser-0-25-1-cff1]] 417-690, 692-709, 725-757; [[source: ttf-parser-0-25-1-cff-charset]] 73-128, 193-238 | the charstring interpreter and the charset are ttf-parser's, section 3.5: Pen's operators are CharStringParser's parse_curve_to() to parse_hflex1(), Run::exec() is _parse_char_string(), seac_glyph() is seac_code_to_glyph_id(), format 3 FdSelect::index_of() is font_dict_index(), and Charset is ttf-parser's Charset, sid_to_gid() and parse_charset(). bias() and subr_index() are 5176's numbers, which calc_subroutine_bias() and conv_subroutine_index() ([[source: ttf-parser-0-25-1-cff-mod]] 124-147) spell too. The DICT and INDEX readers, FDSelect's formats 0 and 4 and covers(), and the flex depth and threshold handed to the sink are written from 5176, 5177 and the `CFF2` text |
| `baud-font/src/cff2.rs` | b | -- | [[source: harfbuzz-12-3-0-cff2-interp-cs]] 141-162, 184-198; [[source: harfbuzz-12-3-0-cff-interp-common]] 220-248; [[source: ttf-parser-0-25-1-cff2]] 282-467 | four open choices taken as HarfBuzz takes them, section 3.3. Its path operators are `cff.rs`'s Pen, under that file's row. Its exec() is not ttf-parser's _parse_char_string(): an operator `CFF2` does not define is ignored and clears the stack where ttf-parser refuses the glyph, the hint order and the mask are checked, `vsindex` is checked against the store, and the blend sums HarfBuzz's way where ttf-parser pops each delta in reverse |
| `baud-font/src/cmap.rs` | b | -- | [[source: ttf-parser-0-25-1-cmap]] 61-85 | is_unicode() is ttf-parser's rule, cited in the file. The format readers are the `cmap` text's. Against ttf-parser's format0.rs to format14.rs: format 13's groups and format 14's default ranges are searched where ttf-parser walks them, and format 4 is not its glyph_index(), so none is followed |
| `baud-font/src/colr.rs` | c | -- | -- | `COLR` v0 and `CPAL` from their text. Against ttf-parser's colr.rs and cpal.rs, which read versions 0 and 1 in another decomposition: not followed |
| `baud-font/src/compose.rs` | c | -- | -- | generated from the UCD, under the Unicode row |
| `baud-font/src/context.rs` | b | -- | [[source: harfbuzz-hb-ot-layout-gsubgpos]], apply_lookup() | the walk resuming past glyphs a nested lookup put in is HarfBuzz's choice. run_records() shifts the matched positions by the nested delta, where apply_lookup() moves the buffer and rebuilds its positions. ttf-parser's ggg/ parses these subtables and applies none, and is not followed |
| `baud-font/src/describe.rs` | c | -- | -- | `name`, `OS/2`, `head` and `post` read for discovery. Against ttf-parser's os2.rs and post.rs: field reads at the texts' offsets, not followed |
| `baud-font/src/face.rs` | b | -- | [[source: harfbuzz-12-3-0-hmtx]] 280-310; [[source: harfbuzz-12-3-0-ot-font]]; [[source: harfbuzz-12-3-0-hb]] | the `vmtx` clamp, the `glyf`, `CFF2`, `CFF` order, the advance at a variation and round_half_up() answer as HarfBuzz answers. The clamp is the accelerator's clamps written as one expression. The directory and metrics readers are the texts'. default_advance() answers nothing past the glyph count, ttf-parser's rule, cited in the file; ttf-parser's hmtx.rs, head.rs, hhea.rs, vhea.rs and maxp.rs are not followed |
| `baud-font/src/forms.rs` | c | -- | -- | a lookup over the generated presentation forms |
| `baud-font/src/glyf.rs` | a | `LICENSES/harfbuzz-COPYING.txt`, `LICENSES/ttf-parser-MIT.txt` | [[source: harfbuzz-12-3-0-glyph]], get_points(); [[source: ttf-parser-0-25-1-glyf]] 75-137, 362-380, 391-396, 521-571 | the phantom-point walk is HarfBuzz's, re-expressed step for step, rowed before this note. The contour builder, Contour::push(), finish() and mid(), is ttf-parser's Builder::push_point(), finish_contour() and Point::lerp(); Coords::next() is CoordsIter::next(), and coord_lengths() is resolve_coords_len() in the unrolled form its comment keeps, section 3.5. The flag tests are the text's bits under ttf-parser's names. Loca reads a short `loca` as far as it goes and refuses an empty or descending range, as ttf-parser's loca.rs does, in its own code. Placement by points and the varied walk are baud's |
| `baud-font/src/gpos.rs` | b | -- | [[source: harfbuzz-12-3-0-anchor-format3]]; [[source: harfbuzz-12-3-0-mark-array]]; [[source: harfbuzz-hb-ot-shape]] | the rounding of a varied anchor, where a pair walk resumes and the zeroed mark advance are HarfBuzz's answers. The subtable layouts and lookup types are the `GPOS` text's. ttf-parser's ggg/ and gpos.rs parse and apply nothing, and are not followed |
| `baud-font/src/gsub.rs` | b | -- | [[source: harfbuzz-14-5-1-shaper-arabic]]; [[source: harfbuzz-hb-ot-layout-gsubgpos]] | the joining features' stages, a default-ignorable matched only when named (may_skip()), a nested reverse chain refused and an empty sequence deleting are HarfBuzz's choices, written over baud's filter and buffer. ttf-parser's ggg/ and gsub.rs parse and apply nothing, and are not followed |
| `baud-font/src/gvar.rs` | a | `LICENSES/harfbuzz-COPYING.txt` | [[source: harfbuzz-12-3-0-gvar]]; [[source: harfbuzz-12-3-0-var-common]]; [[source: harfbuzz-12-3-0-open-type]] | delta application, inference and the packed readers; rowed before this note. infer() is HarfBuzz's loop over unreferenced points ([[source: harfbuzz-12-3-0-gvar]] 782-837) and infer_one() its infer_delta() (593-614). ttf-parser's infer_deltas() ([[source: ttf-parser-0-25-1-gvar]] 1457-1596) walks its iterators again for each point, and neither follows it |
| `baud-font/src/hvar.rs` | b | -- | [[source: harfbuzz-12-3-0-hvar]] 301-311, 405-412 | section 3.3. ttf-parser's hvar.rs, which tests a 32-bit version and keeps the map offsets, is not followed |
| `baud-font/src/ignorable.rs` | c | -- | -- | generated, under the Unicode row |
| `baud-font/src/ivs.rs` | a | `LICENSES/harfbuzz-COPYING.txt` | [[source: harfbuzz-12-3-0-layout-common]] 2470-2494 | factor() is VarRegionAxis::evaluate(), the early return for a zero coordinate included; rowed before this note. ttf-parser's evaluate_axis() ([[source: ttf-parser-0-25-1-var-store]] 170-196) makes the same tests in another order, which factor() does not follow. Data::delta() is HarfBuzz's one loop with zero scalars skipped and Map is its identity for an empty or unknown map, where ttf-parser's parse_delta() runs two loops and its DeltaSetIndexMap refuses an empty map: ttf-parser is not followed |
| `baud-font/src/layout.rs` | a | `LICENSES/harfbuzz-COPYING.txt` | [[source: harfbuzz-10-2-0-set-digest]] 68-142, 214-224; [[source: harfbuzz-12-3-0-set-digest]] 66-172 | the Digest is hb_set_digest_t as HarfBuzz 10.2.0 builds it, with the shifts 2.6.4 and 8.0.0 have too: three 64-bit bits-pattern masks at its shifts 4, 0 and 9, its add, the range test that fills a mask when a range spans 63 or more, and its intersection. The shifts make it (a), section 3.4. HarfBuzz's comment about Thai is quoted and cited. ttf-parser's ggg/ coverage and class readers are decomposed otherwise and are not followed |
| `baud-font/src/lib.rs` | c | -- | -- | the crate's surface |
| `baud-font/src/morx.rs` | a | `LICENSES/harfbuzz-COPYING.txt` | 64e2b53, [[source: harfbuzz-hb-aat-layout-common]]; [[source: harfbuzz-hb-aat-layout-morx-table]] | the state machine and its four subtables; rowed before this note. ttf-parser's morx.rs and aat.rs only parse, and their lookup formats are decomposed otherwise: not followed |
| `baud-font/src/name.rs` | c | -- | -- | `name` from its text. ttf-parser's name.rs is not followed: the ranking is CSS's and baud's |
| `baud-font/src/outline.rs` | a | `LICENSES/ttf-parser-MIT.txt` | [[source: ttf-parser-0-25-1-lib]] 415-499, 577-596 | Sink has OutlineBuilder's five methods with their names and signatures, and Transform has ttf-parser's six fields, a to f, its combine() with the same terms in the same order, and apply(), which is apply_to(). Names and structure are ttf-parser's, so it is (a), section 3.5. Flex, FlexDrawn, Drawn and Sink::flex() are baud's, from the charstring texts |
| `baud-font/src/presentation.rs` | c | -- | -- | generated, under the Unicode row |
| `baud-font/src/raster.rs` | a | `LICENSES/Apache-2.0.txt` | [[source: ab-glyph-raster]] 104-268; [[source: font-rs-accumulate]] 69-81 | section 3.1. Fit, Phase, Bounds and the flex handling are baud's |
| `baud-font/src/script.rs` | b | -- | [[source: harfbuzz-hb-ot-shape]], hb-ot-shape.cc:613 | a decimal digit counted as HarfBuzz counts gc Nd; otherwise UAX #24's resolution |
| `baud-font/src/script_data.rs` | c | -- | -- | generated, under the Unicode row |
| `baud-font/src/shape.rs` | a | `LICENSES/harfbuzz-COPYING.txt` | 64e2b53, [[source: harfbuzz-hb-ot-shape-fallback]]; [[source: harfbuzz-hb-unicode]]; [[source: harfbuzz-hb-ot-shaper-hebrew]] | fallback mark placement and the Hebrew tables; rowed before this note |
| `baud-font/src/standard_encoding.rs` | c | -- | -- | Adobe's table, under its own row |
| `baud-font/src/stroke.rs` | c | -- | -- | baud's stroker, written from the texts it cites -- [[source: apple-quartz2d-paths]]'s line straddling the path, miter join and miter limit, and [[source: apple-ct-stroke-width]] -- and measured against CoreText's stroke, whose code is not open. It draws through `raster.rs`'s Canvas and copies none of it: its own curve flattening, its own pieces |
| `baud-font/src/var.rs` | b | -- | [[source: harfbuzz-12-3-0-ot-var]] 289, 330; [[source: harfbuzz-12-3-0-fvar]] 182-195; [[source: harfbuzz-12-3-0-avar]] 146-250 | section 3.3. ttf-parser's map_value() and normalized_value() (avar.rs, fvar.rs) are float or HarfBuzz's own form; baud's are 16.16 integers, and neither is followed |
| `baud-render/src/atlas.rs` | b | -- | [[source: harfbuzz-12-3-0-limits]] | HB_GLYF_MAX_POINTS taken as a scratch budget, a number |
| `baud-render/src/boxes.rs` | c | -- | -- | drawn from the cell rectangle. VTE's minifont is compared and differs |
| `baud-render/src/cache.rs` | c | -- | -- | baud's run cache |
| `baud-render/src/composite.rs` | c | -- | -- | compositing from the W3C texts |
| `baud-render/src/metrics.rs` | c | -- | -- | cell metrics from the face |
| `baud-render/src/renderer.rs` | c | -- | -- | no font engine's code. The other terminals it cites are outside this note |

## 3. Element by element

### 3.1 `raster.rs` against ab_glyph_rasterizer and font-rs

| element | upstream | baud | reading |
|---|---|---|---|
| a line's splat | draw_line_scalar(), [[source: ab-glyph-raster]] 104-169 | Canvas::line() 230-258 and splat() 189-227 | **line for line**: the same locals (dxdy, linestart, x0floor, x1ceil, xmf, a0, am, a1, a2), the same branches and the same deposits. The macro's `continue` on a deposit past the buffer becomes splat()'s None, and line() continues without advancing x, as the macro does |
| a quadratic | draw_quad(), 178-198 | quad() 263-283 | the same count, `1 + floor((3 devsq)^1/4)`, the same 0.333 cut and the same lerp walk |
| a cubic | tessellate_cubic(), 218-243, "adapted from stb_truetype" | cubic() and split_cubic() 288-309 | the same 0.35 flatness, depth limit 16 and midpoint split. ab_glyph credits stb_truetype for the subdivision, and stb_truetype offers itself in the public domain or under MIT, at the user's choice |
| coverage | font-rs's scalar accumulate(), [[source: font-rs-accumulate]] 69-81; for_each_pixel() 259-268 | coverage() 314-325 | the same prefix sum, `if y < 1.0 { y } else { 1.0 }` and `255 * y` |

The file's header says this, and `tests/raster.rs` holds the masks
byte-identical to the 0.1.10 crate. `raster.rs` now carries the copyright
lines of the file it came from and says what was changed, as Apache-2.0
4(b) and 4(c) ask. Neither font-rs nor ab_glyph_rasterizer ships a
NOTICE file, so 4(d) asks for nothing more.

### 3.2 `bake.rs` against fontTools 4.63.0

| element | upstream | baud | reading |
|---|---|---|---|
| user value to -1..1 | normalizeValue(), [[source: fonttools-4-63-0-models]] 55-85 | normalize() 204-224 | **term for term**, including the test `(v < def and lower != def) or (v > def and upper == def)`, which the `avar` text does not have |
| `avar` bend | piecewiseLinearMap(), 581-598 | piecewise() 227-270 | the exact match, the extrapolation past either end by `v + map[k] - k` (fontTools' own, the text maps only within -1..1), and the interpolation, in that order |
| a tent's scalar | supportScalar(), 144-222 | tent() 168-194 | the OpenType cases in fontTools' order, on one axis; an ill-formed tent is dropped, as the instancer drops it |
| inferred deltas | iup_segment(), [[source: fonttools-4-63-0-iup]] 40-88 | iup() 284-301 | **step for step**: equal ends give d1 or 0, the swap, the scale, the clamps |
| a contour's gaps | iup_contour(), 91-140 | infer() 307-344 | re-expressed: one modular walk from the first named point, where fontTools makes three segment calls; the same result for every point |

### 3.3 `hvar.rs`, `var.rs` and `cff2.rs` against HarfBuzz

| element | HarfBuzz 12.3.0 | baud | reading |
|---|---|---|---|
| `HVAR` accepted | HVARVVAR::sanitize(), [[source: harfbuzz-12-3-0-hvar]] 301-311 | Hvar::parse() 28-52 | the same five checks: major version 1, the store and three maps each sound, in the order the text lays out the fields. That a failed table is absent is the one choice taken from HarfBuzz |
| advance delta | get_advance_delta_unscaled(), 405-412 | Hvar::delta() 55-63 | map then store, which is the text's definition: with no map, the glyph id is the index |
| normalisation | normalize_axis_value(), [[source: harfbuzz-12-3-0-fvar]] 182-195, in floats; roundf at [[source: harfbuzz-12-3-0-ot-var]] 289 and 330 | normalize() 222-257 | the text's steps in 16.16 integers. The one rounding the text leaves open is HarfBuzz's roundf(), which is floorf (x + .5f) |
| `avar` bend | SegmentMaps::map_float(), [[source: harfbuzz-12-3-0-avar]] 146-250 | bend() 277-306 | the text's segment search. HarfBuzz's recovery cases, fewer than two records and CoreText's repeated -1 and +1, are not in it |
| blend sum | blend_deltas(), [[source: harfbuzz-12-3-0-cff2-interp-cs]] 184-198 | Run::blend() 404-426 | the sum from 0 added to the default is HarfBuzz's order, where the text says only that the products "are then all added". blend() folds the stack in place, where HarfBuzz defers each value in blend_arg_t |
| scalars once | process_blend(), 141-162 | Run::regions() 392-400 | taken once, at the first `blend` |
| `f64` operands | number_t, [[source: harfbuzz-12-3-0-cff-interp-common]] 220-248 | `Stack<f64, STACK>` | a precision choice |
| mask length | the first mask sizes it | the hint-mask arm of exec(), 510-530 | fixed once at the first mask. The hint-order refusals are the text's own |

None of the three copies a HarfBuzz comment, table or routine, so all
three are (b).

### 3.4 `layout.rs` against HarfBuzz 10.2.0

| element | HarfBuzz | baud | reading |
|---|---|---|---|
| the filter | hb_set_digest_t, a combiner of three hb_set_digest_bits_pattern_t on unsigned long at shifts 4, 0 and 9, [[source: harfbuzz-10-2-0-set-digest]] 214-224; the same shifts in 2.6.4 (as hb_set_digest_lowest_bits_t) and 8.0.0 | Digest, three u64 words at SHIFTS 0, 4 and 9 | **the constants**: the mask width and the three shifts, in another order. 12.3.0 moved to 4, 0 and 6 ([[source: harfbuzz-12-3-0-set-digest]] 66), and baud kept the older set |
| a glyph | add() and mask_for(), 89 and 139-140 | Digest::add() | `1 << ((g >> shift) & 63)` into each mask, term for term |
| a range | add_range(), 91-106 | Digest::add_range() | the same test, a span of 63 or more fills the mask. Below it, HarfBuzz sets the run by `mb + (mb - ma) - (mb < ma)` and baud sets each bit in a loop |
| two sets | may_have() of two digests, 128-129 | Digest::may_have() | every mask pair shares a bit |

Not one line is copied. The choice of filter, its width, its shifts and its
range rule are HarfBuzz's, and the shifts are the constant that makes it a
port rather than the idea. So `layout.rs` joins the HarfBuzz row. The
COPYING at 10.2.0 is the same file as at 12.3.0.

### 3.5 `cff.rs`, `glyf.rs` and `outline.rs` against ttf-parser 0.25.1

| element | ttf-parser 0.25.1 | baud | reading |
|---|---|---|---|
| the flexes | parse_flex(), parse_flex1(), parse_hflex(), parse_hflex1(), [[source: ttf-parser-0-25-1-cff-charstring]] 456-580 | Pen::flex(), flex1(), hflex(), hflex1() | **statement for statement**: the absolute points named dx1 to dy5, `let dy4 = dy2;`, `let dy5 = self.y;`, and flex1's `abs(dx5 - x) > abs(dy5 - y)`, which the Flex1::EndPoint doc names as ttf-parser's. `self.stack.at(i)` became `a[i]`; the depth and threshold handed to the sink are baud's |
| the curves and lines | parse_curve_to() to parse_vh_curve_to(), 87-454 | rlineto(), rrcurveto(), rcurveline(), rlinecurve(), hhcurveto(), vvcurveto(), alternating_curveto() | the same count checks, the same odd leading operand and the same x1, y1, x2, y2 bodies. hhcurveto() and vvcurveto() walk chunks_exact(4) where ttf-parser indexes its stack, and alternating_curveto() reads forward where ttf-parser reverses its stack and pops |
| the moves | parse_move_to(), 17-85, and the width rule of _parse_char_string() | rmoveto(), hmoveto(), vmoveto(), moveto_args() and moved() | the first-move flag closing every later contour, and a width taken when the operand count is one more than the operator's |
| the interpreter | _parse_char_string(), [[source: ttf-parser-0-25-1-cff1]] 417-690 | Run::exec() | **its operator order**, the reserved `0 \| 2 \| 9 \| 13 \| 15 \| 16 \| 17` list, the odd-count width for stems and masks, the stem count halved, `(stems + 7) >> 3` mask bytes, data after `endchar` refused, and the seac branch popping accent, base, dy, dx and then the width |
| `seac` codes | seac_code_to_glyph_id(), 692-709 | Run::seac_glyph() | the same Standard Encoding lookup and `code <= 228` for ISOAdobe |
| FDSelect format 3 | FDSelect::font_dict_index(), 725-757 | FdSelect::index_of() | the same walk carrying the previous range's first glyph and Font DICT to the sentinel |
| the charset | Charset, sid_to_gid() and parse_charset(), [[source: ttf-parser-0-25-1-cff-charset]] 73-128, 193-238 | Charset, Charset::parse() and Charset::sid_to_gid() | the same variants, `sid == 0`, `last = first + more` in `u32` and `gid += more + 1`; the range count read until the glyphs are covered |
| a contour | Builder::push_point(), finish_contour() and Point::lerp(), [[source: ttf-parser-0-25-1-glyf]] 75-137, 391-396 | Contour::push(), finish() and mid() | **step for step**: first_on, first_off and last_off, the same four match arms and the same three-way close; mid() keeps lerp()'s `a + 0.5 * (b - a)` so the rounding is the same |
| coordinates | CoordsIter::next(), 362-380; resolve_coords_len(), 521-571 | Coords::next() and coord_lengths() | the same zero past the end, and the flag walk in the form resolve_coords_len()'s comment keeps, with its refusal of a repeat past the points left |
| the receiver | OutlineBuilder and Transform, [[source: ttf-parser-0-25-1-lib]] 415-499, 577-596 | Sink and Transform | the five method names and signatures, the fields a to f, combine() and apply() |

ttf-parser is offered under MIT or Apache-2.0, and baud takes it under
MIT, whose text and copyright line are in `LICENSES/ttf-parser-MIT.txt`. MIT
asks for no statement of changes.

### 3.6 The screen's ttf-parser hits

| baud | ttf-parser | what it is |
|---|---|---|
| `cff.rs` 76-77, `cff2.rs` 102-103 | cff1.rs 59-60, cff2.rs 53-54 | the flex operator numbers, 5177's |
| `cff.rs` 404, 441, 1436 | charset.rs 90, 196; cff1.rs 707 | the charset, rowed, and a test of it |
| `cff.rs` 697 | cff1.rs 813 | both quote 5176: "There are no predefined charsets for CID fonts" |
| `cff.rs` 1446 | cff1.rs 427 | the reserved operators of exec(), rowed |
| `cff.rs` 1647-1648, `cff2.rs` 1199-1200, `face.rs` 216, `glyf.rs` 352, `raster.rs` 517 | lib.rs 549-569 | a sink's method signatures, OutlineBuilder's, rowed under `outline.rs` |
| `cmap.rs` 241 | cmap/mod.rs 50 | is_unicode(), cited, section 2 |
| `forms.rs` 8 | parser.rs 11 | the Unicode Standard's "should not be used in general interchange", quoted; ttf-parser's phrase is its own |
| `glyf.rs` 471-483 | glyf.rs 406-410 | the flag tests, the text's bits |
| `gvar.rs` 600 | gvar.rs 397 | the tuple scalar's range test, which HarfBuzz has too; HarfBuzz's, rowed |
| `ivs.rs` 272-284 | var_store.rs 175-187 | evaluate_axis()'s tests, which factor() makes in HarfBuzz's order, section 2 |
| `ivs.rs` 198, `layout.rs` 657, `lib.rs` 176 | cmap/mod.rs 47, aat.rs 301, language.rs 449 | lifetime parameters and a `Debug` impl |
| `layout.rs` 99 | lib.rs 2306 | "this is one of the", a common phrase |
| `outline.rs` 28-31, 48-50, 128-130 | lib.rs 472-474, 549-558, 2108 | OutlineBuilder's methods and Transform's identity, rowed |
