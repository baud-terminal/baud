# Data-parallel finite-state machines — vectorising the parser itself

> **Group:** B · **Sources:** `[[source: dpfsm]]`, `[[source: simdjson]]`, `[[source: hyperscan]]`, `[[source: parabix]]` · **Status:** partial

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

This was carried as the one technique in the corpus that attacks the part of
baud nobody else attacks. **Its central assumption turns out to be one baud
cannot satisfy** -- see "Gaps and open questions". What follows is left as
written, because what it was believed to offer is the point. Every competitor vectorises *scanning* — finding the next
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

## Nothing here is implemented yet

Worth stating plainly, because the sections above read like a design. **baud
vectorises scanning, not the state machine.** The `Scan` trait is three
functions -- `run`, `final_byte`, `st` -- and every one of them answers "where
is the next interesting byte". `feed_with` uses them to skip ahead and then
falls back to `control(byte)`, one byte at a time, for everything they cannot
skip. That is the same thing ghostty and simdjson do, and it is the 10-27% of
real bytes measured above.

The enumerative method is a proposal. Its two preconditions are now measured
and favourable; no line of it exists.

## The production counterpart

`[[source: hyperscan]]` is the same problem taken to production: a vectorised
automaton engine by one of simdjson's authors, measured against real workloads
rather than benchmarks chosen for it. It is wanted here because note B's case
otherwise rests on one academic result and an argument.

Its decomposition is also the interesting part for baud: string matching in
SIMD, automata for what is left. That is already the split baud has between the
scanner and the state machine -- so the question Hyperscan answers is not
"should the two be separate" but "how much of what looks like automaton work
can be moved across the line".

**Read, and it carries a technique simpler than the one this note proposes.**
Hyperscan's automaton engine uses a **bit-based NFA where one bit is one
state**, chosen deliberately over transition tables:

> "We develop bit-based NFA where each bit represents a state. We choose the
> bit-based representation as it outperforms traditional NFA representations
> that use byte arrays to store transitions, and look up a transition table for
> each current state in a serialized manner. Also, bit-based NFA leverages SIMD
> instructions to perform vectorized bit operations to further accelerate the
> performance." `[[source: hyperscan]]` § 4.2

**And it does not apply to baud, which this note claimed it did.** The claim
was that baud's thirteen states fit a `u16` mask so a transition becomes a
shift and an or. That is true and useless: `Term::st` is a *single* `State`,
because `[[source: williams]]`'s parser is a **DFA**. The set of live states
always has exactly one member, so a state bitmask holds exactly one bit and
there is nothing for a vectorised bit operation to do.

Hyperscan's bit-based NFA earns its keep where regex alternation makes many
states live at once -- that is the "look up a transition table for each current
state in a serialized manner" it is avoiding, and a DFA never has more than one
current state to look up. Reading a technique's *benefit* without checking that
its *precondition* holds is how the cache story in note D got written, and this
is the same error one note over.

So the applicable technique is the one this note already had: § 4.2's
composition of **transition functions**, not tracking of state sets. A
transition function over thirteen states is a map `13 -> 13`, which is sixteen
bytes -- one lane per state, value is the next state -- and composing two of
them is exactly `compose(f, g)[i] = g[f[i]]`, a single `pshufb` or NEON `tbl`.
That is what "one or two shuffle operations per input symbol" means, and
thirteen states in a sixteen-lane register is why the substitute is natural
here rather than a compromise.

What Hyperscan does contribute is corroboration rather than a technique: its
decomposition into "a series of string and finite automata matching" is the
split baud already has between the scanner and the state machine.

Hyperscan's other technique, graph decomposition into "a series of string and
finite automata matching", is the split baud already has between the scanner
and the state machine, so it confirms the architecture rather than changing it.

## What baud explicitly rejects

- **Coarse-grained (multi-core) parallelism** `[[source: dpfsm]]` § 3.4. The
  21× figure is 8 cores on one input. baud's constraint is a single PTY reader
  and `MEASUREMENTS.md` records that the publish protocol's payoff could not be
  evaluated on a single-core box. Splitting ingest across cores also breaks the
  ordering a terminal requires. Only § 3.3's fine-grained (SIMD) half applies.
