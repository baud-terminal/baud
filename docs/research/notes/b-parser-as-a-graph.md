# The parser as a graph — how to store 20 nodes and 5,120 edges

> **Group:** B · **Sources:** `[[source: shift-dfa]]`, `[[source: sheng]]`,
> `[[source: regex-automata-dense]]`, `[[source: hyperscan]]`,
> `[[source: dpfsm]]`, `[[source: williams]]` · **Status:** partial

## What it is

`baud-core/src/graph.rs` holds the VT protocol as a **directed graph**:
20 nodes, one per parser state, each with 256 out-edges, one per input byte.
An edge carries what to do and where to go. Feeding a byte is `edge =
graph[node][byte]`, and feeding a stream is walking the graph — `graph[n0][b0]`
gives `n1`, `graph[n1][b1]` gives `n2`, and so on.

It is a graph and not a tree, which matters for choosing the representation: it
has cycles. `CsiParams` has an edge to itself, `Ground` is reachable from nearly
every node, and `CsiIgnore` has many parents. A trie over the VT sequences would
be unbounded, because a parameter is an arbitrary-length digit string; the
back-edges are what make the structure finite. Nothing is hashed and nothing is
content-addressed, so it is not a Merkle structure either — it is a
**deterministic finite automaton**, and the literature to read is the literature
on storing one.

Two of its dimensions are measured rather than assumed, and pinned by
`every_row_of_the_graph_is_the_row_its_node_names`:

| | |
|---|---|
| nodes | **20** |
| byte classes, UTF-8 alone | **20** |
| the same, ISO 8859-1 alone | **19** |
| **byte classes, one map serving both** | **22** |
| distinct actions, all reachable | **37** |

**Twenty-two of 256 columns are distinguishable**, so 91% of the table today
says the same thing twice.

The joint figure is the one a single map has to use, and it is larger than
either encoding's own: two bytes share a class only when every node agrees
about them under *both* graphs, so the shared partition is a refinement of each
and finer than both. Assuming otherwise is what made the first version of
`NCLASS` an index-out-of-bounds at compile time rather than a wrong number at
run time — which is the good failure mode, and the reason the constant is
asserted equal to the derived count rather than merely not less than it.

## What baud takes

### The representations, and what each costs on the loop

**This section was written on a premise that has since been withdrawn.** It
read: "the ingest loop is latency-bound at IPC ≈ 1 on an eight-wide core, and
the only changes which have ever helped are the ones that *shorten the critical
path*." The counter work in `CLAUDE.md` retracted that -- purpose-built
controls put `cat-cjk` at **8.8% back-end bound**, not waiting, and the IPC ≈ 1
figure was claimed and retracted twice because it agreed with the prior. So the
loop is **not** latency-bound, and the shift-DFA result below is the direct
consequence: removing a load from the critical path buys nothing when the loop
was not waiting on it, and the measured recurrence is ~13% *slower*. The
representations are still ranked by their effect on the loop-carried
dependency, because that is the axis the sources reason on -- but "shortens the
chain" is no longer a proxy for "faster here," and the ranking is a map of the
design space rather than a prediction of throughput.

| representation | table | loop-carried chain |
|---|---|---|
| `graph[node][byte]`, today | 11,776 B/encoding | multiply, then a **load** |
| `graph[byte][node]` | same | a **load** |
| premultiplied node ids | same | a **load** |
| byte classes | **1,012 B/encoding** | a **load** (off a much smaller table) |
| shift-packed rows | ~352 B | a **shift and a mask** |
| `PSHUFB` row-as-shuffle | 16 B/class | one vector op |

**`graph[node][byte]` puts two things on the chain and one of them is free to
remove.** `[[source: shift-dfa]]` states it directly: the address is
`table + 256 * state + *s`, so "the multiplication by 256 (shift by 8) is also
added to the loop-carried dependency chain for the state, resulting in an extra
cycle of latency. You can fix that by just flipping the table". Flipped to
`[byte][node]` the multiply lands on the *byte*, which is not loop-carried.
That is a layout change with no representation change and no size change, and it
is the cheapest thing on this list.

**Premultiplied node ids do the same job differently.**
`[[source: regex-automata-dense]]`: "the ith state, its corresponding
premultiplied identifier is i * k where k is the alphabet size … it saves a
multiplication instruction per byte when searching with the DFA, which has been
observed to lead to a 20% performance benefit in micro-benchmarks", against the
cost that identifiers need a wider integer. baud's edge already spends a whole
byte on the node id and uses 20 of 256 values, so premultiplying by 20 classes
(460 values) needs 16 bits and doubles the edge to four bytes — or premultiplying
by 20 *within* a 256-wide field does not fit at all. **Flipping the table gets
the same instruction removed without widening anything**, which is why it is
listed first.

