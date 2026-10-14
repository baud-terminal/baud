# AG. Under load: the reply buffer and the Sixel body

**Status: research, 2026-10-09.** Two owner questions (section 3), each put to
Jev twice with no lean. Built from two investigations of the owner's own
programs: herdr, the owner's multiplexer, and yazi, whose previews the owner
reported blank. Both were run headless against `baud-core` in a pty, set up as
`baud-app` sets it. The evidence (harnesses, captures, run logs) is under
`.claude/research/reply/scratch/`, `.claude/research/yazi/scratch/` and
`.claude/research/scroll/scratch/`, untracked: the captures carry the owner's
workspace names, so none is committed, and any fixture is re-recorded from a
session that restores nothing.

Evidence tags: **M** measured by those harnesses; **C** computed from the code.

## 1. The reply buffer drops replies

### 1.1 What happens

The core queues every reply it owes the child -- device reports, colour
answers, DECRQSS -- in `Storage`'s reply buffer, `REPLY` = 256 bytes
(`baud-core/src/storage.rs`), and the app drains it to the pty. A reply that
does not fit is dropped and counted in `reply_overflows`.

herdr 0.9.3 asks for the whole palette at startup and after every resize:
`OSC 4;n;?` for n = 0..255, four rounds, with `OSC 10`/`11`, `?996n` and `16t`.
In one 4096-byte read the answers overflow, so each round got entries 0-6 only:
23 of 512 answers at 4096-byte reads, all 512 at 1-byte reads, and
`reply_overflows` = 498 over startup (**M**). The reply stream depends on where
the reads fell.

### 1.2 Every reply, and the worst ratio

| Operation | Shortest query (7-bit / Latin-1 C1) | Longest reply | Worst ratio |
|---|---|---|---|
| DA1 | `CSI c`, 3 / 2 | 7 B (**C**) | 3.5 |
| DSR 5 | 4 / 3 | 4 B (**M**) | 1.33 |
| CPR | 4 / 3 | `CSI 640;2048 R`, 11 B (**M**) | 3.67 |
| `?996n` | 7 / 6 | 9 B (**M**) | 1.5 |
| DECRQM | 4 / 3 | 12 B (**M**) | 2.33 |
| XTVERSION | 4 / 3 | 10 B (**M**) | 3.33 |
| XTWINOPS 14/16/18t | 5 / 4 | 16 B (**M**) | 4.0 |
| kitty `CSI ? u` | 4 / 3 | 6 B (**M**) | 2.0 |
| `?2048` report | 8 / 7 | 26 B (**M**) | 3.71 |
| `?2048` x39 in one SM | 198 / 197 | 1,014 B (**C**) | 5.15 |
| `OSC 4;c;?` | 8 / 7 | 28 B (**M**) | 3.71 |
| `OSC 10/11;?` | 7 / 6 | 25 B (**M**) | 4.17 |
| **DECRQSS** for a rich rendition | `DCS $ q m ST` 7 / `90 $ q m 9C` 5 | **114 B (M)** | **22.8** |
| VT52 `ESC Z` | 2 | 3 B (**M**) | 1.5 |

Replies queued outside `feed`: focus 3 B, a mouse report up to 16 B, a key up
to 48 B, a `?2048` report after a resize or a cell-size change 29 B, a `?2031`
notification 9 B.

**The bound for one read** of L bytes is the largest single action's replies
plus one DECRQSS answer per 5 bytes: `1,014 + floor((L-1)/5) x 114`. At
`READ_CHUNK` = 4096 that is 94,380 B (**C**), and an all-DECRQSS read measured
93,480 B (**M**). The safe largest action uses `Report::MAX_LEN`, 39 x 29 =
1,131 B, rather than the 1,014 measured; with it and the replies queued outside
`feed` before a drain (106 B), the buffer that provably never drops within one
read is 1,131 + 93,366 + 106 = 94,603 B; 96 KiB holds it. herdr's worst read needs 7,137 B (**M**).

