# Instruction-set dispatch — why the seam is a trait and not a crate

> **Group:** I · **Sources:** `[[source: rust-target-feature]]`, `[[source: highway-dispatch]]`, `[[source: arm-neon-bitmask]]` · **Status:** the split is built and measured free; the `shrn` experiment is not started

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
kept printing `scan path: AVX2` from a stale flag, so a 175 → 195 MB/s loss was
attributed to the type system instead. The CI step that greps the
monomorphised binary for `vpmovmskb` exists because of that, and it would
become the *only* thing standing between a crate split and a silently portable
build.

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
that must not move: it holds all state behind `&mut self`, a measured 16-34%
against file-scope globals, and it has to inline *into* the
`#[target_feature]` entry point for the whole loop to get the extended feature
set.

The revised claim is not that a boundary would be slow — it most likely would
not — but that the inlining stops being guaranteed by construction and becomes
something CI has to check. baud already has that check.

The cost is that `baud-core`'s "zero dependencies" becomes "two first-party
dependencies", which is a README claim and an owner's decision rather than a
technical one.

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

baud does not use it. Disassembling `feed_with::<Portable>` on this box, the
mask extraction behind `LaneMask::from_movemask` lowers to `uzp1`, `zip1`,
`and.16b` and **`addv.8h`** — the horizontal-reduction form, which carries a
serial dependency `shrn` does not. `shrn` appears elsewhere in the binary, so
the toolchain will emit it; `core::simd`'s `to_bitmask` simply does not choose
it here.

That is a concrete, measurable per-ISA experiment and it is unstarted:
hand-write the aarch64 mask extraction with `shrn #4`, take `rbit`/`clz` on the
resulting 64-bit value, and shift the nibble index down to a byte index. It is
the first thing to try behind `Scan`, and it needs no crate.

## The split was built, and it costs nothing

`baud-isa` exists. It holds `Scan`, `Portable` and `Avx2` -- the whole of what
was `baud-core/src/scan/` -- and `baud-core` depends on it.

**Free, measured.** Five payloads, interleaved, first round discarded, every
range overlapping: streaming 1483.9-1511.6 against 1487.6-1507.2, cat
1091.8-1167.3 against 1084.3-1134.7, btop 754.6-776.4 against 760.8-771.6,
cat-cjk 406.9-414.3 against 403.7-411.2, cjk 2414.7-2436.6 against
2416.9-2433.1, vim 447.4-450.0 against 444.1-453.4. The concern about
cross-crate inlining was worth checking and did not materialise, which is what
LTO plus `#[inline(always)]` on transparent newtypes is supposed to give.

**And the type system got better, which was the point.** The surface is
domain-typed on both sides: `Scan` takes `&[StreamByte]`, not `&[u8]`, and the
vector itself is now a domain type -- `StreamVector`, `repr(transparent)` over
`Simd<u8, 16>`, whose predicates return `LaneMask` directly. A scanner no
longer writes `as` at all. The raw-integer count over both crates fell from
**8 to 5**, and the three that went were exactly the movemask casts the audit
was created to watch.

The gate moved in the same commit: `audit_all.sh` walks `baud-core/src` **and**
`baud-isa/src` against the one budget. Without that it would have reported 8
falling to 2 and called relocation an improvement, which is this repo's
"instrumentation outlived what fed it" failure exactly.

What is *not* yet done is the second ISA implementation this was for. The
`shrn` experiment below is the first candidate, and it now has somewhere to
live.

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
  form there. `CLAUDE.md` carries both results.
