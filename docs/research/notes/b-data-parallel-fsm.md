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

That was written as an argument. It is now a measurement: on real capture the
scanner handles **10–27%** of bytes and the state machine **57–90%**, so the
part nobody attacks is also the part that does most of the work. See "Gaps and
open questions" below — the table there is the number this note used to say had
to come before any implementation.

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

- **~~What fraction of the cost the state machine is has not been measured.~~**
  Measured, and it is the number this note said must come first. `Term::
  path_split` reports what share of input takes each of `feed_with`'s three
  exits, and the benchmark prints it every run:

  | payload | bulk scan | escape sequences | byte-at-a-time |
  |---|---|---|---|
  | `payload::streaming` | **95%** | 2% | 3% |
  | btop | 19% | 66% | 15% |
  | asciiquarium | 10% | 80% | 10% |
  | vim | 27% | 44% | 29% |

  **On real capture the state machine handles 57–90% of bytes, and the scanner
  everyone vectorises handles 10–27%.** So the technique attacks the majority
  of real work, and the answer to "is this worth doing at all" is yes.

  The same table is why the question had gone unanswered: against
  `payload::streaming` the state machine is **5%**, and any measurement taken
  on it would have said no. The synthetic payload had been the only thing to
  measure against.

- **~~VT streams are mostly printable text.~~** They are not, and this note
  asserted it. btop is one escape every 16.5 bytes, asciiquarium one every 5.4.
  That assumption came from the synthetic payload, and it is the assumption the
  convergence argument below rests on.
- **Convergence is a performance property, not a correctness one, and it is
  input-dependent** `[[source: dpfsm]]` § 5.2. With the premise above corrected,
  the question is now sharper rather than answered: on escape-dense input the
  start states do *not* all behave alike, which is where enumeration should pay
  — but whether convergence is common enough on VT streams specifically is
  still unestablished, and `tests/recordings/` is now the corpus to establish it
  against.
- The paper predates AVX-512 and NEON `tbl` being commonplace. Its `shuffle`
  workaround may now be a native gather on both of baud's targets. **Unverified.**
- **Not read:** the HTML-tokenization result is cited here from
  `[[source: simdjson]]` § 2.1's summary of it, not from the original. Any claim
  resting on the *magnitude* of that result needs the original fetched first.
