# Instruction-set dispatch — why the seam is a trait and not a crate

> **Group:** I · **Sources:** `[[source: rust-target-feature]]`, `[[source: highway-dispatch]]`, `[[source: arm-neon-bitmask]]`, `[[source: cargo-1ef25ea-features]]`, `[[source: cargo-1ef25ea-specifying-dependencies]]` · **Status:** the split is built and measured free; ~~the `shrn` experiment is not started~~ the `shrn` experiment is built and measured: `Neon` in `baud-isa-aarch64` (`2960dd9`, 2026-08-28), +10% on asciiquarium and +3.5% on btop on the core later renamed `baud-core2`; wired into `baud-core`'s run scan (`390fdd0`) a null on all nine recordings, and its `stride_three` (`5fa797d`) +4.2% on cat-cjk and +4.0% on less-cjk; on by default since `c3c7c5e` (corrected 2026-10-10; the status line said "not started" while the section below reported the measurement)

## Why this note exists

The proposal was to split `baud-core` into per-instruction-set crates — one
arm, one x86-64, one main — so that each instruction set could be researched
and tuned on its own. That is a real goal and the reasoning behind it is
sound: per-ISA work wants a place to live.

It is also the shape that C++ SIMD libraries have, which is where the idea
comes from. This note is about whether that shape transfers to Rust, because
the answer turns on documented toolchain rules rather than on anything this
repo can measure, and neither rule was in the corpus.

## The C++ pattern is a workaround for a C++ problem

Highway compiles a translation unit **once per target** by re-`#include`-ing
it, and says why the include guards are unusual:

> Suppose you want to share vector code between several translation units, and
> ensure it is inlined. With normal code we would use a header. However,
> `foreach_target.h` wants to re-compile (via repeated preprocessor
> `#include`) a translation unit once per target. A conventional include guard
> would strip out the header contents after the first target.

`[[source: highway-dispatch]]` Q5.4. The reason the file is the unit is that
the *compiler flag* is the unit: `-mavx2` applies to a translation unit, and
mixing flags across TUs that share inline functions in a header is what the
separation exists to prevent.

So the per-ISA file split in C++ is not a design preference. It is the smallest
granularity the toolchain offers.

## Rust's granularity is the function, and the rule is exact

`[[source: rust-target-feature]]` puts the feature set on the function, and
defines what that means for inlining in both directions.

Outward, it is a prohibition:

> The compiler will not inline functions in contexts that do not support all
> the functions features.

and with `#[inline(always)]` it is a hard error rather than a silent
pessimisation — the RFC's own example is `bar cannot be inlined across
mismatching features`.

Inward, it is a permission, and this is the half that matters here:

> Iff the function is inlined into a context that extends its feature set, then
> the compiler is allowed to generate code for the function using this extended
> feature set.

## baud already depends on the inward rule

`feed_avx2` is `#[target_feature(enable = "avx2")]` and its whole body is
`self.feed_with::<Avx2>(base, n)`. `feed_with` carries no feature attribute: it
is generic over `Scan`, monomorphises, and is inlined **into** the feature-gated
entry point, which is precisely the case the RFC permits. That is how the hot
loop — the scan, the UTF-8 path, the cell writes — gets compiled with AVX2
available without any of it being feature-gated itself.

The per-ISA seam is therefore already present, and it is `Scan`:

```
baud-isa/src/lib.rs        trait Scan { run, final_byte, st }
baud-isa/src/portable.rs   core::simd -- NEON on aarch64, SSE2 on x86-64
baud-isa/src/avx2.rs       the only unsafe in baud-core
```

`no_std` has no `is_x86_feature_detected!`, so `feed` and `feed_avx2` are
separate entry points and the caller picks once at startup. That is static
dispatch by construction, which is the form with no runtime cost.

## What a crate boundary would change

It would move the required inlining — the generic hot loop **into** the
feature-gated entry point — across a crate boundary, and make it depend on LTO.
baud sets `lto = true`, so it would most likely still happen.