- **Speculation with rollback.** The enumerative method is speculation-free —
  it computes all starts rather than guessing one. baud must not adopt a variant
  that re-runs input on misprediction: `the_screen_does_not_depend_on_where_the_reads_fell` is the
  property the whole parser rests on, and a rollback path is a second code path
  that can disagree with the first. That failure has already happened here once,
  when `ESC 7`/`8`/`M` existed only in the AVX2 loop.

## Claims relied on

| claim | source | where |
|---|---|---|
| The method assumes the Mealy action function **can be invoked out of order** | `[[source: dpfsm]]` | § 2.1 |
| Enumerating all start states breaks the cross-iteration dependence in an FSM | `[[source: dpfsm]]` | Abstract, § 1 |
| Up to 3× over optimised sequential baselines, single core | `[[source: dpfsm]]` | Abstract |
| Costs one or two `shuffle` ops per input symbol | `[[source: dpfsm]]` | § 4.2 |
| Transition composition is associative, enabling pairwise folding | `[[source: dpfsm]]` | § 2.2, § 5.1 |
| Convergence between start states is common; convergence to a *single* state is rare | `[[source: dpfsm]]` | § 5.2 |
| HTML tokenization — the closest analogue to VT parsing — was >2× a baseline | `[[source: simdjson]]` | § 2.1, citing [22] |
| A bit-based NFA, one bit per state, outperforms byte-array transition tables — **for an NFA**, which baud's parser is not | `[[source: hyperscan]]` | § 4.2 |
| Regex matching decomposes into string matching plus finite automata | `[[source: hyperscan]]` | Abstract |

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
- **~~Whether convergence is common enough on VT streams is unestablished.~~**
  Measured, against the recordings, and it is the second precondition met.
  `tests/convergence.rs` drives eight parsers into deliberately distinct states
  — ground, escape, CSI entry, CSI params, CSI intermediate, OSC, DCS
  passthrough, APC — then feeds them the same bytes from 200 sample points and
  counts how many bytes until all eight agree.

  | recording | converged | mean | worst |
  |---|---|---|---|
  | btop | **100%** of 200 | 23.6 B | 153 |
  | asciiquarium | **100%** | 4.4 B | 12 |
  | vim | **100%** | 21.6 B | 141 |

  Every sample converged, in tens of bytes. That is what the method needs: the
  enumeration has to be carried only until the lanes collapse, so a mean of
  4–24 bytes means one lane suffices again almost immediately.

  The ordering is worth noting because it is the opposite of the intuition this
  note started with. asciiquarium converges **fastest** (4.4 B) and is the most
  escape-dense (one every 5.4 B). Dense control sequences force every start
  through the same few states; it is long printable stretches that keep them
  apart, because Ground and the string states disagree for as long as the text
  runs.

  **Both preconditions are now measured and both are favourable**: the state
  machine takes 57–90% of real bytes, and enumerations collapse within tens of
  bytes. What remains is not a question about VT streams but about the
  instruction sequence — the `shuffle` item below.
- **~~The precondition nobody has checked~~ — checked, and the method's own
  assumption is one baud cannot satisfy.** `[[source: dpfsm]]` § 2.1:

  > "This paper uses the Mealy formalism [20] and assumes the existence of a
  > function that is invoked with the current input symbol s_i and the state
  > q_i reached after reading that symbol. In the context of parallelizing
  > FSMs, **we will make a simplistic assumption that the function can be
  > invoked out of order.** Modifying sequential clients of FSMs to satisfy
  > this assumption [...]"

  **A terminal's actions cannot be invoked out of order.** Print-then-scroll is
  not scroll-then-print; writing cell A then B is not writing B then A; an SGR
  applies to what follows it and nothing before. Ordering is not an
  implementation detail of a VT parser, it is the semantics. Satisfying the
  assumption would mean buffering every action and replaying it in order, which
  reintroduces the sequential dependence the method exists to remove, or
  proving the actions commute, which they do not.

  So **this note's central proposal does not apply to baud's parser as a
  whole**, and it has been carried as "the one technique that attacks the part
  nobody else attacks" through every revision without the assumption being
  read. The 3x and the 21x are real results about FSMs whose clients tolerate
  out-of-order invocation. baud's does not.

  **What survives is narrower and still worth having.** Scanning carries no
  actions: finding where a control sequence begins and ends, and which bytes
  are parameters, is pure classification. An enumerative pass could establish
  those boundaries over a block, leaving only the dispatches to run in order —
  and the dispatches are the minority of bytes, because parameter digits are
  collected rather than acted on. That is a better-structured scanner rather
  than a parallel state machine, it is a *different* proposal from the one
  above, and it has no measurement yet.

