# Memory hierarchy — why a read cost 2.5× where a write did not

> **Group:** D · **Sources:** `[[source: apple-cpu-guide]]` · **Status:** partial

## Why this note exists

Group D has been declared in `MANIFEST.toml` since the corpus was set up and
was **empty**. Every claim this project makes about caches, working sets and
write amplification — including rule #2, *working set ≤ cache* — rested on its
own benchmark and on argument. That was tolerable while the claims were
directional. It stopped being tolerable when a measurement produced a result
nobody could explain and three explanations were offered from intuition.

The first thing this source did was **refute** the explanation that prompted
fetching it. That is the note's main result.

## The measurement it explains

`fe7de5d` added a scan to `fill_row_counted` — read the row, blank only up to
the last cell differing from the blank — and cost **66% of streaming
throughput**. Four variants, interleaved, ghostty as an unchanged control
(1105.5–1136.2 across every run):

| arm | row read? | write | spans, streaming |
|---|---|---|---|
| head — backwards scan, shortened write, two `View` borrows | yes | short | 520.2 / 568.8 / 567.1 |
| C2 — same, one borrow | yes | short | 559.5 / 530.4 / 537.3 |
| C3 — **forwards** scan, shortened write | yes | short | 582.7 / 572.6 / 561.1 |
| C4 — backwards scan, **full-row** write | yes | full | 510.8 / 516.0 / 496.5 |
| C1 — **no scan**, full-row write | no | full | **1334.0 / 1387.9 / 1402.4** |

The single variable that tracks the outcome is **whether the row is read at
all**. Direction does not matter, what is written does not matter, and the
number of borrow constructions does not matter.

## The explanation that was wrong

The inferred story was: a full-line write needs no prior contents, so it avoids
a fill, while a read forces one — the scan therefore pulls 1600 bytes of cold
grid into cache per scroll and violates rule #2.

**`[[source: apple-cpu-guide]]` § 4.20 says otherwise**, of the L1 data cache:

> "It usually operates as a write-back **write-allocate** cache. The write
> allocation policy keeps the recently written data in the L1 cache, where it is
> conveniently located for re-reading."

Write-allocate means the store allocates the line too. The read cannot be adding
a fill the write avoided, because the write already did it. The story was
mechanically wrong on this hardware, and it was wrong in the confident-sounding
direction — it cited the project's own rule #2, which made it sound derived
rather than guessed.

Same section, useful and unrelated: "While the system operates on 128B
cachelines, the L1D Cache operates on 64B cachelines." A 200-column row of
8-byte cells is 1600 B — 25 L1 lines.

## The explanation that fits

`[[source: apple-cpu-guide]]` § 4.6.10, "Memory Dependence Prediction":

> "Load and store µops may execute and send their requests to the rest of the
> memory subsystem out of order and speculatively. […] it is possible to have
> dependencies through memory if a **store is closely followed by a load of an
> overlapping data element**. Such dependencies are not caught by the usual
> register-tracking mechanisms within each core. When a store and dependent load
> execute out-of-order, a **memory ordering violation occurs and the offending
> load must be re-executed**. If these violations occur often, performance
> [suffers]."

§ 4.6.11 gives the shape as an in-place-buffer example — "loads from (m, n)
start executing before the stores from (m, n−1) have completed" — and § 4.6.10.1
names the worst pattern as repeated read-modify-write over a small pool.

That is baud's ingest loop. `put_ascii_run` stores cells; the next scroll's
`fill_row_counted` loads those same recently-stored cells, and the pair repeats
once per scrolled line over the same few hundred KB of grid.

It predicts all four arms, which the cache story does not:

* the violation needs the **load to exist**, not to run in any direction — C3;
* it does not care what is subsequently **stored** — C4;
* it is unaffected by how the borrow is constructed — C2;
* deleting the load removes it — C1.

**This is a candidate, not a confirmed cause.** It is consistent with every arm
and with a primary source, which is more than the previous two explanations had,
and it is still an inference. What would settle it is a counter.

## Which reopens a refusal

`CLAUDE.md` said, until `dc0eed4` (2026-08-29) withdrew it: "Hardware
counters are not available on this machine even as root. SIP blocks the
`cpc`, `fbt` and `pid` providers; only `profile` survives `sudo`. Cache
misses, branch mispredicts and IPC need a recovery-mode reboot, so the two
open anomalies … stay open."

The first half is true and the conclusion is **scoped too widely**, in the way
this project keeps recording. dtrace's `cpc` provider is SIP-blocked; dtrace is
not the only route to the PMCs. `[[source: apple-cpu-guide]]` § 6.1–6.2
documents Xcode Instruments and a list of performance-monitoring events as the
supported way to read them.

~~That path is untested here for a boring reason: `xcrun xctrace` is absent on
this box, because only the Command Line Tools are installed and not Xcode. So
the blocker is **an install, not an architecture** — and whether Instruments
needs SIP disabled is a question this note does not answer and should not guess.~~
That path went untested when this was written (2026-08-27) because `xcrun
xctrace` looked absent. It was not, and it records with no `sudo` on this box,
whose SIP still blocks dtrace's `cpc`, as the paragraph below records (struck
2026-10-10).

**Answered since.** `dc0eed4` (2026-08-29) found that `xctrace record
--template 'CPU Counters'` records with no `sudo`. A later record found the
"absent" tool was never absent: `/usr/bin/xctrace` is an `xcode-select` shim
that answers "not found" under a `DEVELOPER_DIR` carrying no Instruments, and
Xcode's own copy records fine (the record, "The counters run on
baud-core now, and the elimination holds on the new core", which also reads
the counters on `baud-core`).

Two anomalies are recorded as unresolvable on the strength of the wider claim:
the width tables at 5% and blanking at 38%. Both should be re-read against the
narrower one.

## Claims relied on

| claim | source | where |
|---|---|---|
| The L1D is write-back **write-allocate**; the write allocation keeps written data in L1 | `[[source: apple-cpu-guide]]` | § 4.20 |
| The system uses 128B cachelines; the L1D operates on 64B | `[[source: apple-cpu-guide]]` | § 4.20 |
| A store closely followed by an overlapping load can cause a memory ordering violation, and the load is re-executed | `[[source: apple-cpu-guide]]` | § 4.6.10 |
| Repeated read-modify-write over a small data pool is the worst such pattern | `[[source: apple-cpu-guide]]` | § 4.6.10.1, § 4.6.11 |
| Instruments and the performance-monitoring events are the documented route to the counters | `[[source: apple-cpu-guide]]` | § 6.1, § 6.2 |

## Gaps and open questions

- **Unconfirmed.** No counter has been read. The mechanism fits four arms and a
  primary source; it has not been observed.
- **The erase-path scan is untested.** `bcf1774` applied the same pattern to
  `fill_counted` (`EL`/`ED`). The fix reverts both functions, so that one's cost
  is inferred from the shared shape rather than measured.
- **The older blanking anomaly is not explained by this.** The record,
  "Blanking, priced before `RowExtent`", records that a variant *skipping*
  blanking entirely measured 142–150 against 213–229 — less work, slower —
  and resolves it there as a harness artefact. That is a different comparison
  from this one and this note does not account for it. Do not fold the two
  together.
- **Not read:** § 4.6.8 non-temporal accesses and § 4.22's non-temporal metrics,
  which are the documented tool for not polluting a cache with data used once.
  Relevant if a bounded blank is ever attempted again; no claim here rests on
  them.
- **Version 4 exists** and this is 3.0. Nothing cited here is likely to have
  changed, but a claim that starts mattering should be re-checked against it.