"Most likely still happens, verified by nothing" is the specific failure this
repo has already had. A refactor deleted `feed_avx2` outright and the benchmark
kept printing `scan path: AVX2` from a stale flag, so a loss was attributed
to the type system instead. The CI step that grepped the monomorphised binary
for `vpmovmskb` existed because of that, and it would have become the *only*
thing standing between a crate split and a silently portable build. It went
with `baud-core2`; its successor is the bench workflow's probe, which
requires a 32-byte `vpmovmskb %ymm` in the AVX2 arm and fails on `cmp -s` if
the two arms are one binary (`.github/workflows/bench.yml`).

Against that, the split buys nothing the language does not already give:

* **Per-crate compiler flags are not needed.** `#[target_feature]` is
  per-function. The thing C++ needs the translation unit for does not exist
  here.
* **Per-ISA code separation already exists**, at module level, with one file
  per instruction set and the only `unsafe` confined to `avx2.rs`.
* **Per-ISA research is not blocked by it.** A new instruction set is a new
  `impl Scan` and a new entry point, additively.

And it would cost: `baud-core` is `no_std`, `alloc`-free and
zero-dependency, gated by a bare-metal build and a type audit with per-file
budgets. Each of those multiplies per crate.

## Reuse is a different question, and a better one

The argument above answers "does splitting make it faster", and the answer is
no. It does not answer "does splitting make the ISA work *reusable*", which is
the stronger form of the proposal: a per-ISA kernel is very low-level,
general-purpose code, and nothing about `shrn`-as-a-movemask or a three-byte
UTF-8 gather is specific to terminals.

That argument is sound, and it changes what to extract. The unit is not
"`baud-core`, split by ISA" — most of `baud-core` is a terminal, and a crate
called `baud-core-aarch64` holding `feed_with` would be a terminal wearing an
architecture's name.

**And the unit is not "primitives" either**, which is the obvious wrong answer
and the one this repo is built to refuse. A crate exporting
`fn movemask(v: u8x16) -> u32` is a bag of casts; the type audit exists because
five of `baud-core`'s eight remaining raw-integer sites are *exactly* that
boundary, where an intrinsic's result enters the type system. Extracting them
as primitives would export the problem.

The reusable unit is **domain-typed operations**: the same `Scan` surface,
taking and returning `baud-domain`'s types. `run` already returns `ByteLen` and
not a `usize`; mask extraction already lands in `LaneMask` and its first set
lane is a `Lane`, so a bitmask can never be added to a byte offset. An ISA
crate written against those types is reusable *and* cannot reintroduce the
class of bug the newtypes were added to prevent — which is what makes it worth
extracting rather than merely possible.

That gives three layers, each with one job:

```
baud-domain            the type system
baud-isa-<arch>        ISA implementations of domain-typed operations
baud-core              the terminal
```

The line falls in the same place for a second, independent reason. Domain
newtypes are `repr(transparent)` and `#[inline(always)]`, and this repo has
already measured that typing the hot path is free — so they cross a crate
boundary the way they cross a function boundary. `feed_with` is the one thing
that must not move: it holds all state behind `&mut self`, and it has to
inline *into* the `#[target_feature]` entry point for the whole loop to get
the extended feature set.

The revised claim is not that a boundary would be slow — it most likely would
not — but that the inlining stops being guaranteed by construction and becomes
something CI has to check. baud already has that check.

The cost is that `baud-core`'s "zero dependencies" becomes "two first-party
dependencies", which is a README claim and a project decision rather than a technical one.

## Per-microarchitecture kernels are already possible here

M4 and M5 differ in more than the instruction set — issue width, port counts
and instruction latencies all move — so a kernel tuned for one is not
automatically best on the other. Splitting by *ISA* would not capture that
anyway, since both are aarch64.

baud can already do it, and for a reason that looks like a limitation.
`no_std` has no `is_aarch64_feature_detected!`, so dispatch is **static**:
`feed` and `feed_avx2` are separate entry points and the caller picks once at
startup. An `feed_m4` alongside them is one more `impl Scan` and one more entry
point, and the embedder — which has `std`, and `sysctl`, and knows what it is
running on — makes the choice. A library that dispatches at runtime has to own
that detection; baud hands it to the caller who can actually do it.

What baud cannot do is detect the microarchitecture itself. That is the part to
be honest about, and it is the embedder's job by construction rather than by
omission.

## One measured ISA finding, and it is actionable