**Correction, 2026-10-09: the richest DECRQSS answer is 111 B, not 114.**
The 114 was measured on a rendition holding `4:3` and `21` together, which
held curly and double at once because `SGR 21` set its bit beside the
underline family instead of replacing it. `21` replaces the family now
(`docs/record.md`, "SGR 21 is the double style, and the underline family
holds one"), so no rendition holds two styles, and the richest one, with
`4:3` as the longest spelling, answers in 111 B (**M**, by
`the_longest_decrqss_reply_is_within_its_bound`). From it: a worst ratio of
22.2, and the one-read bound `1,014 + floor((L-1)/5) x 111` = 91,923 B at
`READ_CHUNK` (**C**), 2,457 B under the 94,380: three bytes fewer for each of
the 819 answers. The never-drops buffer falls with it, so 96 KiB still holds
it: with the safe largest action and the carry, 1,131 + 819 x 111 + 106 =
92,146 B. (The 94,603 above is 1,131 + 93,366 + 106, the safe figure; the
section first gave it without naming the 1,131, now corrected.) The
all-DECRQSS read's 93,480 B (**M**) was taken before the fix, on 114-byte
answers, and has not been re-taken.

### 1.3 Defects found on the way, decided by no question

The first two and the last are fixed (2026-10-09, `docs/record.md`, "DECRQSS
is sent whole, and the app drains after every read"); the rest are open.

- **Fixed: DECRQSS answered with the wrong shape** when its reply did not fit.
  `baud-core/src/dcs.rs`'s emitter checked each fragment against the constant
  `REPLY` rather than the room left, so a fragment that did not fit was skipped
  and later, shorter fragments were written: with 200 bytes pending the child
  received a DCS with no terminator; with 252 pending, `ESC P 0 ;` with the
  `1$r` gone (**M**). An unterminated DCS swallows every later reply on the
  application's side. CLAUDE.md: "Never answer a query with the wrong shape."
  The answer is now built in a `DECRQSS_MAX`-byte buffer (124, derived from
  `Attrs::WIRE` and the encoder's widest spellings, const-asserted under
  `REPLY`) and sent whole or dropped and counted once; a sweep across every
  pending length up to 255 bytes holds it, in both C1 forms. The other three writers
  (`want_reply`, `Term::queue`, `Term::key`) already copied one encoded reply
  under one check, and the same sweep holds each.
- **Fixed: the app did not drain after every read.** On macOS the pump fed every
  queued chunk and then flushed once; on Linux a key queued after a
  reply-heavy read was dropped, and the key's result ignored. Both drivers feed
  through `baud-app/src/reads.rs`, which hands each read's answers to the
  outbox before the next read (no extra system call: the write is still once a
  turn), and queue a key -- and on Linux a focus report -- into an emptied
  buffer, where it always fits. This stops the loss *between* reads only: one
  4096-byte read of herdr's queries still overflows, which is section 3's
  question.
- **One action can overflow 256 B even drained after every byte**: `?2048` x39
  in a single SM sent 9 reports and counted 30 (**M**).
- **Two departures rest on the buffer.** `OSC 4` takes only its first pair
  because "n pairs means n replies from a fixed reply buffer"
  (`baud-domain/src/vt/osc.rs`), where [[source: ctlseqs]] says "Any number of
  c/spec pairs may be given"; and `OSC 10;?;?` answers only 10, where ctlseqs
  says "Each successive parameter changes the next color".
- **Fixed (2026-10-10): keys stayed seven-bit after `S8C1T`**, found
  correcting the comments: every reply followed `S7C1T`/`S8C1T` and `Term::key`
  passed `C1Transmit::SevenBit`, so after `ESC SP G` a DA answered `9B` and an
  arrow key sent `ESC [`. [[source: ctlseqs]]: xterm "generates 7-bit controls
  (by default) or 8-bit if S8C1T is enabled"; xterm's keyboard writes through
  the `unparseseq` its replies use ([[source: xterm-input]]), one
  `control_eight_bits` switch for both ([[source: xterm-reset]], charproc.c
  9356-9373). `Term::key` reads `cold.c1`; `a_key_follows_s8c1t_as_a_reply_does`
  holds it with DA as the control.
- **Fixed:** the comment in `baud-core/src/paint.rs` calling `want_reply` "the
  only way bytes reach the reply buffer" was false: `dcs.rs`'s emitter and
  `Term::queue`/`Term::key` write it too. Its paragraph on seven-bit
  introducers was stale as well (the core implements `S7C1T` and `S8C1T`, and
  `want_reply` encodes with them), and so was `Term::key`'s claim that
  `want_reply` passes the same seven-bit constant.

### 1.4 What the texts say

[[source: ctlseqs]] never lets a reply be left out: DECRQSS "xterm responds with
DCS 1 $ r Pt ST"; CPR "Result is CSI r ; c R"; `OSC 4` "replies with a control
sequence of the same form". [[source: ecma48]] 8.3.35 is the only text that
contemplates a device that cannot answer now, and only as a status for DSR:
"1 busy, another DSR must be requested later". So a dropped reply is a query
recognised and not answered -- the "parsed and dropped" shape CLAUDE.md rates
worst -- and a fragment is the wrong shape.

### 1.5 The options

- **A, a derived bound.** `REPLY` becomes a const built from named parts (the
  largest single action, DECRQSS's maximum and its 5-byte minimum query,
  `READ_CHUNK`, the replies queued outside `feed`), about 96 KiB, const-asserted,
  with a test that matches over every reply and checks each extreme against its
  budget. It needs both fixes in 1.3's first two items (both made
  2026-10-09). Cost: about 94 KiB per
  terminal, 0.4% of a 24 MB `Storage`. Two release builds that differ only in
  `REPLY`: 542 functions byte-identical, and the inlined feed loop differs in 4
  places, two allocation immediates and one assert -- no per-byte change, by
  disassembly. It holds within its contract (at most one read fed between
  drains); an embedder that feeds more still overflows, counted. A future reply
  whose length depends on stored state (XTGETTCAP, kitty graphics, multi-pair
  `OSC 4`) must join the budget test or the bound breaks silently.
- **B, backpressure.** `feed` stops after any action that leaves less room than
  the largest single action and returns how far it got; the app drains and
  feeds the rest. It holds for any embedder that drains, and the buffer can
  shrink to about 4 KiB. Cost: one compare and branch per non-SGR CSI on the
  fast path, unmeasured (CLAUDE.md's A/A protocol on btop, vim-unicode,
  asciiquarium and less-pager, git-log as the control); every caller of `feed`
  changes; a bench that replays a recording with queries must clear replies
  each rep.
- Practice, not standard: xterm flushes its staging buffer early and grows its
  output buffer ([[source: xterm-reset]]); foot and contour queue without bound;
  none uses read-side backpressure and none drops on a fixed buffer.
- Under either option, lossless replies plus the app's unbounded outbox let a
  child that sends queries and never reads grow baud's memory, about 23 bytes
  for each byte it writes; only an app rule (stop reading the pty while the
  outbox is over a bound) limits that, as it does in xterm and foot.

**Jev, twice, no lean:** A at 0.19, then B at 0.22 (`.claude/research/reply/`).
The design study recommends A, departures first (the residual overflow outside
its contract, and the two colour departures until they are built).

### 1.6 Red-first tests (either option)

DECRQSS whole or absent with 176-252 bytes pending (built 2026-10-09, as a
sweep across every pending length to 255); `?2048` x39 drained after
every byte gives 39 reports; the worst read gives 0 overflows; a budget test
over every reply; herdr's startup, re-recorded, fed at 4096 and 1024 bytes,
answers every palette index in order with the query's own terminator; a
property that the reply stream does not depend on where the reads fell, with a
generator that emits `ESC % @`, C1 forms, a rich rendition, DECRQSS and `?2048`
lists, at `PROPTEST_CASES=20000`.

## 2. The Sixel body refuses yazi's previews

### 2.1 What happens

yazi picks Sixel in baud (an unknown brand that offers Sixel in DA1), caps a
preview at 600x900 pixels, and encodes at 1 to 4.6 bytes a pixel, because it
writes every pixel row as its own pass with a colour select before every run.
baud refuses any Sixel whose raw body exceeds `host::IMAGE_BUDGET.body`, 256
KiB (`baud-app/src/host.rs`, `baud-core/src/sixel.rs`), and yazi erases the old
preview before it sends the new one, so the pane goes blank (**M**, the real yazi
26.5.6):

| Synthetic test file | Image sent | Body | At 256 KiB |
|---|---|---|---|
| a 64x64 image | 64x64 | 10-11 KiB | stored |
| flat colour blocks | 600x800 | 30 KiB | stored |
| RGBA | 600x600 | 47 KiB | stored |
| a sparse PDF page | 600x776 | 11 KiB | stored |
| a UI screenshot | 600x375 | 334-390 KiB | **refused** |
| a photo | 600x800 | 474-680 KiB | **refused** |
| a full page of text | 600x776 | 797-820 KiB | **refused** |
| noise, the worst case | 600x900 | 2.35 MiB | refused |

Raising only the body limit stored every one, and old images were freed on each
step. The refusal was counted with arena and slot refusals in one counter that
nothing read; the budget line went to stderr, which a Dock launch discards.

The owner's `~/.config/baud/config.toml` is gone (only `config.toml.old`, with
`advertise_sixel = true`), so baud now starts with Sixel unadvertised and yazi
draws text art: the blanks do not show today, and would return with Sixel
advertised by default.

