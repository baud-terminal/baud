# Data-parallel finite-state machines — vectorising the parser itself

> **Group:** B · **Sources:** `[[source: dpfsm]]`, `[[source: simdjson]]` · **Status:** partial

## What it is

Mytkowicz, Musuvathi and Schulte give a parallel algorithm for finite-state
machines, published at ASPLOS '14 `[[source: dpfsm]]`. An FSM run is normally
strictly sequential: the state after symbol *i* is needed before symbol *i+1*
can be processed, so the loop `state = T[in][state]` cannot be split. Their
algorithm breaks that dependence by **enumerating transitions from all possible
start states at once**, which lets the input be chunked and the chunks processed
independently. Only the run that started from the correct state is kept.

On regular expressions, Huffman decoding and HTML tokenization they report
**up to 3× over optimised sequential baselines on a single core**, and linear
speedups to 21× on 8 cores `[[source: dpfsm]]` Abstract.

## What baud takes

This is the one technique in the corpus so far that attacks the part of baud
nobody else attacks. Every competitor vectorises *scanning* — finding the next
interesting byte. ghostty does it, simdjson does it `[[source: simdjson]]` § 3.
Nobody vectorises the **state machine**, and a VT parser is a state machine.

- **Enumerative computation.** Run the transition function from every possible
  start state simultaneously in SIMD lanes, so a chunk boundary no longer
  requires knowing the incoming state `[[source: dpfsm]]` § 1, Fig. 2. baud's
  `State` enum is small — Ground, Escape, CSI, OSC and a handful more — and the
  method needs one lane per state, so the whole state set fits in a single
  128-bit NEON or 256-bit AVX2 register.
- **`shuffle` as a gather substitute.** The transition table lookup wants a
  gather, which the paper notes desktop processors of the time did not have
  generically; they use `shuffle` instead, at *one or two shuffle operations per
  input symbol* `[[source: dpfsm]]` § 4.2. A 16-entry table is exactly what a
  NEON `tbl` / AVX2 `pshufb` does natively, so the substitute is not a
  compromise at baud's state count — it is the natural instruction.
- **Associativity.** Transition composition is associative, so enumerations can
  be combined pairwise and multiple symbols folded at once `[[source: dpfsm]]`
  § 2.2, § 5.1. That is what turns the technique from "parallel across cores"
  into "parallel within one core", which is what baud needs — the measured
  bottleneck is single-core ingest, not a lack of threads.

## What baud explicitly rejects

- **Coarse-grained (multi-core) parallelism** `[[source: dpfsm]]` § 3.4. The
  21× figure is 8 cores on one input. baud's constraint is a single PTY reader
  and `MEASUREMENTS.md` records that the publish protocol's payoff could not be
  evaluated on a single-core box. Splitting ingest across cores also breaks the
  ordering a terminal requires. Only § 3.3's fine-grained (SIMD) half applies.
- **Speculation with rollback.** The enumerative method is speculation-free —
  it computes all starts rather than guessing one. baud must not adopt a variant
  that re-runs input on misprediction: `ingest_is_chunk_invariant` is the
  property the whole parser rests on, and a rollback path is a second code path
  that can disagree with the first. That failure has already happened here once,
  when `ESC 7`/`8`/`M` existed only in the AVX2 loop.

## Claims relied on

| claim | source | where |
|---|---|---|
| Enumerating all start states breaks the cross-iteration dependence in an FSM | `[[source: dpfsm]]` | Abstract, § 1 |
| Up to 3× over optimised sequential baselines, single core | `[[source: dpfsm]]` | Abstract |
| Costs one or two `shuffle` ops per input symbol | `[[source: dpfsm]]` | § 4.2 |
| Transition composition is associative, enabling pairwise folding | `[[source: dpfsm]]` | § 2.2, § 5.1 |
| Convergence between start states is common; convergence to a *single* state is rare | `[[source: dpfsm]]` | § 5.2 |
| HTML tokenization — the closest analogue to VT parsing — was >2× a baseline | `[[source: simdjson]]` | § 2.1, citing [22] |

## Gaps and open questions

- **The 3× is against *their* sequential baseline, not against a scanner-based
  parser.** baud already skips whole printable runs without touching the state
  machine, so the state machine is not 100% of its cost. What fraction it is has
  **not been measured** — that measurement must come before any implementation,
  and it is the number that decides whether this is worth doing at all.
- **Convergence is a performance property, not a correctness one, and it is
  input-dependent** `[[source: dpfsm]]` § 5.2. VT streams are mostly printable
  text, where every start state behaves alike; whether that helps or whether the
  enumeration is simply redundant work on such input is unestablished.
- The paper predates AVX-512 and NEON `tbl` being commonplace. Its `shuffle`
  workaround may now be a native gather on both of baud's targets. **Unverified.**
- **Not read:** the HTML-tokenization result is cited here from
  `[[source: simdjson]]` § 2.1's summary of it, not from the original. Any claim
  resting on the *magnitude* of that result needs the original fetched first.