`[[source: arm-neon-bitmask]]` argues that `shrn` — shift right and narrow,
treating a 128-bit vector as eight 16-bit lanes and truncating to eight bytes —
is the cheap way to emulate x86's `movemask` on NEON, and that most libraries
have not taken it up.

~~baud does not use it.~~ It did not when this was written; `Neon` in
`baud-isa-aarch64` uses it now (corrected 2026-10-10). Disassembling
`baud-core2`'s `feed_with::<Portable>` on this box, the
mask extraction behind `LaneMask::from_movemask` lowers to `uzp1`, `zip1`,
`and.16b` and **`addv.8h`** — the horizontal-reduction form, which carries a
serial dependency `shrn` does not. `shrn` appears elsewhere in the binary, so
the toolchain will emit it; `core::simd`'s `to_bitmask` simply does not choose
it here.

That is a concrete, measurable per-ISA experiment ~~and it is unstarted~~:
hand-write the aarch64 mask extraction with `shrn #4`, take `rbit`/`clz` on the
resulting 64-bit value, and shift the nibble index down to a byte index. It is
the first thing to try behind `Scan`, and it needs no crate. **Built and
measured since** (corrected 2026-10-10): `2960dd9` wrote it and measured +10%
on asciiquarium and +3.5% on btop on the core of the day, later renamed
`baud-core2`. On `baud-core` the two results are the last section's: the NEON scan wired in was a null, because the trait cannot reach the cost (`390fdd0`), and the stride-three scan is +4% on both CJK rows (`5fa797d`).

## The split was built, and it costs nothing

`baud-isa` exists. It held `Scan`, `Portable` and `Avx2` -- the whole of what
was `baud-core2`'s scan module, the crate then named `baud-core` -- and that
crate depended on it. The `baud-core` of today depends on it behind its scan
features (`neon-scan`, the default, and `avx2-scan`). It holds the
`Scan` trait, the `Portable` scanner struct and the other portable traits
(`Decode`, `CacheProbe`, `Warm`, each added after `2960dd9`), and the intrinsics live in the
per-architecture crates: `Avx2` in `baud-isa-x86-64`, `Neon` in
`baud-isa-aarch64` (corrected 2026-10-10).

**Free, measured.** Five payloads, interleaved, first round discarded, every
range overlapping: streaming 1483.9-1511.6 against 1487.6-1507.2, cat
1091.8-1167.3 against 1084.3-1134.7, btop 754.6-776.4 against 760.8-771.6,
cat-cjk 406.9-414.3 against 403.7-411.2, cjk 2414.7-2436.6 against
2416.9-2433.1, vim 447.4-450.0 against 444.1-453.4. The concern about
cross-crate inlining was worth checking and did not materialise, which is what
LTO plus `#[inline(always)]` on transparent newtypes is supposed to give.

**And the type system got better, which was the point.** The surface is
domain-typed on both sides: `Scan` takes `&[StreamByte]`, not `&[u8]`, and the
vector itself is now a domain type -- `Vectorized`, `repr(transparent)` over
its sixteen lanes, whose predicates return `LaneMask` directly. (It was
`StreamVector`, over a `Simd<u8, 16>`, until the block decoder made it an
array tagged with a `Kind`.) A scanner no longer writes `as` at all. The raw-integer count over both crates fell from
**8 to 5**, and the three that went were exactly the movemask casts the audit
was created to watch.

The gate moved in the same commit: `audit_all.sh` walks `baud-core/src` **and**
`baud-isa/src` against the one budget. Without that it would have reported 8
falling to 2 and called relocation an improvement, which is this repo's
"instrumentation outlived what fed it" failure exactly.

~~What is *not* yet done is the second ISA implementation this was for. The
`shrn` experiment below is the first candidate, and it now has somewhere to
live.~~ Done: the second implementation is `Neon`, the `shrn` experiment, in
`baud-isa-aarch64` (`2960dd9`; corrected 2026-10-10).

## The conclusion, and the part that is not settled

**Add implementations behind `Scan`, not crates around it.** Explicit NEON,
AVX-512 and SVE are each one module and one entry point, and the trait was
built for exactly that. The vectorised UTF-8 decoder the cost map points at
would land the same way, as trait methods with a portable form and per-ISA
overrides.