**Byte classes shrink the table by 92% and change nothing about the chain.**
`[[source: regex-automata-dense]]`: "each equivalence class represents a set of
bytes that does not discriminate between a match and a non-match in the DFA …
the size of the transition table can be reduced drastically from
`#states * 256 * sizeof(id)` to `#states * k * sizeof(id)`". Here that is 11,776
bytes down to **1,012** an encoding -- both graphs and the class map together
are **2,280 bytes, 36 cache lines**, against 23,552 and 368. The
class lookup is one extra load, but it is indexed by the **byte alone**, so it
is off the loop-carried chain and issues immediately. This is worth doing for
residency even though it does not shorten the critical path — rule #2 of this
project is that the working set stays in cache, and a 23 KB table that 92% of
does nothing is the shape that rule exists to catch.

**Shift-packed rows are the only entry that attacks the chain itself.**
`[[source: shift-dfa]]` packs an entire transition row into one word indexed by
the byte, so the load depends only on the input:

```
state = (table[*s] >> state) & mask
```

with states pre-multiplied by the field width. The author's own summary of why:
"the first-level lookup is a load from memory that _only_ depends on the input
byte … the second-level lookup picks out the right column within the row based
on the current state" using ALU operations at one-cycle latency. **That takes a
four-to-five cycle L1 load off the loop and replaces it with a shift and a mask.**

Its constraint is capacity, and baud exceeds it. `[[source: shift-dfa]]` fits
"up to 10 states" in a 64-bit word at 6 bits per state, and reports that a
two-word extension "reduce[s] to 1/3rd" of single-word throughput because of the
added TEST/CMOV. baud has 20 nodes: 5 bits each is 100 bits, which needs a
128-bit word. Indexed by class rather than by byte that is 22 words, **352
bytes** -- the whole parser in six cache lines. Whether an arm64 `u128` shift keeps enough of the advantage was
the single most interesting unmeasured thing in this note.

**Measured now, and it does not: the shift-DFA recurrence is ~13% slower on
this arm64 core.** `scripts/shift-dfa-microbench.rs` isolates the recurrence
over baud's 20-state shape -- load-based `flat[state*256+b]` against shift-DFA
`(SHIFT[b] >> state*5) & mask`, both walking the same transition table so the
state sequences are identical (asserted) -- and the load reads **~1.45 ns a
step** against the shift's **~1.65**, stable across ten interleaved rounds and
two runs on AC power. Two reasons, and both were foreseeable from the source:

* **The loop is not latency-bound.** At ~1.45 ns a step, ~6.5 cycles, the L1
  load pipelines across iterations rather than stalling the recurrence -- so
  the four-to-five cycle load latency the shift-DFA removes was never on the
  critical path to begin with. This is the same reading the counter work gives
  for the whole parser: `cat-cjk` is 8.8% back-end bound, not waiting.
* **A `u128` shift is not one instruction on arm64**, and baud's 20 states do
  not fit the *pre-multiplied* form that would avoid the `* 5` per step -- 20
  fields wide enough to hold a pre-multiplied id (`19 * width < 2^width`) needs
  width 8, and `20 * 8 = 160 > 128`. So the raw-id form pays a multiply, a
  variable 128-bit shift synthesised from 64-bit halves, and a mask, against a
  single pipelined load. Vognsen's own note that a two-word extension "reduce[s]
  to 1/3rd" is the same effect seen small.

**So the lead is closed with a measurement, not built into the parser.** The
mechanism loses in isolation, and in `baud-core` it would additionally be
diluted to the 0.2-9.7% of bytes that reach `control` (the rest take
`fast_esc`/`fast_csi`/the run scans) *and* fight `step`'s `act()`, which takes
`&mut Node` and can override the transition -- so the state cannot stay a naked
integer across iterations, which is the one thing the shift-DFA needs.
Building the in-parser arm would confirm a worse-than-null the microbench
already establishes cleanly. The microbench is kept runnable, the way the
counter controls are.

**The first run of the microbench read `shift 0.000 ns/step`** -- dead-code
elimination folded the pure-arithmetic loop away while the memory-load loop
survived, a probe that could not fire. `black_box` on both accumulators is what
turned it into a measurement, and it is the same failure this project records
for the counter controls and the search probes.

**The vector form has a hard bound baud is on the wrong side of.**
`[[source: sheng]]` does the transition with one `PSHUFB` per byte, where the
shuffle mask *is* the row — the technique `[[source: dpfsm]]` describes
academically and `[[source: hyperscan]]` ships. Its limit is **16 states**, one
per lane of a 128-bit register, and baud's graph has 20. Splitting the graph so
the hot sub-graph fits is discussed below.

### The two-tier reading, which the payload mix argues for

The route split already measured says which nodes are hot, and it differs by
payload more than anything else in this project:

| payload | UTF-8 route | escapes | one-at-a-time |
|---|---|---|---|
| cat-cjk | **81.2%** | 0.0% | 2.6% |
| btop | 14.5% | **66.3%** | 0.2% |
| asciiquarium | 0.0% | **80.3%** | **9.7%** |

