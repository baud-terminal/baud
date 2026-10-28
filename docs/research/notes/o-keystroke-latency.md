# O. Keystroke to pixel: the instrument, and what it read first

Row 0 of note N. Nothing in this repository measured typing before this:
the five-terminal comparison measures CPU seconds, the frame benches
measure a frame the app builds for itself, and the startup bench measures
a launch. What a person feels at the keyboard is the interval from the key
going down to the glyph being on the panel, and rows 6, 9 and 11 of the
checklist are each a claim about that interval. This note is the
instrument that makes those claims decidable -- built the way the app
already measures its own frames, checked from outside with a screen
capture, and run under the keep rule of note N: interleaved rounds, the
first discarded, an A/A copy in the sweep.

## How it has been measured before, and what each method defines

Every prior instrument is a black box: a key goes in at one end and a
pixel comes out at the other, and the four of them disagree about both
ends.

| instrument | the clock starts | the clock stops | resolution | statistic |
|---|---|---|---|---|
| Typometer, `[[source: fatin-typometer]]` `[[source: typometer-benchmark-impl]]` | after `Robot.keyPress` returns -- the post itself is excluded | one polled pixel differs from the background, `[[source: typometer-change-detector]]` | a busy poll, sub-millisecond on Windows and Linux; on macOS the pixel comes from `java.awt.Robot`, the compositor's copy, `[[source: typometer-screen-accessor]]` | min, max, **average**, SD over 200 keys at 150 ms; Fatin argues the average over the median and that the maximum matters, `[[source: fatin-typing-with-pleasure]]` § 3.3 |
| Dan Luu's terminal latency, `[[source: danluu-term-latency]]` | a synthetic keypress (Typometer, by Fatin's own comment) | an internal screen capture | as Typometer's | percentiles over **10,000** keys at 10.3 a second, idle and under load; macOS 10.12 on a 2014 MacBook Pro: terminal.app 6 ms idle median, iTerm2 44, alacritty 31 |
| a light sensor and a 1 kHz HID device, `[[source: hume-latency-tester]]` | the USB report leaves a Teensy -- no key travel, no keyboard | the sensor crosses a threshold at the **start** of the transition | 1 ms | mean and SD over 35-110 keys; **a fixed inter-key delay phase-locked to the refresh and read a false SD of 1 ms**, so the delay is jittered by 50 ms; Apple Terminal 35.8 ± 7.0, kitty 36.1 ± 5.6, alacritty 50.4 ± 3.9 on a 60 Hz panel, with **16 ms between the top and the bottom of the monitor** from scan-out |
| a 240 fps camera, `[[source: lkhrs-terminal-latency]]` `[[source: chadaustin-windows-terminal-latency]]` | the key seen moving, or the second frame after contact | the drawing complete, or the pixels starting to change | 4.17 ms | one or five samples a terminal |
| Dan Luu's end to end, `[[source: danluu-input-lag]]` `[[source: danluu-keyboard-latency]]` | the key starts to move | the screen has finished updating | a 240 or 1000 fps camera | a 2014 MacBook Pro at 100 ms whole; the Apple keyboard alone 15 ms from switch to USB |

Three things follow for baud, and each shaped a line of the design.