Two things this note did **not** establish. **Both have since been answered, and
one of them answered against this note's own suggested instrument.**

* **~~Whether cross-crate LTO would preserve the inlining.~~ Still open, but not
  by the means named here.** This said it is "testable with the `vpmovmskb` grep
  already in CI". It is not: a fair A/B puts `-C target-feature=+avx2` on both
  arms, and with the flag on, LLVM VEX-encodes the *portable* path's movemask
  too. Measured on the x86-64 runner, the two arms carry **44 `vpmovmskb` each**
  -- the grep distinguishes nothing. What discriminates is the register width,
  `vpmovmskb %ymm` for the 32-byte form, and that is what the bench workflow
  asserts now.
* **~~Whether an explicit NEON `Scan` beats `core::simd` on this box.~~
  Measured, and the answer is: only where the extraction is the cost.** Wiring
  `Scan::run` -- the printable-ASCII scan -- was a **null on all nine
  recordings**, because the payloads with long printable runs are store-bound.
  Wiring `stride_three`, where `core::simd`'s `to_bitmask` lowers to `addv.8h`
  and `vshrn_n_u16` does not, is **+4.2% on cat-cjk and +4.0% on less-cjk**.
  So the per-ISA question is not "does a hand-written form beat `core::simd`"
  but "which scan, and is its extraction on the critical path" -- and the
  x86-64 counterpart does not exist, because `pmovmskb` is already the cheap
  form there. Both results are the last section's.

## The three-byte vectors say they are UTF-8 (2026-10-31)

Nothing a user sees changes: the same cells, from the same machine code.
What changed is what the type claims. A `Vectorized` block carries a tag
for how its sixteen bytes are read, and the scans and the decode CJK,
Hangul and box-drawing text runs through -- `stride_three_run`,
`stride_four_run`, `routes` and `stride_three_quad` -- sat on
`Vectorized<Printable>` or on every tag, though they ask only about UTF-8
leads and continuations, which printable ASCII never holds. They are on
`Vectorized<Utf8>` now, beside `leads` and `decode`.

**The tag is true because of where the call comes from.** `baud-core`
calls three of them only from `Act::Char3`, and `stride_four_run` not at
all; only the UTF-8 graph has that edge, since under ISO 8859-1
`baud-core/src/graph.rs` returns before the lead match, a high byte being a
C1 control or one character (`Act::Latin1`). The tag names the encoding,
not well-formedness. The stride scans and the route map check only the lead
and continuation pattern; the quad decode (`Codepoint::from_three`) and the
scalar decode after it in `put_run` check the second-byte ranges Table 3-7
narrows for `E0` and `ED`.

**Refused at compile time.** Four doctests on `Vectorized` ask each of the
four of a printable block and must fail with E0599, beside one that asks
them of a UTF-8 block and must pass. Turned to `Utf8`, each of the four
reported "Test compiled successfully, but it's marked compile_fail";
naming E0425 on one reported the expected code missing. Each call site put
back on `Printable` stops the build with E0599: the quad decode in the
default build, the direct stride scan without `neon-scan`, the route map
under `route-stride`, and `Portable::stride_three` in `baud-isa`.

**Free by the machine code, so not timed.** `ingest2`, built from
`git archive` of base and change at one path with an A/A copy of the change,
on three arms -- default (`neon-scan`, only the quad decode live), the portable
scan every build off aarch64 ships, and `route-stride`, has a
`__TEXT,__text` that `cmp` finds identical across the three builds of each
arm, every section keeping its size and address. The base differs only in
panic locations' line and column numbers, the UUID and the signature, so it
rebuilt; the arms' texts differ from one another, so each is what it says.
`Neon::stride_three` is still the default scan, the AVX2 crate is
untouched, and `neon_agrees_with_portable_on_every_prefix_of_a_four_symbol_alphabet`
holds the retyped portable scan to NEON's; its AVX2 twin runs only in CI.

