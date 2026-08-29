# Instruction-set dispatch — why the seam is a trait and not a crate

> **Group:** I · **Sources:** `[[source: rust-target-feature]]`, `[[source: highway-dispatch]]`, `[[source: arm-neon-bitmask]]` · **Status:** partial — the packaging question is answered, the `shrn` experiment is not

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
baud-core/src/scan/mod.rs        trait Scan { run, final_byte, st }
baud-core/src/scan/portable.rs   core::simd -- NEON on aarch64, SSE2 on x86-64
baud-core/src/scan/avx2.rs       the only unsafe in baud-core
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
architecture's name. The reusable unit is the **primitives**: byte
classification, mask extraction, the UTF-8 gather. Those are pure functions
over slices with no state.

Which is also what makes the split safe, and the reason to draw the line there
rather than anywhere else:

* **Pure `#[inline(always)]` functions cross a crate boundary well.** They have
  no state to keep behind `&mut self` and nothing to lose by being generic.
* **`feed_with` must not cross one.** It is the hot loop, it holds all state
  behind `&mut self` — a measured 16-34% against file-scope globals — and it
  has to inline *into* the `#[target_feature]` entry point for the whole loop
  to get the extended feature set. Moving it makes that LTO-dependent, and
  unverified.

So: a primitives crate is defensible and a `baud-core` split is not, and they
are not the same proposal. The cost of the primitives crate is that
`baud-core`'s "zero dependencies" becomes "one first-party dependency", which
is a claim in the README and an owner's decision rather than a technical one.

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

## The conclusion, and the part that is not settled

**Add implementations behind `Scan`, not crates around it.** Explicit NEON,
AVX-512 and SVE are each one module and one entry point, and the trait was
built for exactly that. The vectorised UTF-8 decoder the cost map points at
would land the same way, as trait methods with a portable form and per-ISA
overrides.

Two things this note does **not** establish:

* **Whether cross-crate LTO would in fact preserve the inlining.** The claim
  above is that it becomes unverified, not that it fails. It is testable with
  the `vpmovmskb` grep already in CI, and if the split is ever wanted for
  ownership reasons rather than performance ones, that test is what to build
  first.
* **Whether an explicit NEON `Scan` beats `core::simd` on this box.** `Portable`
  lowers to NEON already; whether a hand-written form beats it is unmeasured,
  and is the first per-ISA question worth asking.