* **The output-side definitions differ by more than the numbers do.**
  Hume's threshold is the start of the transition; the camera methods
  read "complete"; Typometer reads a compositor's copy that Hume and
  ghostty's author both say can penalise a GPU application
  (`[[source: hn-mitchellh-latency-benchmarks]]`: it "cannot be measured
  properly without a camera, or pixel access to the screen but then have
  to be careful it's not impacting your benchmark"). So an instrument has
  to say which end it reads, and baud's reads two: the surface set on the
  layer, which is the last thing the app controls, and the window server's
  own display time of the changed frame from outside.
* **Alacritty's model of the worst case is three refresh intervals less
  the draw** (`[[source: alacritty-673]]`): input handled just after a
  vblank, the child's reply after the next, the draw waiting one more.
  Apple's render loop says the same from its side
  (`[[source: apple-understanding-hitches]]`,
  `[[source: apple-techtalk-render-loop]]`): event, commit, render
  prepare, render execute, display, with a double-buffered frame taking
  three vsync intervals from begin to last pixel and the display driver
  "up to one vsync interval to update all pixels". A per-leg instrument is
  what turns that model into a measurement of which interval baud pays.
* **No terminal ships one.** kitty's documentation cites Hume's hardware
  figures and Typometer and ships none (`[[source: kitty-performance]]`);
  foot's render timer is frame time, not input latency
  (`[[source: foot-wiki-performance]]`); wezterm's frame-latency knob was
  tuned with Typometer on Windows (`[[source: wezterm-pr-7470]]`). The
  per-leg decomposition below -- key event, pty write, first byte back,
  fed, commit, GPU end, surface set, target -- has no counterpart in any
  of them, and it is the only form under which rows 6, 9 and 11 can be
  told apart: each one moves a different leg.

## The route, and where a stamp is taken on it

A keystroke in the macOS app crosses these hands, and each leaves a
timestamp that is either already there or one call away:

| # | where | the stamp | who takes it |
|---|---|---|---|
| 1 | the key event | `NSEvent.timestamp`, through `NSApplication.currentEvent` | AppKit, before baud sees the event |
| 2 | the responder ran | `clock_gettime_nsec_np(CLOCK_UPTIME_RAW)` | `Responder::key` / `text` / `command` at entry |
| 3 | the pty write returned | the same clock | after `Pty::write` in `App::send` |
| 4 | the echo arrived | the same clock, on the reader thread, the instant `read(2)` returned | `pty::reader`, carried with the bytes as `Read::at` |
| 5 | the core was fed | the same clock | `Driver::pump`, after `Term::feed` |
| 6 | the frame was committed | the same clock | `App::present`, at `Drawn::Frame` |
| 7 | the GPU finished it | `MTLCommandBuffer.GPUEndTime` | the completion handler, carried in the `Landing` |
| 8 | the surface was set on the layer | the same clock, in `Responder::presented` | the view, immediately after the `CATransaction` that sets the layer's contents commits -- a hook the window crate gained for this, because the wake that follows can also be the pty reader's |
| 9 | the panel showed it | two bounds: `CADisplayLink.targetTimestamp` as announced by the tick before 8 (`vsync`), and the next tick's (`vsync_hi`) | the vsync the surface makes when the commit met the compositor's deadline, and the one after when it did not; the probe says which |

**One clock, and the SDK says so in each case.** `CADisplayLink`'s
timestamps are "Mach host time converted to seconds"
(`[[source: macos-sdk-cadisplaylink-h]]`); `CACurrentMediaTime` "is the
result of calling mach_absolute_time () and converting the units to
seconds" (`[[source: macos-sdk-cabase-h]]`); `GPUStartTime` and
`GPUEndTime` are "the host time in seconds that GPU starts [finishes]
executing this command buffer", and "relative to system mach time"
(`[[source: macos-sdk-mtlcommandbuffer-h]]`,
`[[source: apple-mtlcommandbuffer-gpuendtime]]`); a `CGEventTimestamp` is
"roughly, nanoseconds since startup" (`[[source: macos-sdk-cgeventtypes-h]]`);
and `CLOCK_UPTIME_RAW` "is identical to the result of mach_absolute_time()
after the appropriate mach_timebase conversion is applied"
(`[[source: macos-man-clock_gettime]]`). `NSEvent.timestamp` is the one
whose base Apple never names: the header only types it
(`[[source: macos-sdk-nsevent-h]]`) and the reference reads "seconds since
system startup" (`[[source: apple-nsevent-timestamp]]`). So it was measured
rather than assumed: a `CGEvent` posted with its timestamp set to a
`CLOCK_UPTIME_RAW` reading arrives in the app with `NSEvent.timestamp`
equal to that reading **exactly**, in every one of forty trials, while an
event posted with the timestamp left at zero is re-stamped with a real
host time -- so the field is live and the preservation is real. And the
five clocks read back to back -- `CLOCK_UPTIME_RAW`, `mach_absolute_time`
through the timebase, `CACurrentMediaTime`, `systemUptime` -- agree within
the hundreds of nanoseconds it takes to read them in sequence.

`HostTime` in `baud-domain/src/time.rs` is that count as a type:
nanoseconds of mach host time, with `since` returning a `Duration` and
refusing a negative one, because a leg that comes out negative is a stamp
matched to the wrong key and not a duration. `std::time::Instant` is not
used for any stamp. It reads `CLOCK_UPTIME_RAW` on macOS today
(`[[source: rust-std-sys-time-unix]]`, a private constant), and "today" is
not a base the instrument should rest on when the frameworks' own
convention is documented and an `Instant` cannot be built from an
`NSEvent.timestamp` at all.

## Pairing: which echo, which frame

A record opens when a key is sent and closes when the frame it landed in
has both vsync bounds. In between, every stamp is applied to *every open
record that lacks it*: the first read stamped after a key was handled is
the echo of every written key before it, the first commit after a feed is
the frame of every key fed before it, and a present stamps every committed
record whose commit preceded that frame's GPU end. With one frame in flight
(research decision D14) and a cadence slower than a frame, that is one key,
one echo, one frame, and every stamp lands on exactly one record. Under
typeahead several keys share an echo and a frame and each still gets the
stamps; the driver counts the keys that shared a frame so a reader knows
what the median is a median of. A key with no echo -- a child that never
answers, or a key the core encodes to nothing, which is marked unsent so
the next key's write cannot claim it -- expires after two seconds and is
emitted with the legs it has and the rest marked `-`, so the driver can
print "posted 30, completed 0" rather than nothing. That last case is a
positive control no prior instrument has, and it exists because a probe
that cannot fail is the failure this repository has recorded a dozen times.

## The driver, and the two children

`scripts/key-latency.py` launches the binary under `BAUD_KEYLAT=1`, waits
for its first present, and hands its pid to `scripts/key-post.swift`,
which posts plain letters through the HID event tap the way a keyboard
does (`[[source: apple-cgevent-post]]`, `[[source: apple-cghideventtap]]`)
-- refusing, before every key, unless the frontmost ordinary window
belongs to that pid, and never a control key, a modifier or Return. Each
post is stamped by the poster on the same clock
(`[[source: apple-cgeventsettimestamp]]`), the two streams are paired by
order, and a pair whose event precedes its post or trails it by more than
100 ms is excluded and counted. Rounds are interleaved across arms with
the first discarded and an A/A copy in the sweep, which is the keep rule
of note N applied to typing. The inter-key gap is 150 ms in the mean,
Fatin's, and jittered by a quarter either way from a seed the run prints,
because Hume found a fixed gap phase-locks to the refresh and reads a false
spread; the A/A floor is reported beside every leg rather than trusted as
small for the same reason.

Two children, two questions. `cat` keeps the child out of the loop: the
tty's line discipline echoes the byte inside the kernel, so the `child`
leg is the pty round trip and everything else is baud's. The login shell
is what a user types at, and its `child` leg is the shell's turn -- zle
reading the byte and redrawing the line -- which is not baud's to shorten,
and which row 9's local echo would take out of the felt interval. Dan Luu
found shells and multiplexers within noise of each other, which is why
one shell is enough here.

## The pixel, from outside

`scripts/key-pixel.swift` posts one key the same way and watches the
screen through ScreenCaptureKit: a stream over a small rectangle at the
cursor, whose frames carry the window server's own display time of each
changed frame (`[[source: apple-scstreamframeinfo-displaytime]]`,
`[[source: macos-sdk-scstream-h]]`: "the mach absolute time when the event
occurred. For a frame event, this is when the frame was displayed by the
window server") and a status that separates a changed frame from an idle
one. The difference from the post is the keystroke-to-pixel time for that
key, measured by the compositor rather than by baud, on the same clock
after the timebase conversion (the stamp is in mach ticks; on this box the
timebase is 125/3). Its job is to validate the in-app estimate, not to
replace it: the estimate exists for every key of a sweep, the probe for
the keys it is pointed at. The two deprecated capture calls,
`CGDisplayCreateImage` and `CGWindowListCreateImage`, are removed from the
current SDK and were not used.

What neither reads is the panel: the estimate and the probe both end at
the presentation, and Hume's 16 ms top-to-bottom scan-out and the pixel's
own response time sit after it, as does the keyboard's own 15-60 ms before
the event (`[[source: danluu-keyboard-latency]]`). The number below is the
software's share, from the event the OS delivers to the frame the
compositor presents.

## What it read

Apple M5 Max, macOS 27.0, on mains, the built-in 120 Hz panel, the app's
default cell path at 120x40 under the measuring machine's config. Five rounds of thirty
keys at a 150 ms gap, the first round discarded, an A/A copy of the same
binary interleaved with the arm order rotated: 120 keys an arm. Every
posted key paired with its line, no exclusions but two on the `cat` A/A arm
(an echo the reader stamped 1-4 us before the writer's post-write stamp,
which the driver refuses as a negative leg rather than pooling), no
landing refused, `stamp = 0` on every one of the 480 keys. Medians in
microseconds; the floor is the A/A copy's, per leg (the larger of the
median delta and either arm's interquartile range).

| leg | `cat` -- the kernel's echo | the login shell | floor, cat / shell |
|---|---|---|---|
| deliver: the post to the responder running | 1652 | 1664 | 613 / 584 |
| write: `Term::key` and the pty write | 38 | 37 | 13 / 12 |
| **child: the write to the reader's `read(2)`** | **14.5** | **2551** | 4.7 / 628 |
| wake: the read to the core fed | 46 | 29 | 22 / 14 |
| build: fed to the frame committed | 91 | 188 | 44 / 44 |
| gpu: committed to `GPUEndTime` | 1004 | 615 | 451 / 399 |
| show: the GPU's end to the surface on the layer | 218 | 229 | 46 / 51 |
| **to_shown: the post to the surface on the layer** | **3041** | **5422** | 731 / 1163 |
| vsync: the surface to the announced target | 2827 | 4499 | 2896 / 5172 |
| total, early bound: the post to the announced target | 7097 | 11286 | 4182 / 4915 |
| total, late bound: the post to the target after it | 15431 | 19620 | 4182 / 4915 |

**What separates the two children is the child, and only the child.**
`child` is 2.5 ms through zsh against 15 us through the line discipline --
zle reading the byte and redrawing its line -- with `build` +97 us and
`to_shown` +2.4 ms following from it; every other leg overlaps its floor.
That is what a user feels typing at a prompt that baud does not own, and it
is what row 9's local echo would take out of the interval.

**The app's whole share is three milliseconds, and most of it is the
platform's.** `to_shown` -- the post to the surface being on the layer, the
last thing the process controls, with no vsync quantisation in it -- is
3.0 ms on the `cat` floor: 1.65 ms of that is the event's delivery from
the HID tap to the responder, 1.0 ms is the GPU's round trip for a frame
whose own work is ~100 us, 0.2 ms the completion reaching the main thread,
and the parse, the write, the wake and the build together are under 200 us.
An isolated key's frame is built 91 us after its echo is fed, on the wake
itself; it does not wait for a tick.

**The two bounds bracket the panel, and the probe says which side the
median key lands on.** Twenty keys watched from outside through
ScreenCaptureKit, the cell to the right of the cursor compared byte for
byte against a stable baseline, the window server's own display time of
the first frame that differed:

| | |
|---|---|
| runs that saw the change | 20 of 20; the control rect, off the cursor, saw none |
| display time relative to the display-link target before it | **3.5-6.0 ms after, median 4.1**, every run |
| inside `[vsync, vsync_hi)` | 16 of 20 |
| one period before the early bound | 4 of 20 -- the runs where the surface was set within 1.1 ms of its tick |
| after the late bound, or before `shown` | 0 |

So the compositor's stamp is its own pass, ~4 ms after the previous
target, and a surface makes that pass only if it is on the layer within
about a millisecond of the tick that announced the target; set later, it
is composited a period on and shown at the late bound. On the unobserved
sweep the median key's surface sat 5.5 ms after its tick, past that
window, so **the late bound is the panel time for the median key: about
15 ms from the post for `cat` and 20 ms through the shell**, of which the
app's own path is 3.0 and 5.4. The probe's own numbers are not the
sweep's: capturing the window at 120 Hz puts `to_shown` at 13 ms in the
same runs, four times the unobserved figure, which is the caveat every
prior instrument carries and the reason the in-app stamps are the
baseline and the probe is the check.

**Typeahead is reachable and the app coalesces it.** At a 2 ms cadence
(the helper's 1 ms key-up and no gap) 76 of 80 keys shared a frame: one
commit per 8.33 ms period, three keys a frame, the extra wait landing in
`build` at 3.7 ms. Every row's consecutive legs sum to its total to the
nanosecond, over the 718 rows of the three sweeps.

**The no-echo control had to be `stty -echo`.** The spec's control was a
child that never answers, and `sleep` answered every key: the line
discipline echoes a typed byte inside the master's `write(2)` whatever the
child does, so `cat` and `sleep` measure the same thing. `--child none`
turns echo off on the slave, and every key then halts at `written` with
the rest `-`, five posted, none completed, the driver exiting non-zero.

**What the instrument says about the rows that were waiting for it.**
Row 6 -- pace a keystroke's frame by completion rather than by the display
link -- rests on "a key's frame waits up to one tick today"; the `build`
leg says an isolated key's frame is committed 91 us after its echo is fed,
so the tick is not what a single key waits for, and what it does wait for
is the compositor's period. Row 11 -- skip the present when nothing
changed -- is neutral here. Row 9 -- draw the echo before the pty round
trip -- cannot be measured with this pairing: a record is committed only
after it is fed, so a frame drawn ahead of the echo has nothing to land in,
and the row needs a record that can be committed on the key alone.

**Two things the reviews refuted that had to be rebuilt first.** The
first version assigned `vsync` the target of the first tick after `shown`,
which is the vsync *after* the one a surface that met the deadline lands
on -- one period late by construction; and it stamped `shown` at the wake,
which the pty reader can also send ahead of the present. `vsync` is the
announced target now, `vsync_hi` the one after, and `shown` is taken where
the view commits the `CATransaction`, through a `presented` hook the
window crate gained. Two more fell in the second round: a key the core
encodes to nothing was stamped `written` by the next key's write and took
its echo (an explicit unsent state now,
`a_key_the_core_encodes_to_nothing_is_not_written_by_the_next_key`), and
two GPU completions before one present handed the first frame the second's
end stamp (the presenter queues each admitted frame's end and a present
takes its own, `a_present_takes_its_own_frame_s_end_in_landing_order`). A
dropped display-link tick voids a record's bounds rather than assigning
the next target (`a_dropped_tick_voids_the_vsync_bounds`), and the
fixture the driver's self-test parses is rebuilt from its own numbers by
the emitter (`the_fixture_s_first_record_is_what_the_line_emits`), so the
two halves cannot drift apart while both suites stay green.

## Row 6 read through it, and the helper's posts were phase-locked under load

Row 6 -- pace a keystroke's frame by completion rather than by the display
link -- is measured, and the workload that shows it is `--child stream`: a
child printing a line every 2 ms under the tty's echo, so output is
flowing when the key lands and the frame loop's "draw now on a wake only
if a frame's time has passed" gate is never open. There the base waited
2.7-3.2 ms for the tick (`build`, fed to committed, against 65 us through
`cat`), and drawing the echo on the read that brings it puts `to_shown`
2.5-2.8 ms lower in each of four sweeps, past the floor in the two quiet
ones; the figures and the mechanism are in `MEASUREMENTS.md`, "Keystroke to pixel".

**What the sweeps found about this instrument.** The first two had every
arm's `vsync` leg tight -- 7.4-7.6 ms on the tick-paced base, 1.9-2.0 ms
on the change -- which means the posts sat at one phase of the refresh:
the raw rows put 63 % of them in two octiles of the period, where the same
helper under `cat` fills all eight. The jitter was drawn and applied; what
undid it was the order of the helper's loop. The front-window guard and
the event's creation are round trips to the window server, and under a
compositing load the server answers in phase with its passes, so a wait
placed before them -- a plain sleep, or a spin to the deadline, which
changed nothing -- landed every post at the server's rhythm. The wait is
the last instruction before the stamp now, and the posts fill every octile.
The lesson is the driver's own warning about a fixed gap, arriving through
a side door: a schedule is only as good as the last thing that runs before
the stamp, and on a busy window server that thing was the server.

**What the row leaves the panel.** The probe's rule from above -- a
surface makes the pass of the tick it follows only when it is on the layer
within about a millisecond of that tick -- says the base's tick-paced
frame (on the layer 0.6-0.8 ms after its tick) and the immediate frame
(on the layer about a millisecond after the echo, mid-period) make the
same pass at every phase but the first 0.1 ms after a tick; the difference
is the base's tail, 8-12 % of keys whose frame reached the layer past that
millisecond. Adjudicating that needs the probe under a stream that writes
away from the cursor, which is not built.