- **The third data-parallel formulation, now readable: bitstreams.**
  `[[source: parabix]]` -- the authors' working draft; the PACT '14 copy is
  behind IEEE and the manifest says so -- attacks the same problem from the
  other side. Instead of enumerating states, it **transposes** the input: eight
  parallel bit streams, one per bit position of every byte, so a character
  class becomes one bitstream computed by boolean operations over the eight,
  and a match becomes a *marker* bitstream with "a 1 bit at the next character
  position to be matched". Repetition is `MatchStar(marker, class)`, "all
  positions that can be reached by advancing the marker bitstream zero or more
  times through the character class bitstream", and it is implemented as an
  **addition with carry** -- which is why the paper spends a section on long
  stream addition across 128- and 256-bit blocks, and why it reports SSE2, AVX2
  and GPU implementations of the same algorithm. Nondeterminism is added on
  top so the full generality of regular expressions is reached.

  **It lands where `[[source: dpfsm]]` lands, for the same reason.** The output
  is a marker stream over a whole block -- *where* matches are, all at once --
  and not a sequence of actions in order. The VT grammar's actions are the part
  that cannot be reordered, so as a parser this is the § 2.1 assumption wearing
  bit-planes. As a *scanner* it is a stronger fit than enumeration: sequence
  starts, terminators and parameter spans are exactly marker streams, and
  `Scan` already computes the character-class half with compare masks. What
  bitstreams would add is the transposition, which costs the same per block
  whether the block holds one escape or none -- the "a path that decides by
  scanning either commits or wastes the scan" class, on a corpus where 84% of
  multi-byte runs are one character long. Unmeasured, and the transposition
  cost is the measurement to take first.

- ~~williams' transitions carry *actions*, and composition loses them.~~ `[[source: dpfsm]]`'s method
  composes transition *functions*, which yields the state you end in. baud's
  machine does not merely move between states -- 31 call sites in
  `machine.rs` execute, print, collect, accumulate parameters and dispatch
  **on the transition**, and those actions are the entire point: they are what
  writes cells.

  31 call sites in `machine.rs` execute, print, collect, accumulate parameters
  and dispatch **on the transition**, and those actions are what write cells.
  Knowing the end state of a sixteen-byte block is not the same as having
  processed it. Answered above.

- **Where the bytes actually are, measured.** `Term::utf8_share` and
  `Term::path_split` together rank the three levers on real capture, and the
  state machine wins on every recording:

  | work | btop | vim | asciiquarium |
  |---|---|---|---|
  | **escape sequences** | **66%** | **44%** | **80%** |
  | UTF-8 decode | 14.5% | 28.2% | 0% |
  | bulk scan (already vectorised) | 19% | 27% | 10% |

  Note A's UTF-8 lookup algorithm is worth building and is the smaller half.
  This is the larger one, on every payload that is not synthetic.
- The paper predates AVX-512 and NEON `tbl` being commonplace. Its `shuffle`
  workaround may now be a native gather on both of baud's targets. **Unverified.**
- **Not read:** the HTML-tokenization result is cited here from
  `[[source: simdjson]]` § 2.1's summary of it, not from the original. Any claim
  resting on the *magnitude* of that result needs the original fetched first.