**Found while measuring it, and fixed by the next section.**
`--no-default-features` on `baud-bench` did not reach the portable scan:
`baud-render` and `baud-present-macos` took `baud-core` with its defaults
through the workspace dependency, so `neon-scan` came back on
(`cargo tree -e features -i baud-core`), and the two non-default arms above
set that dependency to `default-features = false`, the same edit on every
build. Since the next section the workspace and the bench take the core and
both libraries without their defaults, so an arm needs no such edit.
**Not fixed.** `--all-targets` turns the defaults back on in `baud-core`
itself: its dev-dependency on itself,
`baud-core = { path = ".", features = ["alloc"] }`, restores them, so on
aarch64 a clippy or check arm meant to compile the portable or
`route-stride` call site has to be `--lib`. `in_range` and `either` still
read control sequences and control strings as `Vectorized<Printable>`.

## The portable arm runs the portable scan (2026-10-31)

Nothing a user sees changes: the release app's `__TEXT,__text` is master's.
The bench's portable arm, `cargo bench -p baud-bench --no-default-features`,
ran NEON. The flag turns off "the `default` feature of the selected
packages", and a dependency is built with "the union of all features
enabled on" it (`[[source: cargo-1ef25ea-features]]`, "Feature unification"):
`baud-bench` took `baud-core` without its defaults, `baud-render` and
`baud-present-macos` with them. In master's two `ingest2` arms baud's own
functions carry the same 12 `shrn.8b` and 7 `addv.8h`. That held from
2026-09-12, when `baud-bench` first took `baud-render`; the stride-three
scan's +4.2% is from 2026-09-10.

**Every build keeps its features.** The workspace takes `baud-core`,
`baud-render` and `baud-present-macos` without their defaults, since a
member's own `default-features = false` "may be ignored" otherwise
(`[[source: cargo-1ef25ea-specifying-dependencies]]`). The two libraries
make `neon-scan` their default, so a program built on either runs NEON; the
app names `baud-core`'s `default` and none of theirs, and the bench's
default is `neon-scan` plus `baud-core/default`. By `cargo tree`, every
crate in the app's and the default bench's builds has master's features,
and the portable arm's `baud-core` has `alloc` alone. Naming `neon-scan` in
each crate instead kept the app's `__text` its size but not its bytes: a
crate's features are in its `-C metadata`, and master's two `ingest2` arms,
whose `baud-core` is the same, differ in `__text` as well.

**Free by the machine code.** From `git archive` at one path, with an A/A
build, the app's and the default `ingest2`'s `__TEXT,__text` are master's,
every section its size; `__DATA_CONST,__const` differs in 14 words, each a
panic's line moved by the 34 lines added to `write.rs`. In the portable
`ingest2` baud's own functions carry no `shrn.8b` and 19 `addv.8h`; the
binary's 26 `shrn.8b` are std's backtrace code and the `object` crate's.

**The gate.** `baud-core` exports `RUN_SCAN` under the three `cfg`s that
pick `run_of_text`'s arm; `baud-bench`'s `SCAN_ARM` reads them over its own
features, and `ingest2` does not compile when the two differ.
`scripts/check-features.sh` checks each crate that has a default without
it. On master with the gate alone that check stopped at "baud-core runs the
NEON scan, and this arm names another", and the default arm built. Four
perturbations each stop it: the bench or the workspace taking
`baud-render`'s defaults, `RUN_SCAN` naming the portable scan on the NEON
arm, and `SCAN_ARM` naming NEON on the portable one.

**The arms now time apart.** `ingest2` at 3000 reps, both arms and an A/A
build of each (`cmp` identical), rotated over six rounds, the first dropped,
on mains at a load average of 1.6-2.3, no other build running at the
start. All four
write the same cells, 2,549,318,481 on `cat` and 1,146,344,986 on
`cat-cjk`. MB/s over the ten runs of each arm:

| recording | NEON | portable |
|---|---|---|
| `cat-cjk` | 514.9-525.8, median 516.6 | 490.3-499.4, median 493.4 |
| `cat` | 1494.7-1694.9, median 1586.1 | 1532.0-1685.7, median 1618.0 |

`cat-cjk`'s ranges are disjoint, NEON 4.7% ahead at the median. `cat`'s
overlap: no result, as the printable-run scan measured alone.

**Found, not fixed.** `baud-core`'s dev-dependency on itself turns its
defaults back on in its own tests, so `cargo test -p baud-core
--no-default-features` still runs NEON, as `scripts/test-features.sh` says.