On cat-cjk the live sub-graph is `{Ground, Utf8One, Utf8Two, Utf8Three}` — four
nodes, which is inside every bound in the table above including `PSHUFB`'s. On
asciiquarium the CSI nodes are live and the DCS nodes still are not. So a
split — a small hot automaton with an escape hatch into the full table — is
available in principle, and **it is exactly the shape this project has already
measured as a loss twice**: "a path that decides by scanning either commits or
wastes the scan", and the class-agnostic write path that lost 43% for deciding
by scanning. The difference here is that the decision is a *state test* and not
a scan, so the prior does not obviously transfer. Unmeasured either way.

## What baud does not take

**Nothing from `[[source: dpfsm]]`'s parallel method**, for the reason note B
already records: its § 2.1 assumes the action function can be invoked out of
order, and a terminal's actions cannot. That is unchanged by anything here. What
this note takes from that line of work is the *representation* half —
`[[source: sheng]]`'s row-as-shuffle and the bound on it — which carries no such
assumption.

**Sparse DFAs.** `[[source: regex-automata-dense]]` notes sparse tables "do not
tend to benefit nearly as much from using byte classes" and read their
transitions from packed bytes. A sparse row is a search, and this project has
already measured that indexing beats hashing *because* the index is arithmetic
and the alternative has a search in it. A sparse representation puts the search
back.

**Row displacement / comb vectors**, the classic LR-table compression. baud's
rows are not sparse in the sense that method exploits — every one of the 5,120
edges is defined, because a VT parser has a defined answer for every byte in
every state. There is nothing to overlap.

## What it measured, which was nothing

**The graph replaced the walk and no payload moved.** Interleaved A/B, one
binary per arm, first round discarded, `cells_written` byte-identical: cat-cjk
472.1-474.7 against 467.4-469.7, asciiquarium 380.5-392.1 against 380.7-390.0,
btop 804.6-812.8 against 800.7-814.1, vim 499.1-509.0 against 505.0-509.3,
less-cjk 481.7-490.1 against 475.4-482.0. Every range overlaps.

**And the three representations are indistinguishable from each other**, which
is the more useful half. `ByNode`, `ByByte` and `ByClass` measure 369.8-375.1,
364.3-382.2 and 359.7-366.4 on asciiquarium and 791.7-796.2, 787.7-800.7 and
793.2-800.3 on btop. So the predictions above are answered: the table flip is
not a small win here, byte classes are flat as predicted but for a reason the
prediction did not name, and the shift-packed form is not worth building for
*this* graph.

**The reason is that this graph is not on the hot path.** `control` saw 2.6%
of cat-cjk's bytes, 0.2% of btop's and 9.7% of asciiquarium's in this
measurement (2026-08-29, on the core later renamed `baud-core2`); the rest went through the run scanner, `fast_utf8`, or `fast_csi`/`fast_esc`. No
representation of a machine that handles a twentieth of the bytes can move the
total.

That does not retire the representations -- it retires the *payload*. The
literature above is about storing a DFA, and baud has a second one on the path
that does run: `CLUSTER_NEXT`, 94 states over an 8-bit class alphabet, 24 KB,
priced by a ceiling arm at 14% of cat-cjk. Everything measured here should be
re-measured against **that** graph, where a flat result would mean something
different.

## Gaps and open questions

* **~~The table flip is unmeasured and should be the first thing tried~~**,
  ~~because it is a layout change with no size change, no new type, and a named
  mechanism.~~ Measured, and indistinguishable from the other two layouts
  ("What it measured, which was nothing", above; corrected 2026-10-10, this
  bullet having said "unmeasured" since that section answered it on
  2026-08-29).
* **~~Byte classes are unmeasured~~**, and the prediction was *flat throughput
  and a 92% smaller table*. Measured flat, as predicted but for a reason the
  prediction did not name: the graph is not on the hot path (the same section;
  corrected 2026-10-10).
* **~~The 128-bit shift-DFA is unmeasured~~ -- measured, ~13% slower on this
  arm64 core** (`scripts/shift-dfa-microbench.rs`). It was the only candidate
  that *shortens the critical path*, but the critical path was not the load:
  the loop pipelines it at ~6.5 cycles a step, so replacing a pipelined load
  with a multi-instruction `u128` shift loses. Closed, above.
* **Whether the action can be derived from the edge's destination** rather than
  stored beside it. If it can, the edge is one byte instead of two and every
  representation above halves. 37 actions and 20 nodes say it cannot in general;
  whether it can *per node* is not known.
* **The action dispatch is not a graph problem and is not addressed here.** The
  table names an action; performing it is a call into the grid. A `match` over
  37 variants lowers to a jump table, which is a dependent load feeding an
  indirect branch — the mechanism that sank the palette trie and the block
  decoder. If the switch to the table measures negative, that is where to look
  first, and it is a separate question from how the edges are stored.