### 2.2 The text

[[source: vt3xx-sixel]], raster attributes: "Ph and Pv define the horizontal and
vertical size of the image (in pixels), respectively. Ph and Pv do not limit the
size of the image defined by the sixel data." No text read limits a Sixel's
body or says what a terminal does when an image exceeds its capacity.

### 2.3 The options

- Raise the default body to 3 MiB: holds yazi's worst case at its cap.
- Raise it to 5 MiB: holds the least compact encoding of any image the 1M-pixel
  arena can hold, so the arena is the only limit in practice.
- Decode as the body streams: no raw body is buffered, only the decoded-pixel
  arena limits an image; Ph and Pv size the reservation, and data beyond them
  must still be accepted. A larger change: the decoder keeps state across reads.
- Raise now, stream next.

Resident memory of a larger scratch was unmeasured here; it is measured in
the record, "The Sixel body default is 3 MiB until a Sixel streams", where
the owner's decision of 2026-10-13 is recorded: decode as the body streams,
and raise the default to 3 MiB until then. The raise is built (2026-10-14);
the streaming decoder is not. **Jev, twice, no lean:**
"raise now, stream next" at 0.55, then 0.24, with streaming close behind
(`.claude/research/yazi/`).

### 2.4 Decided by no question

Refusals are made visible: a counter per limit, and the app logging each
refusal with the limit hit and the sizes. Note S's claim that a refusal is
"diagnosable" (`docs/research/notes/s-sixel.md`) is corrected with it.

## 3. The owner's questions

1. **The reply buffer: A (a derived bound, about 96 KiB) or B (backpressure)?**
   Neither departs from a text; A changes nothing per byte and keeps the API, B
   holds for any embedder at the cost of an unmeasured branch and an API change.
2. **The Sixel body: 3 MiB, 5 MiB, stream-decode, or raise now and stream
   next?** No text limits the body; the measurements are in 2.1.
