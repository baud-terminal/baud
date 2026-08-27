# Protocols worth adding, and protocols worth refusing

**Status, 2026-08-26: Tier A and Tier B are implemented, except `OSC 8`.**
Tier C is implemented except the kitty keyboard protocol, which still needs an
input path baud does not have. `docs/vt-coverage.md` carries the per-row truth;
this file keeps the *reasoning*, including for the parts now done, because the
reasons are what a later reader needs in order to not undo them.

A recommendation, not a plan of record. `docs/vt-coverage.md` says what baud
implements of the VT protocol; this says what *other* protocols a terminal
built on it would need, in what order, and which ones to decline.

The ordering rule is not popularity. It is: **what does an application break on
if it is missing?** A protocol whose absence makes a program hang ranks above
one whose absence makes it look worse.

---

## Tier A — an application misbehaves without these

### 1. Alternate screen buffer (`CSI ? 1049 h/l`) — **done**

`[[source: ctlseqs]]`. Every full-screen program — vim, less, htop, tmux —
switches to it on start and back on exit. Without it, quitting vim does not
restore the shell's scrollback: the user loses their session history, which
reads as data loss rather than as a missing feature.

Cost is real: a second screen buffer, and `Storage` is a fixed block by design.
Doubling the viewport is 13.9 MB → 27.8 MB **virtual**, and MEASUREMENTS.md
already records that untouched pages cost ~0.3 MB resident. The alternate screen
has no scrollback, which is what makes it cheaper than it looks.

### 2. Bracketed paste (`CSI ? 2004 h/l`) — **done**

`[[source: ctlseqs]]`. Input-side, so it does not touch ingest — but its absence
is a **security** problem, not a cosmetic one: without it a pasted string
containing a newline executes in the shell as typed input. This is the
paste-jacking class. Cheap to add, and it belongs above anything visual.

### 3. Mouse reporting (`CSI ? 1000/1002/1003/1006 h/l`) — **done**, SGR only

`[[source: ctlseqs]]`, "Mouse Tracking". SGR encoding (1006) is the only one
worth implementing — the older X10/UTF-8 encodings cannot express coordinates
past 223 columns, and baud's grid goes to 2048. Needs the reply channel, which
now exists.

---

## Tier B — the terminal looks broken without these

### 4. OSC 0/1/2, window and icon title — **done**

`[[source: ctlseqs]]`. One line of dispatch on top of the OSC state, which is
already parsed. The payload must be **bounded** — a title is attacker-controlled
and a `no_std` core cannot grow a buffer.

### 5. OSC 8, hyperlinks — **the one Tier A/B row still open**

The [explicit hyperlink spec][osc8] rather than ctlseqs. Increasingly assumed by
`ls`, `gcc`, and test runners. Note the security rule in that spec: a terminal
must not follow arbitrary schemes.

[osc8]: https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda

### 6. OSC 4 / 104, colour palette — **done**

`[[source: ctlseqs]]`. Needed by themes. Also a **query** surface (`OSC 4 ; n ; ?`),
so it exercises the reply channel.

### 7. SGR completeness — **done**

Not a new protocol but the largest unfinished one baud already claims. The gap
has never been enumerated against `[[source: ecma48]]` § 8.3.117, and
`docs/vt-coverage.md` records it as `partial` for exactly that reason. 256-colour
and truecolour (`38;2;r;g;b`) matter most; the sub-parameter form (`38:2::r:g:b`)
is the one implementations get wrong.

---

## Tier C — worth doing once the above is done

- **Synchronised output (`CSI ? 2026`)** — **done**., the [spec][sync]. Lets a renderer
  avoid tearing. Cheap, and it is a *hint*: ignoring it is correct-but-worse,
  which is why it is not Tier A.
- **XTVERSION (`CSI > 0 q`)** and **DECRQM (`CSI $ p`)** — **both done**. Both are queries, both
  are now unblocked by the reply channel, and both let applications
  feature-detect rather than guess from `$TERM`.
- **Kitty keyboard protocol**, the [spec][kkbd]. Fixes what terminals have never
  been able to express: Ctrl+Shift combinations, key release, and disambiguation
  of Escape. Input-side, so it needs an input path baud does not have.

[sync]: https://gist.github.com/christianparpart/d8a62cc1ab659194337d73e399004036
[kkbd]: https://sw.kovidgoyal.net/kitty/keyboard-protocol/

---

## Recommended against, with reasons

Declining is a decision and belongs in the record.

| protocol | why not |
|---|---|
| **Highlight tracking** (`CSI ? 1001`) | `[[source: ctlseqs]]`: "this mode requires a cooperating program, else xterm will hang". A terminal that a sequence can hang is worse than one that ignores the sequence. |
| **Sixel** (`DCS ... q`) | An image format, not a terminal protocol. Large surface, needs a palette and a raster buffer, and it does not interact with the ingest architecture baud exists to test. The `DCS` payload is already consumed safely, which is the part that matters for correctness. |
| **ReGIS** | Vector graphics. No modern application emits it. |
| **Kitty graphics** (`APC G`) | Same reasoning as sixel. Note baud already consumes `APC` payloads without executing them, which was a real bug before Tier 0 landed — the protocol is *safe to ignore*, which is the bar. |
| **DECUDK**, user-defined keys | Input-side, and superseded by the kitty keyboard protocol for anything new. |
| **National Replacement Character Sets** (DECNRCM) | Superseded by UTF-8. Documented here because `[[source: ctlseqs]]` carries them and a reader will ask. |
| **Tektronix mode** (`ESC [ ? 3 8 h`) | A different terminal emulated inside this one. Historical. |

---

## The rule these follow

Two failure modes recur across every row above, and both are worse than not
implementing a thing at all:

1. **Accepting a sequence and doing nothing**, with no way for the application
   to find out. `docs/vt-coverage.md` calls this `parsed` and treats it as a
   bug; DECSTBM, DL and SM/RM were all in that state.
2. **Answering a query with the wrong shape.** An application parses whatever
   arrives as the report it asked for, so a wrong answer is worse than silence.
   This is why baud's DA under-claims as a VT102 and why an unimplemented DSR
   gets no reply.

Any protocol added here inherits both rules.
