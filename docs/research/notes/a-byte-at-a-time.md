# What the kernel and an ATMega do — and why it is not a throughput idea

> **Group:** A · **Sources:** `[[source: linux-vt-console]]`, `[[source: vt100-avr]]`, `[[source: williams]]` · **Status:** partial

## Why this note exists

A terminal is what a serial console is, and a serial console has to work on
parts with no cache, no branch predictor and no vector unit. The proposal that
prompted this note is a good one: those implementations are under harder
constraints than baud, so their structure might carry an idea baud has missed.

It does not, and the reason is worth writing down rather than rediscovering.

## The two, read rather than described

**The Linux kernel console.** `do_con_write` in `[[source: linux-vt-console]]`
is a `while (count)` over the input, and its body takes **one byte**:

```
u8 orig = *buf; buf++; n++; count--;
tc = vc_translate(vc, &c, &rescan);
if (vc_is_control(vc, tc, c)) { con_flush(...); do_con_trol(...); continue; }
vc_con_write_normal(vc, tc, c, &draw);
```

`vc_con_write_normal()` writes a single cell with `scr_writew` per character.
There is no run collection, no bulk store, and no path that looks at more than
one byte at a time.

**An ATMega terminal.** `[[source: vt100-avr]]` is smaller still:

```
void Terminal::putc(uint8_t c, unsigned count) {
    while (count--) callState(EV_CHAR, 0x0000 | c);
}
```

`callState` is an indirect call through a member pointer that *is* the parser
state -- `state_idle`, `state_escape`, `state_esc_sq_bracket()`. One indirect
call per byte, which on an in-order core with no predictor is the most
expensive thing in the loop, and it is still the right design there because
the alternative is a jump table that does not fit.

## So the finding is a negative, and it is a useful one

Neither implementation has anything baud lacks. Both are strictly
byte-at-a-time; baud already collects printable runs with a vector scan,
writes eight cells to a `Simd<u64,8>` store, and decodes four characters a step
in `put_wide_run`. **On structure, baud is ahead of both**, and there is no
technique here to import.

That is not surprising once stated. The two are optimising different
quantities:

| | optimises for | why |
|---|---|---|
| kernel console | predictability, code size, correctness under lock | it runs in kernel context and is never the bottleneck; the tty layer is |
| ATMega terminal | RAM, under 4 KB total | at 8 MHz the *serial line* is the bottleneck, not the parser |
| baud | bytes a second | there is no slower stage to hide behind |

An embedded console is never throughput-bound: a 115200-baud line delivers
11.5 KB/s, and baud ingests **thirty-six thousand times** that. Nothing
designed against a serial line has had a reason to solve the problem baud has.

## The one idea that does transfer, and it is for later

`vc_draw_region` is a damage rectangle -- `draw->x`, `draw->from`, `draw->to`
-- accumulated across characters and flushed by `con_flush` only when
something forces it: a wrap, insert mode, an inverse cell, or the end of the
write. So the kernel writes cells one at a time and batches the *notification*
downstream of them.

baud has the mirror of that in `RowExtent`, which bounds what a blank has to
touch, and it has no renderer to notify yet. When there is one, this is the
shape to copy: **the per-cell work and the per-frame work are batched
separately, and the boundary between them is a region rather than a flag.**

## The lesson runs the other way, and that is worth saying plainly

The usual shape of a survey note is "production got there first". This one is
the reverse: **the kernel console writes one cell per character through
`scr_writew()` and has no run path at all**, and baud writes eight cells to a
single store on the same input. Console throughput is not hypothetical either
-- a boot with a verbose log, a panic dump, a `dmesg` flood and a build running
on tty1 are all cases where `do_con_write()` is the thing on the CPU.

Two qualifications, because the kernel's constraints are real and this should
not read as a cheap shot:

* **It holds `console_lock` for the whole write** and has to remain correct in
  contexts baud never sees, including from a panic path. A bulk path is more
  code to be right about in a place where being wrong loses the machine's only
  output.
* **The per-character work it does is not the same work.** `vc_translate()`
  runs the UTF-8 state machine, the ACM translation table and the Unicode
  screen mirror; a run collector would have to keep all three consistent.

But neither of those argues against collecting a run of printable ASCII, which
is what the overwhelming majority of console output is, and which is the
cheapest of baud's paths to justify: the bytes are already contiguous, the
attributes are already fixed for the run, and the damage region is already
being accumulated. `[[source: linux-vt-console]]` shows the machinery for the
second and third of those is present and only the first is missing.

### And "just use the ISA" has one real obstacle in kernel context

Vector registers are not free to touch in the kernel. Using them requires
bracketing with `kernel_fpu_begin()`/`kernel_fpu_end()` on x86 or
`kernel_neon_begin()`/`kernel_neon_end()` on arm64, which save and restore
user FPU state and disable preemption for the duration. That is why crypto and
RAID drivers do it and why general-purpose kernel code does not, and a console
write that may run from awkward contexts is close to the worst place to add it.

**But a run scan does not need vector registers.** The kernel already scans
bytes in bulk without them: `memchr()` and the `strlen` family use SWAR --
64-bit word loads with the zero-and-high-bit trick -- precisely because it
works in any context with no bracketing and no feature detection. Finding the
next byte below `0x20` in a printable run is the same shape of test, and it
would turn the console's per-byte loop into a per-word one for the case that
dominates console output.

So the ISA argument survives the obstacle, and lands somewhere narrower than
"use SIMD": **the fast path a console needs is a word-at-a-time scan, which is
portable C, needs no privilege and no save/restore, and is already used
elsewhere in the same tree.** That is also the honest description of baud's own
`Portable` scanner -- `core::simd` lowers to whatever the target has, and the
NEON and AVX2 twins exist because the scan is the hot loop, not because the
idea requires them.

## What this closes

"Look at how embedded does it" is now answered with two primary sources rather
than an assumption, and the answer is that the throughput problem is baud's
alone. The remaining leads for `cat-cjk` are in this repo's own measurements,
not in prior art -- and every one of them has been priced (MEASUREMENTS.md).
