# AH. Lights on the title bar: what each LED may honestly light for

**Status: research draft, 2026-10-09; revised the same day after a read-only
review. The RD and SD pair was built on 2026-10-24 (section 10), and seven
more modem lights on 2026-11-03 (section 11); ARQ, the network pair, DK, CPU
and MEM are unbuilt.** Every baud file:line citation in this note is pinned at
commit `473ff12`, except in section 11.

- Section 1 holds the two requests.
- Sections 2 to 5 cover the sources each LED would rest on, what one process
  can and cannot read about another on macOS and Linux, and what baud already
  knows.
- Section 6 sets out the options with their costs, and section 8 the open questions.

The evidence was kept outside the repository until the files below were
committed:

- `sources/` holds the downloads, hashed in `manifest-proposed.toml`.
- The five probes' sources, their saved outputs and a file of command
  listings are staged under `to-commit/ah-observed/`. They are keyed as
  `ah-obs-*` entries, `retrieved`, to be committed under
  `docs/research/sources/ah-observed/`, after note AD's precedent.
  - Process ids are replaced by role names and the power source's `id=`
    field is removed.
  - No output carries the ssh session's remote host or ports.
  - `probes/` keeps the built binaries for the checks (8.1).

Evidence tags:

- **M**: measured by one of the probes, on this Mac (Apple M5 Max, macOS
  27.0 build 26A5425a, on AC power, in a sandboxed shell). Each M
  cites its saved output by `ah-obs-*` key, and each output records the
  sha256 of the probe source it ran (also an `ah-obs-*` key; the ssh output
  records a 12-character prefix of it).
- **C**: computed from M figures.
- **D**: derived from source code, not measured.
- **S**: quoted from a source.
- **UNVERIFIED**: only a secondary source was found, or nobody checked.

Man pages are cited by line of their roff source (`roff N-M`). Note AK cites
the same files by rendered line.

## 0. In plain words

- **An honest LED lights only for an event baud measured.**
  - Every LED below is backed by a counter or a state with a named source:
    either a byte baud moved or a figure the kernel reports about the program
    in front.
  - A process baud may not read, such as `sudo vim`, must not be shown as
    idle. Whether it gets a look of its own or its LED is hidden is Q8.
- **RD and SD, the pty pair, need no extra read or write.**
  - baud already moves these bytes itself.
  - On Linux, each lit LED adds one poll deadline, to turn it off.
  - The redraw cost is unmeasured.
  - Today nothing counts them (5.3).
- **The Linux tty LED trigger cannot be the model for that pair.**
  - It reads a serial driver's interrupt counters, and a pty driver has none,
    so on a pty it stays dark (2.1).
  - Its timing is one of the references in 6.2. It polls every 100 ms and
    blinks 50 ms on. Under steady traffic, a poll that lands while the last
    blink is still pending is dropped. So it shows about one 50 ms flash per
    200 ms, roughly 5 Hz (**D**, unmeasured).
- **Disk, CPU and memory can be read for the program in front, on both
  systems.**
  - On macOS it is one `proc_pid_rusage` call per process, 464-473 ns (**M**, [[source: ah-obs-leds-probe-out]]).
  - Apple's own header calls these interfaces private: `libproc.h` says "This
    header file contains private interfaces to obtain process information.
    These interfaces are subject to change in future releases." They are
    declared in the SDK all the same (3.2).
  - The reading has to cover every live member of the foreground process
    group, not the shell.
  - On macOS a child's disk bytes never reach its parent's counters (**M**, [[source: ah-obs-leds-probe-out]]).
    Its CPU and page-ins reach them only after the parent reaps it (**M**, [[source: ah-obs-child-probe-out]]).
  - A member started and reaped between two samples is never seen directly
    (3.4).
- **Network bytes for one connection.**
  - **macOS.** The SDK declares a record that carries TCP sequence numbers:
    `struct xtcpcb64` in `netinet/tcp_var.h`, served by the sysctl
    `net.inet.tcp.pcblist64` (3.7).
    - On the probe's own loopback pair, the sequence deltas counted exactly
      the 1,048,576 bytes sent and received (**M**, [[source: ah-obs-pcb64-probe-out]]).
    - From inside the sandbox, the dump held only the probe's own sockets, so
      a live ssh socket in another terminal was invisible (**M**, [[source: ah-obs-pcb64-probe-out]]).
    - Whether baud, outside that sandbox, can see another process's sockets
      is **UNVERIFIED**. Two unsandboxed runs made earlier in this research
      showed the same thing and were not saved (3.7). It is on the list of checks (8.1).
    - Mechanism and cost: one `pcblist64` size query and fetch, 4.5 µs on the
      probe's three-record dump (**M**, [[source: ah-obs-pcb64-probe-out]]);
      a full dump, which visibility would return, is unmeasured.
    - What the SDK also declares for another process's socket is its *state*,
      through `PROC_PIDFDSOCKETINFO`. An established TCP socket is an honest
      carrier-detect.
  - **Linux** has per-socket TCP byte counts through `sock_diag`.
  - Over ssh, the pty pair carries the session's terminal data. A network pair
    would also count traffic that does not pass through baud's pty.
    Examples: port forwards, agent and X11 forwarding, scp and sftp channels,
    keepalives, and other sessions sharing a ControlMaster connection.
    That list is **UNVERIFIED**: no ssh source is in the corpus.
- **"What protocols are used" can be read from the core's own state.**
  - That state covers the keyboard protocol, mouse reporting, bracketed paste,
    synchronized output, the alternate screen, images, 8-bit controls and the
    input encoding.
  - Most of these have an accessor (5.4). Two do not: `modifyOtherKeys` and
    the mouse encoding.
  - "Protocols" could instead mean the session's transport, named from the
    process tree: ssh, mosh, telnet or serial. Which one is meant is Q1.
- **The modem vocabulary mostly maps.**
  - The Hayes Smartmodem 300 had seven lights: AA, CD, OH, RD, SD, TR and MR.
  - HS came with the 1200. The Couriers added RS, CS, ARQ and SYN, and from
    the V.34 on, FAX shares the ARQ light (2.3).
  - Most of these labels map onto something baud can measure (6.1).
  - The Hayes 300's MR was lit "at all times when the Smartmodem is powered
    up". In baud an MR would measure nothing.
- **The title bar has room on the right.**
  - AppKit places a trailing titlebar accessory flush right, at the strip's
    full height of 28 pt.
  - That was measured in a window shaped like glassnote's, built by two probes
    linked against SDK 14.4, the same SDK baud links (**M**, [[source: ah-obs-titlebar-probe-sdk14-out]]).
  - The window crate has had an accessory API since 1.0.0-beta.8, and the
    lights are a trailing accessory (5.1).

## 1. What was asked (2026-10-08)

- Old-style modem lights, rx/tx at the right of the title bar, perhaps also showing which protocols are in use, with a setting to turn them off.
- After Linux's TTY LED triggers (`CONFIG_LEDS_TRIGGER_TTY`): the network's tx/rx when the session is over ssh, disk IO, CPU and memory.

## 2. The sources

### 2.1 The Linux tty LED trigger, at source

[[source: linux-ledtrig-tty]] is `drivers/leds/trigger/ledtrig-tty.c` at
Linux `cf72cbb`, the commit the manifest already pins for `linux-n-tty`.

**Its Kconfig help** ([[source: linux-leds-trigger-kconfig]] 139-146):

> "This allows LEDs to be controlled by activity on ttys which includes serial
> devices like /dev/ttyS0."

**Its ABI** ([[source: linux-abi-led-trigger-tty]]):

| Line | Text (**S**) |
|---|---|
| 1-6 | `ttyname`, since 5.10 (Dec 2020): "Specifies the tty device name of the triggering tty" |
| 8-14 | `rx`, since 6.8 (Feb 2024): "Signal reception (rx) of data on the named tty device. ... If set to 1 (default), the LED will blink on reception." |
| 16-22 | `tx`: "Signal transmission (tx) of data ... If set to 1 (default), the LED will blink on transmission." |
| 24-32 | `cts`: "CTS = Clear To Send / DCE is ready to accept data from the DTE. If the line state is detected, the LED is switched on." Default 0. |
| 34-42 | `dsr`: "DSR = Data Set Ready / DCE is ready to receive and send data." Default 0. |
| 44-52 | `dcd`: "DCD = Data Carrier Detect / DTE is receiving a carrier from the DCE." Default 0. |
| 54-62 | `rng`: "RNG = Ring Indicator / DCE has detected an incoming ring signal on the telephone line." Default 0. |

**How it detects activity: a poll every 100 ms.**

- `#define LEDTRIG_TTY_INTERVAL 50` (line 11).
- The work function ends with
  `schedule_delayed_work(&trigger_data->dwork, msecs_to_jiffies(LEDTRIG_TTY_INTERVAL * 2));`
  (295-296).
- Each run calls `tty_get_tiocm` for the four line states (230-251), then
  `tty_get_icount` (260).
- It compares `icount.tx` and `icount.rx` with the last values. It stores the
  new count first and then asks for a blink (264-274).
- The counts are the driver's, held in `struct serial_icounter_struct`:
  `int cts, dsr, rng, dcd; int rx, tx; ...`
  ([[source: linux-uapi-serial-h]] 104-110).

Line states hold the LED on, and data blinks it. Data wins (253-256): "The
evaluation of rx/tx must be done after the evaluation of TIOCM_*, because
rx/tx has priority."

**The blink, and what happens under steady traffic.**

1. The trigger calls
   `led_blink_set_oneshot(trigger_data->led_cdev, &interval, &interval, invert)`
   with `interval = LEDTRIG_TTY_INTERVAL` (200, 279-281). That asks for one
   blink of 50 ms on and 50 ms off.
2. The software blink arms its timer at `jiffies + 1`
   ([[source: linux-led-core]] 217).
3. The timer turns the LED on for `blink_delay_on` (92, 120) and then off for
   `blink_delay_off` (99, 120). On the off step it marks the one-shot as ending
   (104-118). One more expiry, after the off period, ends it (78-82). So the
   timer stays pending until about 100 ms plus a jiffy after the poll that
   asked.
4. The next poll comes about 100 ms after the last (295-296). It typically
   finds the timer still pending, so `led_blink_set_oneshot` returns at once:
   `if (test_bit(LED_BLINK_ONESHOT, ...) && timer_pending(&led_cdev->blink_timer)) return;`
   (263-265). The count was already stored (264-274), so that poll's activity
   is never shown.
5. The poll after that finds the timer idle and blinks.

The result under steady traffic is one 50 ms flash per 200 ms: about 5 Hz,
50 ms on and 150 ms off (**D**, derived from the code and unmeasured;
scheduling jitter can change which poll lands).

The control is the network trigger. It calls `led_stop_software_blink` before
each blink ([[source: linux-ledtrig-netdev]] 706-707), so its blink restarts
at every poll. The tty trigger does not do this.

**The trigger cannot light for a pty.**

1. `tty_get_icount` returns `-ENOTTY` unless the driver has a `get_icount`
   op ([[source: linux-tty-io]] 2565-2568). `tty_get_tiocm` is the same for
   `tiocmget` (2480-2485).
2. None of the four pty `tty_operations` tables in [[source: linux-pty]] has
   either op: `master_pty_ops_bsd` (494-507), `slave_pty_ops_bsd` (509-523),
   `ptm_unix98_ops` (722-737) and `pty_unix98_ops` (739-753).
3. The positive control is the serial core's `uart_ops`
   ([[source: linux-serial-core]] 2702). It has
   `.tiocmget = uart_tiocmget` (2726) and `.get_icount = uart_get_icount`
   (2730).
4. On a pty, then, the work jumps to `out` (260-262) with the state still
   `TTY_LED_DISABLE` (199), and the LED is set to `LED_OFF` (287-290).

### 2.2 The kernel's other triggers: network, disk, CPU

All three are at the same commit.

**Network** ([[source: linux-ledtrig-netdev]]):

- `#define NETDEV_LED_DEFAULT_INTERVAL 50` (32), described as "interval -
  duration of LED blink, in milliseconds" (38).
- It sums the *interface's* `tx_packets` and `rx_packets` from
  `dev_get_stats` (695-704).
- It blinks once whenever the sum changes, restarting the blink each time
  (706-729), and reschedules itself at `interval*2` (732-733).
- The interval is bounded to 5-10000 ms (504-505).
- It watches a whole network device, not a process.

**Disk** ([[source: linux-ledtrig-disk]]):

- `#define BLINK_DELAY 30` (14).
- `ledtrig_disk_activity(bool write)` blinks `disk-activity`, and also
  `disk-write` or `disk-read`, with a 30 ms one-shot (20-29). It is driven by
  calls to that function, not by a poll.
- `LEDS_TRIGGER_DISK` "depends on ATA" ([[source: linux-leds-trigger-kconfig]]
  35-37).
- Who calls it, and how often, is **UNVERIFIED**: no caller is in the corpus.

**CPU** ([[source: linux-ledtrig-activity]]). It maps load to blink rate and
duty, with no threshold (98-109):

> "when the CPU usage is zero, the LED must blink slowly with very faint
> flashes that are detectable but not disturbing (typically 10ms every
> second, or 10ms ON, 990ms OFF). Then we want blinking frequency to
> increase up to the point where the load is enough to saturate one core in
> multi-core systems or 50% in single core systems. At this point it should
> reach 10 Hz with a 10/90 duty cycle (10ms ON, 90ms OFF). After this point,
> the blinking frequency remains stable (10 Hz) and only the duty cycle
> increases to report the activity, up to the point where we have 90ms ON,
> 10ms OFF when all cores are saturated. It's important that the LED never
> stays in a steady state so that it's easy to distinguish an idle or
> saturated machine from a hung one."

Its timer re-arms at most 100 ms ahead: "we cap the sleep time to 100 ms"
(121-123), `delay = min_t(int, activity_data->time_left, 100)` (150). So its
samples are never more than 100 ms apart, and they can be much closer: about
10 ms apart at the shortest duty steps (**D**).

### 2.3 The modem front panel

The request's vocabulary is the modem's, so each label is taken from the manufacturers' manuals.

- **Scanned manuals.** A helper read each one from its page images and
  checked every quote against the scan's text layer. A dozen of those checks
  were re-run here by normalised search. No per-quote record of the
  transcription was kept.
- **The V.34 manual** is a plain-text file, quoted directly.
- **The 1200's firmware lines** are source code, quoted from the file.
- Files, URLs and hashes are in `sources/modems/SOURCES.tsv`.

**Hayes Smartmodem 300: seven lights, and no HS.**
[[source: hayes-smartmodem-owners-1981]] (PDF p.18-19, printed 2-7 and 2-8):
"The front panel of the Smartmodem contains seven indicator lights that
visually identify the status of the Smartmodem". Read right to left they are
MR, TR, SD, RD, OH, CD and AA.

| LED | The 1981 owner's manual (**S**) |
|---|---|
| MR | "MR (Modem Ready) indicator light is illuminated at all times when the Smartmodem is powered up." |
| TR | "TR (Terminal Ready) indicator lights to show a signal has been given to the Smartmodem that the data terminal is ready to receive or send data; this signal enables the Smartmodem to execute commands." |
| SD | "SD (Send Data) indicator flashes when data is sent from the RS-232C port to the Smartmodem (i.e., when commands are sent to the Smartmodem or data is transmitted to a distant computer)." |
| RD | "RD (Receive Data) light flashes when data received from a distant computer or terminal is sent from the Smartmodem to the RS-232C port. ... It will also flash when the Smartmodem sends result codes." |
| OH | "The OH indicator light is illuminated when the Smartmodem is using the telephone line; otherwise this light is off." |
| CD | "When the Smartmodem recognizes a carrier signal, the CD indicator will light. When no detectable carrier is present, or if the distant modem hangs up, this light will be off." |
| AA | "if the Smartmodem is set ... to automatically answer, the AA light will be illuminated. Then, when the phone rings, the light will go off during each ring" |

[[source: hayes-smartmodem-300-1986]] (PDF p.26-27, printed 2-4 and 2-5)
says the same more briefly. Two lines from it:

- "MR (Modem Ready)–Lit when you turn on Smartmodem 300."
- On TR: "When configuration switch 1 is DOWN (DTR always supplied), this
  light remains lit as long as Smartmodem 300 is powered up."

**Hayes Smartmodem 1200: eight lights, with HS.** No owner's manual was
found, so the manual's wording for all eight is **UNVERIFIED**. The labels
come from two other Hayes sources:

- **The schematic.** [[source: hayes-smartmodem-1200-schematic-1984]] shows
  the LED bank CR2-CR9 on PDF p.2: OH, AA, RD, CD, HS, TR, SD and MR.
- **The firmware.** [[source: hayes-smartmodem-1200-firmware-1982]],
  `modem123.asm`, defines `HS$LED: EQU 10H ;;0=HS LED ON` (184). It sets HS
  in two places:
  - **In command state, from the computer-side rate.** HS is set from
    `OBAUD`, the output baud rate (514-521: `;;ASSUME LOW SPEED`,
    `;;TEST FOR 1200 BAUD`, `;;TURN ON HIGH SPEED LED`). `OBAUD0/1` are the
    "OUTPUT BAUDRATE SELECT" bits (141-142), copied from the input baud the
    modem detected (1910-1915: `;;TRANSFER INPUT BAUD TO OUTPUT BAUD`).
  - **At connect, from the line speed.** It is on for a 1200 connect
    (1113-1114) and off for a low-speed one (1093-1094).

[[source: hayes-44-012-1991]] (PDF p.96, printed B-3, §B.2.1) treats a dark
HS as misconfiguration: "if the MR (modem ready) light does not come on, the
modem may have a problem. Or if the HS light (external Smartmodem and
V-series System Products) does not come on, the modem may be improperly
configured." The Couriers hold HS after the call: "Remains ON after
disconnect until next call is originated or answered"
([[source: usr-courier-hs-1988]]). The V.Everything guide says "Once this
light is on, it remains on until reset" ([[source: usr-courier-veverything-1997]]
PDF p.57). So HS shows the rate the modem last ran at. It is a held indicator,
not an activity light.

A catalog page says "seven in the 300 baud model, eight in the 1200". It is
secondary and is not relied on.

The original Smartmodem 2400's set is **UNVERIFIED**. The only 2400 guide
found is for the later V-series, and it says only "Eight LED indicators on
front panel" ([[source: hayes-vseries-2400-1987]] PDF p.112, printed A-20).

**U.S. Robotics Courier: nine lights, then twelve.**

- The Courier 2400, [[source: usr-courier-2400-1986]] (PDF p.67), has nine:
  HS, AA, CD, OH, RD, SD, TR, MR and AL (Analog Loopback).
- The 9600 HST, [[source: usr-courier-hst-9600-1988]] (PDF p.77), has
  twelve: it adds RS, CS and ARQ and keeps AL.
- From the 1988 High Speed manual on, SYN replaces AL, and the count stays at
  twelve. The 1990 V.32bis edition, [[source: usr-courier-hs-1990]] (PDF
  p.26), lists "the modem's twelve status lights".
- The V.34 manual, [[source: usr-courier-v34-1994]] (lines 3512-3586), makes
  ARQ into ARQ/FAX: "Fax Mode: Flashes steadily to indicate fax mode". It adds
  to AA: "Flashes ON for incoming ring detect."

Selected rows from the 1988 High Speed manual,
[[source: usr-courier-hs-1988]] (PDF p.96-97, printed B-2.1 and B-2.2):

| LED | Courier 1.015.282 (**S**) |
|---|---|
| HS | "All calls above 2400 bps: ON during call progress, after completion of dialing; ... ON during connection. Remains ON after disconnect until next call is originated or answered." |
| MR | "ON when the Courier is powered on. Flashes when the modems retrain, including online fallback, or while the modem is in Test mode." |
| RS | "ON when the Courier is powered on if your computer supports RTS. OFF if the Courier is set to &R2 (Received Data hardware flow control) and the computer lowers RTS." |
| CS | "ON until the modem lowers CTS when Transmit Data hardware flow control is enabled (&H1, &H3)." |
| ARQ | "ON when the Courier is set to &M4 or &M5 and successfully connects with another modem under error control. Flashes when the Courier retransmits data to the remote modem." |
| SYN | "ON when the modem is set to &M1 and enters synchronous mode." |

The V.Everything guide, [[source: usr-courier-veverything-1997]] (PDF
p.57-58), gives each light's on, off and flashing meanings. For example:

- RD, Flashing: "Is sending data to your computer".
- SD, Flashing: "Is receiving data from your computer".
- MR, Flashing: "Is retraining with a remote device or is in Test mode".

**U.S. Robotics Sportster: seven rows.** The three Sportster manuals found
list AA, CD, RD, SD, TR, CS and ARQ/FAX, with no HS, OH or MR rows:

- [[source: usr-sportster-1993]], PDF p.79, Table B.1;
- [[source: usr-sportster-56k-1997]], PDF p.143;
- [[source: usr-sportster-mac-56k-1997]], PDF p.35.

That is what the manuals list. It is not a claim about every Sportster
panel.

**The viewpoint for RD and SD is the computer's.**

- SD flashes for what the computer sends to the modem.
- RD flashes for what the modem passes to the computer, including the modem's
  own result codes. Hayes 1981 says so ("It will also flash when the
  Smartmodem sends result codes"), and so do the Courier 2400 and 9600 HST:
  "or when the modem is sending result codes to the terminal or computer"
  ([[source: usr-courier-2400-1986]] PDF p.67).

baud is the computer at the keyboard, so SD is baud writing to the master and
RD is baud reading from it.

**Which lights carry information, by the manuals.**

- **MR.** On the Hayes 300 it is lit "at all times when the Smartmodem is
  powered up", so it carries none. On the Couriers it "Flashes when the modems
  retrain ... or while the modem is in Test mode" (1988 HS; the 9600 HST p.77,
  the V.34 manual and V.Everything p.58 say the same). That flash carries
  information, but baud has no retrain to match it.
- **TR.** It shows DTR. It stays on regardless only under the DTR override
  (Hayes 1986: "switch 1 is DOWN (DTR always supplied)"; Courier: "ON when the
  DTR override (DIP switch 1) is ON"). The nearest baud analogue, the master
  held open, is true for as long as baud runs, so a baud TR would always be
  on.
- **RS.** It goes off under `&R2` when the computer lowers RTS. That is
  receive flow control, the mirror of CS. Under `&R2`, RS dark means "the
  computer stopped accepting data".
  - Taken inverted, as CS is in 6.1, baud's analogue would light when baud
    stopped reading the master.
  - That never happens today, on either system, so an inverted RS would
    never light:
    - **macOS.** The reader sends into an unbounded `channel()`
      (`pty.rs:789`), and returns only when the stream closes, a wait fails or
      the receiver is gone (`pty.rs:900-935`).
    - **Linux.** The poll always asks the master for `POLLIN`
      (`pty.rs:988-995`), and the driver reads whenever it is ready
      (`linux.rs:279-283`).
  - If baud ever reads under backpressure (note AG section 1 weighs it for
    replies), RS becomes an honest candidate. It is listed in 6.1 either
    way.
- **What a dark light meant.** The manuals tie a dark light to a fault only
  for MR, and for HS as misconfiguration, both at power-up (44-012 §B.2.1,
  above). A dark TR or RS meant only that the line was not asserted.

### 2.4 Flashing

[[source: wcag22]] is the closest normative text, and it is written for web
pages. WCAG2ICT, a W3C Group Note on applying WCAG 2 to non-web software
([[source: wcag2ict-22]], section "Applying SC 2.3.1 Three Flashes or Below
Threshold to Non-Web Documents and Software"), says of 2.3.1: "This applies
directly as written ... replacing “web pages” with “non-web documents or
software”". It "provides informative guidance on applying WCAG 2.0, 2.1, and
2.2 Level A and AA success criteria", so 2.3.2, which is Level AAA, applies to
baud only by analogy.

- **SC 2.3.1** (Level A): "Web pages do not contain anything that flashes more
  than three times in any one second period, or the flash is below the general
  flash and red flash thresholds."
- **Under the thresholds' definition**, content passes if "the combined area
  of flashes occurring concurrently occupies no more than a total of .006
  steradians within any 10 degree visual field on the screen (25% of any 10
  degree visual field on the screen) at typical viewing distance". A "red
  flash" is "any pair of opposing transitions involving a saturated red".
- **Note 1 of that definition** covers software: "For general software or web
  content, using a 341 x 256 pixel rectangle anywhere on the displayed screen
  area when the content is viewed at 1024 x 768 pixels will provide a good
  estimate of a 10 degree visual field". A quarter of that rectangle is
  21,824 px² (**C**).
- **SC 2.3.2** (Level AAA): "Web pages do not contain anything that flashes
  more than three times in any one second period". It has no area clause.

What this means for the LEDs:

- **By area.** Eight LEDs in a 96 x 28 pt accessory, even counted whole, are
  2,688 pt² (**C**). So a small LED row meets 2.3.1 by area. That is computed
  from WCAG's estimate, not measured on a screen, and it assumes WCAG's pixel
  and AppKit's point are close at those distances (**UNVERIFIED**).
- **The kernel's timing under steady traffic fails 2.3.2.** At about 5 Hz
  (2.1, **D**) it exceeds the three-flash limit whatever its size. So does any
  timing that flashes once per event when events come more than three times a
  second (6.2).
- baud already honours Reduce Motion for the cursor trail. Commit bc7cd03 is
  titled "Reduce Motion means no trail, unless the config names one". Whether
  the LEDs follow Reduce Motion is Q6.

## 3. What can be measured, signal by signal

Each subsection gives the source, what it counts, how much one read costs, and
what it cannot see. Costs (**M**) are from two launches of `leds_probe`
([[source: ah-obs-leds-probe-out]], source [[source: ah-obs-leds-probe-c]]),
each 7 rounds of 20,000 calls, quoted as the range of the two medians, unless
the row says otherwise.

### 3.1 The pty pair: RD and SD

These are bytes baud reads from the master and bytes it writes to it. They
need no read or write beyond what baud already does.

**Reads, macOS.**

- A reader thread exists only on macOS (`#[cfg(target_os = "macos")]`,
  `baud-app/src/pty.rs:880-942`). It stamps each read with its time, in
  `Read { at, bytes }` (`pty.rs:77-80`).
- `pump` drains the reads and already hands each stamp to the latency
  tracker (`app.rs:4114-4121`).
- So RD's event time can be the read's own stamp, not the drain's. The
  flash itself is drawn no earlier than the drain and the next frame,
  whatever the stamp says.

**Reads, Linux.** The Linux driver reads with `Pty::read_chunk`
(`pty.rs:828`) after `poll` returns. That is the read time.

**Writes.** What the kernel took is `at` in `write_some` (`pty.rs:253-276`).
`out.bytes.drain(..at)` at 271 removes exactly those bytes.

**SD includes replies nobody typed.** Device reports, colour answers and
DECRQSS are queued in the core's reply buffer and written by `flush_reply`.
They are real transmissions. Whether they stay in SD or get an LED of their
own is Q3.

### 3.2 Which program is in front

**What the texts say: the call is for the controlling terminal only.**

- POSIX says "The tcgetpgrp() function shall fail if:", and the list
  includes "[ENOTTY] The calling process does not have a controlling
  terminal, or the file is not the controlling terminal."
  ([[source: posix-tcgetpgrp]], note AK's rendering, 26-30). That is a
  requirement to fail, not a permission.
- The Linux man page states the same requirement: the fd "must be the
  controlling terminal of the calling process"
  ([[source: linux-man-tcgetpgrp]] roff 21-26).

**Both kernels depart from that on a pty master.** A master is not baud's
controlling terminal, yet both answer:

- **Linux exempts the master** ([[source: linux-tty-jobctrl]] 470-475):
  "(tty == real_tty) is a cheap way of testing if the tty is NOT a master
  pty", then `if (tty == real_tty && current->signal->tty != real_tty) return -ENOTTY;`.
- Linux documents the exemption for the ioctl behind the call: `TIOCGPGRP`
  is "equivalent to *argp = tcgetpgrp(fd)", and its ENOTTY reads "The
  terminal is not a master pseudoterminal and not our controlling terminal"
  ([[source: linux-man-tiocspgrp]] roff 28-30, 59-60).
- **XNU exempts the controller** ([[source: xnu-tty-dev]] 1025-1031). On the
  controller side's `TIOCGPGRP` it notes: "We aviod calling ttioctl on the
  controller since, in that case, tp must be the controlling terminal." The
  replica side does check: [[source: xnu-tty]] 1266-1270,
  `if (!isctty(p, tp)) { error = ENOTTY; ...`.
- **The sampler in 3.9 does not make that call.** It reads `e_tpgid` from
  the shell's `PROC_PIDTBSDINFO` on macOS, and field 8, `tpgid`, of the
  shell's `/proc/<pid>/stat` on Linux. Neither is a `tcgetpgrp` on the
  master, so baud would not rest on the departure.

**Measured on macOS (**M**, [[source: ah-obs-leds-probe-out]]).**

- A child became session leader on the slave. A grandchild then moved its own
  group to the foreground with `tcsetpgrp`, as a job-control shell does, and
  `tcgetpgrp(master)` returned the grandchild's group.
- `proc_pidinfo(shell, PROC_PIDTBSDINFO)` returns `e_tpgid`, "tty process
  group id" ([[source: macos-sdk-sys-proc-info-h]] 78). It equalled
  `tcgetpgrp`'s answer in both launches.

**Apple calls these interfaces private.** `libproc.h` opens with "This header
file contains private interfaces to obtain process information. These
interfaces are subject to change in future releases."
([[source: macos-sdk-libproc-h]] 42-43). They are declared in the SDK, and
tmux uses them (4). Every macOS process LED below rests on them.

| Read (macOS) | ns per call (**M**, [[source: ah-obs-leds-probe-out]]) |
|---|---|
| `tcgetpgrp(master)` | 2562-2567 |
| `proc_pidinfo(shell, PROC_PIDTBSDINFO)` (`e_tpgid`) | 158-159 |
| `proc_pidinfo(pid, PROC_PIDT_SHORTBSDINFO)` (`pbsi_comm`) | 146-147 |
| `proc_name(pid)` | 165-166 |
| `proc_pidpath(pid)` | 492-496 |
| `proc_listpgrppids(pgid)`, every live member of the group | 7475-7499 |
| `getpid()`, a floor for comparison | 87-88 |

- `proc_listpgrppids` returns a *count of pids*. The header does not say so
  ([[source: macos-sdk-libproc-h]] 94), but [[source: xnu-libproc-c]] 76-85
  divides by `sizeof(int)`. It calls `proc_listpids` with `PROC_PGRP_ONLY`
  (79). Its 8 µs suggests a walk of every process, but the kernel side was not
  read.
- **Linux.** The same reads are `/proc/<pid>/stat` field 8, `tpgid`, "The ID
  of the foreground process group of the controlling terminal of the process"
  ([[source: linux-man-proc-pid-stat]] roff 116-119), and `/proc/<pid>/comm`.
  For `comm`: "Strings longer than TASK_COMM_LEN (16) characters (including
  the terminating null byte) are silently truncated", and a thread can rewrite
  its own `comm` ([[source: linux-man-proc-pid-comm]] roff 21-32). The cost
  on Linux is **unmeasured**: there is no Linux box here.

**baud discards the child's pid today.** `Pty::fork` keeps only the master:
`struct Pty` (`pty.rs:44-56`) holds no pid, and the pid from `forkpty` (754)
is used only to tell child from parent. Every process LED needs the pid kept.

### 3.3 "Over ssh", from the process tree

The test reads the foreground group, never the title and never a string the
remote side sent.

**What the test reads.**

- A member's executable path, from `proc_pidpath` or `/proc/<pid>/exe`, names
  an ssh client or `mosh-client`. `proc_pidpath` was measured on the probe's
  own job (`/bin/sleep`, 3.2), not on the ssh client.
- For ssh, the member also holds an established TCP socket. On a live, idle
  ssh client in another terminal, `PROC_PIDLISTFDS` and then
  `PROC_PIDFDSOCKETINFO` returned family 2, `soi_kind` 2 (`SOCKINFO_TCP`) and
  `tcpsi_state` 4, `TSI_S_ESTABLISHED` ([[source: macos-sdk-sys-proc-info-h]]
  427-435 and the TSI_S table above it) (**M**, [[source: ah-obs-leds-probe-ssh-out]]).
- `PROC_PIDLISTFDS` costs 212-218 ns and `PROC_PIDFDSOCKETINFO` 818-857 ns.
  Those ranges are min to median of one launch (**M**, same file).

**What it cannot see.**

- **ssh inside `tmux attach` or `screen -r`.** The foreground of baud's pty
  is the tmux client, and the ssh runs on the tmux server's pty. This follows
  from the process model; it was not measured.
- **An ssh ControlMaster client.** It is said to reach the master over a Unix
  socket and hold no TCP socket, which would leave CD dark while the session
  is live. **UNVERIFIED**: no ssh source is in the corpus.
- **mosh.** `mosh-client` is said to use UDP, which has no connection state
  (**UNVERIFIED**). A name test still works. The UDP socket's addresses may be
  readable through `SOCKINFO_IN` (`pri_in`, [[source: macos-sdk-sys-proc-info-h]]
  559). Whether that tells "connected" from "idle" is **UNVERIFIED**.
- **OSC 7 and remote-host announcements are claims, not measurements.** OSC
  7's `file://host/path` is the remote shell's statement of where it is. It is
  honest to show as "the program says". It is not evidence that a network is
  in use. OSC 7 is planned for the title.

### 3.4 Disk

**macOS** reads `rusage_info_v4.ri_diskio_bytesread` and
`ri_diskio_byteswritten` ([[source: macos-sdk-resource-h]] 307-308) through
`proc_pid_rusage` ([[source: macos-sdk-libproc-h]] 107-111: "Return resource
usage information for the given pid, which can be a live process or a
zombie"). The header states neither what the fields count nor their unit, so
the probes measured both (**M**, [[source: ah-obs-leds-probe-out]], [[source: ah-obs-child-probe-out]]):

| Action (`leds_probe` on itself, [[source: ah-obs-leds-probe-out]], unless named) | `ri_diskio` change |
|---|---|
| 32 MiB written with `F_NOCACHE`, then `fsync` | written +33,554,432; read +0 |
| the same 32 MiB read with `F_NOCACHE` | read +33,554,432; written +0 |
| two cached reads of it, `F_NOCACHE` off | read +33,554,432 in total: consistent with the first read going to storage and the second hitting the cache |
| 32 MiB through a pipe | read +0, written +0 |
| a child writes 16 MiB uncached, still alive | the child's own counter 16,777,216; the parent +0 |
| the same after `waitpid` | parent +0 |
| `child_probe` ([[source: ah-obs-child-probe-out]]): a child pages in a 16 MiB uncached mapping | the child's own read counter 16,777,216; the parent's own +0 after `waitpid` |

So the counter is storage-layer bytes, mapped page-ins included, per process.
A child's bytes are never folded into its parent's counters, and the header
has no child diskio field ([[source: macos-sdk-resource-h]] 289-326). Reading
the shell shows nothing while its job works.

**Linux** reads `/proc/<pid>/io`.

- `read_bytes` is "The number of bytes really fetched from the storage layer"
  and `write_bytes` "The number of bytes really sent to the storage layer"
  ([[source: linux-man-proc-pid-io]] roff 68-73).
- `rchar` and `wchar` count bytes "returned by successful read(2) and similar
  system calls" (34-44). That would count pty and pipe traffic too, an
  inference from those words rather than the page's own claim, so they are the
  wrong fields.
- The file covers "the process and its waited-for children" (14-16), so a
  job's bytes reach the shell only after it is reaped.
- Access "is governed by ptrace(2) access mode PTRACE_MODE_READ_FSCREDS"
  (89-92).
- `cancelled_write_bytes` can make the total go backwards (75-86).
- Cost: an open, a read and a close per process per sample, **unmeasured**.

**The blind spot: a member started and reaped between two samples.** A
`make` in front spawns compilers that may live for less than one sampling
interval. Such a process is never sampled itself. What survives it:

- **macOS, CPU and page-ins.** Its parent's `ri_child_user_time`,
  `ri_child_system_time` and `ri_child_pageins`
  ([[source: macos-sdk-resource-h]] 301-305) take it in only after the parent
  reaps it. In `child_probe` they stayed +0 while the child was alive (**M**,
  [[source: ah-obs-child-probe-out]], two launches; source
  [[source: ah-obs-child-probe-c]]). After `waitpid` they matched the
  child's own figures as read while it was alive:
  - `child_user` +7,169,897 and +7,161,281 ticks, against the child's
    7,169,891 and 7,161,278, within 6 and 3 ticks;
  - `child_system` +99,675 and +105,485, against 99,617 and 105,453, within
    58 and 32 ticks;
  - `child_pageins` +1,024 against 1,024.

  The small excess is the child running on after it was read: a write, a read
  and `_exit`.
- **macOS, disk bytes.** There is no child diskio field, so they reach
  nothing.
- **Linux.** `/proc/<pid>/io` includes waited-for children (above), `stat`
  has `cutime` and `cstime` (fields 16 and 17, roff 161-176), and `cmajflt`
  (13): "The number of major faults that the process's waited-for children
  have made" (roff 142-144).
- **The option.** Read the `ri_child_*` fields of each sampled member as
  well. They arrive in the same `rusage_info_v4`, so the extra system-call
  cost is nothing. That catches reaped children's CPU and page-ins once their
  parent, itself a sampled member such as `make`, reaps them. It never catches
  their disk bytes on macOS. Q5 carries it.

### 3.5 CPU

**macOS** reads `ri_user_time` and `ri_system_time`
([[source: macos-sdk-resource-h]] 291-292). The header gives no unit.

- **The unit is Mach absolute-time ticks, not nanoseconds.** Over a 0.5 s busy
  loop, `getrusage` moved 496,877,000-496,883,000 ns while `ri_user_time`
  moved 11,925,043-11,925,179: a ratio of 41.667 in both launches. That is
  this machine's `mach_timebase_info`, 125/3 (**M**, [[source: ah-obs-leds-probe-out]]).
- Read as nanoseconds, the field would be 41.7 times wrong on this machine.

**Linux** reads `/proc/<pid>/stat` fields 14 and 15, `utime` and `stime`,
"measured in clock ticks (divide by sysconf(_SC_CLK_TCK))"
([[source: linux-man-proc-pid-stat]] roff 146-159). Lines 21-28 define the
`[PT]` marker for ptrace-gated fields, and the marked fields sit at 256-385.
`utime` and `stime` are not among them.

An LED needs a *rate*: the CPU time members used between two samples, over the
time between them. It can light in two ways:

- above a threshold, which is a number baud would invent;
- through the activity trigger's rate and duty mapping, which has no
  threshold and is never steady (2.2).

The choice is Q5. A child reaped between samples counts only through 3.4's
`ri_child_*` option.

### 3.6 Memory

The obvious figure is a level, not an event.

- **macOS `ri_phys_footprint`** ([[source: macos-sdk-resource-h]] 298) rose by
  67,158,064 when the probe touched 64 MiB (**M**, [[source: ah-obs-leds-probe-out]]).
- **Linux `VmRSS`**: "This value is inaccurate"
  ([[source: linux-man-proc-pid-status]] roff 199-208).

An LED lit on a level needs an invented threshold, or a rule such as "grew
since the last sample".

**There is an event counter: page-ins.**

- **macOS `ri_pageins`** (295) rose by exactly 2,048 when the probe touched a
  32 MiB file mapping at a 16 KiB stride, written with `F_NOCACHE`: one
  page-in per page touched (**M**, [[source: ah-obs-leds-probe-out]]).
- **Linux `majflt`**, field 12: "The number of major faults the process has
  made which have required loading a memory page from disk"
  ([[source: linux-man-proc-pid-stat]] roff 138-141).

So MEM can be an event LED that lights on a page fault to storage, with no
threshold. Or it can be a level gauge, which is not an LED. Q5.

### 3.7 Network bytes for one program

**macOS.** The SDK declares one record with usable counts. Whether baud can
see another process's records is open.

- **`PROC_PIDFDSOCKETINFO`** returns `struct socket_fdinfo`
  ([[source: macos-sdk-sys-proc-info-h]] 569-572).
  - Its byte-sized fields are `soi_rcv` and `soi_snd`. Each is a
    `struct sockbuf_info` whose first member is `sbi_cc` (517-525): the bytes
    queued in the buffer *now*.
  - `socket_info` holds no cumulative count (538-567). A queue level misses
    every byte that arrived and was read between two samples.
  - It does carry the socket's ports and addresses (`in_sockinfo`,
    `insi_fport`, `insi_lport`, 374-385). The probe found the ports there in
    network byte order: the raw values were the byte-swapped ports
    `getsockname` reports (**M**, [[source: ah-obs-pcb64-probe-out]]).
- **`TCP_CONNECTION_INFO`** ([[source: macos-sdk-netinet-tcp-h]] 242) returns
  `tcpi_txbytes` and `tcpi_rxbytes` (289, 292). It is a `getsockopt`,
  though, and needs a descriptor for the socket, which the ssh process holds,
  not baud.
- **`net.inet.tcp.pcblist64`: declared in the SDK.**
  - Each record is `struct xtcpcb64`
    ([[source: macos-sdk-netinet-tcp-var-h]] 493-570), which embeds
    `struct xinpcb64` ([[source: macos-sdk-netinet-in-pcb-h]] 215-250, with
    `inp_fport` and `inp_lport`). That in turn embeds `struct xsocket64`
    ([[source: macos-sdk-sys-socketvar-h]] 132-152). The dump starts with a
    `struct xinpgen` (in_pcb.h 254-259).
  - The record carries `iss`, `snd_una`, `snd_max`, `snd_nxt`, `irs`,
    `rcv_nxt` and `t_rxtshift` (tcp_var.h 507-542). Between two samples, the
    delta of `snd_una` is the bytes the peer acknowledged and the delta of
    `rcv_nxt` the bytes received in order (the SYN and a FIN each take one
    sequence number, so `snd_una - iss` and `rcv_nxt - irs` are off by one as
    absolute counts).
  - **Measured, built against the SDK's own headers with no copied layout**
    (**M**, [[source: ah-obs-pcb64-probe-out]], two launches, sandboxed;
    source [[source: ah-obs-pcb64-probe-c]]). On the
    probe's own loopback pair, sending 1,048,576 bytes moved the sender's
    `snd_una - iss` and `snd_max - iss` by exactly 1,048,576, and the
    receiver's `rcv_nxt - irs` by exactly 1,048,576. That is unlike
    `pcblist_n`'s doubled `rxbytes` (below).
  - **Attribution goes through the ports.**
    - `xsocket64` carries `so_uid` and `so_pgid` but no pid (socketvar.h
      132-152). On the probe's sockets `so_pgid` read 0.
    - So a record is tied to a process by matching its `inp_lport` and
      `inp_fport` against the ports `PROC_PIDFDSOCKETINFO` gives for that
      process's socket. The probe found both its records this way.
    - The cost is the CD row of 3.9 plus one dump.
  - **What the counts mean.**
    - The fields are 32-bit sequence numbers (`tcp_seq`), so a delta wraps at
      2^32 and has to be taken modulo 2^32 between samples.
    - `snd_una` counts bytes the peer acknowledged, not bytes sent;
      `snd_max` is the highest sent.
    - For ARQ, `t_rxtshift` > 0 ("log(2) of rexmt exp. backoff", tcp_var.h
      542), or `snd_nxt` < `snd_max` (`snd_max` is "highest sequence number
      sent; used to recognize retransmits", 508-510), shows a retransmission
      in progress at the moment of the sample (**D**). That is a state, not a
      count of retransmits.
  - **Cost.** One size query plus fetch took 4427-4502 ns, median per call,
    two launches, 200 calls by 7 rounds, on a 1,464-byte dump of three records
    (**M**, [[source: ah-obs-pcb64-probe-out]]). A dump holding every socket on the machine would be larger, and
    its cost is unmeasured.
  - **Visibility.** In both launches the dump held only the probe's own three
    records, so the live ssh client's two TCP sockets were absent. The leading header's
    `xig_count` read 53 and 56 (**M**, [[source: ah-obs-pcb64-probe-out]]). Outside the sandbox the result is
    **UNVERIFIED**, and it is listed as open (8.1).
- **NetworkStatistics, which `nettop` uses, is absent from the SDK headers.**
  `otool -L /usr/bin/nettop` lists
  `/System/Library/PrivateFrameworks/NetworkStatistics.framework`. The SDK
  carries only `.tbd` stubs for it under `PrivateFrameworks`, and no
  `ntstat*` header (**M**, [[source: ah-obs-listings-out]]).
- **`net.inet.tcp.pcblist_n`: a name in XNU, a layout absent from the SDK.**
  - It is declared with `SYSCTL_PROC(_net_inet_tcp, OID_AUTO, pcblist_n, ...)`
    ([[source: xnu-tcp-subr-c]] 2551-2553).
  - Its records `xsocket_n` (with `so_last_pid`) and `xsockstat_n` (with
    `rxbytes` and `txbytes`) are declared in XNU under `#ifdef PRIVATE` and
    `#pragma pack(4)` ([[source: xnu-socketvar-h]] 107-122; 425-577, with
    `xsocket_n` at 500 and `xsockstat_n` at 549). The SDK's own
    `sys/socketvar.h` has no `xsocket_n`.
  - The counters are kept by `so_update_tx_data_stats` and
    `sb_update_data_stats` ([[source: xnu-uipc-socket2-c]] 837-855). The
    record order is `get_pcblist_n`'s ([[source: xnu-in-pcblist-c]] 292-305
    and 405-442).
  - **Measured** with the layout copied from the XNU header, on the probe's
    own loopback sockets: sending 1,048,576 bytes moved `txbytes` by exactly
    1,048,576. Receiving the same bytes moved `rxbytes` by 2,101,312, about
    twice, which is **unexplained** (**M**, [[source: ah-obs-leds-probe-out]]).
  - With a live ssh client in another terminal, the size query returned
    40,088 B and the fetch 48 B: a leading header with `xig_count` 57 and no
    socket records (**M**, [[source: ah-obs-leds-probe-ssh-out]], sandboxed).
- **What is known about the missing sockets.**
  - **D, from the pinned code** ([[source: xnu-in-pcblist-c]] 315-320): the
    size query is computed from the system-wide `ipi_count`, plus
    `ntstat_userland_count` under `SKYWALK` (318). Together with the counts
    above (**M**, [[source: ah-obs-pcb64-probe-out]], [[source: ah-obs-leds-probe-ssh-out]]), this says the headers count sockets the fetch withheld.
  - **Not re-read for this note**; an earlier reading reports that
    `get_pcblist_n`
    (in_pcblist.c 292-484), `get_tcp_inp_list` (tcp_subr.c 2137-2164) and
    the `pcblist64` handler (tcp_subr.c 2416-2531) have no per-caller filter.
    It also reports that the trailing header (454-471) is written only past
    the `xig_count == 0` exit (369-371). That is consistent with the 48 B:
    two headers, a non-zero count, no records.
  - **The running kernel is not the pinned one.** `uname -v` reports
    `xnu-13432.1.9`; the pinned source is `xnu-12377.121.6` (**M**,
    [[source: ah-obs-listings-out]]).
  - **So the filter sits in the sandbox, or in kernel code newer than the
    pinned source. Which one is UNVERIFIED.**
  - **Also UNVERIFIED:** a reviewer's sandboxed `pcblist_n` run (35,776 and
    48 B), and two earlier unsandboxed runs that showed the
    same thing. None of those outputs was saved. The unsandboxed check is on the list (8.1).

**Linux: per-socket TCP bytes, through `sock_diag`.**

- A `NETLINK_SOCK_DIAG` request with the `INET_DIAG_INFO` extension returns,
  for TCP, "an object of type struct tcp_info"
  ([[source: linux-man-sock-diag]] roff 428-431).
- `struct tcp_info` has `tcpi_total_retrans` (287), and `tcpi_bytes_acked`
  ("RFC4898 tcpEStatsAppHCThruOctetsAcked") and `tcpi_bytes_received`
  (291-292) ([[source: linux-uapi-tcp-h]]).
- Each reply names the socket's `idiag_inode` (sock_diag.7 roff 569-570;
  [[source: linux-uapi-inet-diag-h]] 129, with `INET_DIAG_INFO` at 146).
- A process's sockets are its `/proc/<pid>/fd` links of the form
  `socket:[inode]` ([[source: linux-man-proc-pid-fd]] roff 18-33).
- `/proc/<pid>/net` is not per-process. Its files "expose information for the
  network namespace of which the process is a member"
  ([[source: linux-man-proc-pid-net]] roff 36-37).
- For UDP, and so for mosh, no per-socket byte count was found
  (**UNVERIFIED**). `tcp_info` is TCP's.
- Cost: a netlink round trip plus a `readlink` per descriptor,
  **unmeasured**.

### 3.8 A program baud may not read

The usual case is a `sudo` command in front. The probe read pid 1, which is
root-owned (**M**, [[source: ah-obs-leds-probe-out]]):

| Call | Result |
|---|---|
| `proc_pid_rusage(1)` | -1, `EPERM` |
| `proc_name(1)` | 0, empty, `EPERM` |
| `proc_pidinfo(1, PROC_PIDT_SHORTBSDINFO)` | 64 bytes, `pbsi_comm` "launchd" |
| `proc_pidinfo(1, PROC_PIDLISTFDS)` | 0, `EPERM` |

So on macOS baud can name a root process but cannot read its disk, CPU,
memory or sockets.

On Linux, `/proc/<pid>/io` is ptrace-gated (3.4). For `stat`, "If the check
denies access, then the field value is displayed as 0" for `[PT]` fields
([[source: linux-man-proc-pid-stat]] roff 21-28). The `[PT]` fields are at
256-385; `utime`, `stime` and `majflt` are not among them.

**A failed read must not look like a quiet process.** How it looks instead is
Q8, decided as a look of its own (8).

### 3.9 What a sample costs

All rows are computed from the medians above, taking the upper end of each range across both launches (**C**).

| Sample (macOS) | Calls | Cost |
|---|---|---|
| foreground group | `PROC_PIDTBSDINFO` on the shell | 0.16 µs |
| group members | `proc_listpgrppids` | 7.5 µs |
| disk, CPU, page-ins and `ri_child_*`, per member | `proc_pid_rusage` | 0.47 µs each |
| name, only when the group changes | `PROC_PIDT_SHORTBSDINFO`, `proc_pidpath` | 0.15 + 0.50 µs |
| CD (ssh members only) | `PROC_PIDLISTFDS` once per member, then `PROC_PIDFDSOCKETINFO` per socket | 0.22 µs + 0.86 µs a socket |
| NET pair (ssh members only), if other processes' records are visible (3.7) | the CD row, then one `pcblist64` size query and fetch | 4.5 µs on a three-record dump; a full dump unmeasured |

- A one-member group costs about 8.1 µs a sample. At ten samples a second
  that is about 81 µs of CPU a second, about 0.008% of one core.
- `proc_listpgrppids` is nine-tenths of that. Listing members only when the
  group changes would miss processes that join it, such as the compilers a
  `make` spawns. So the sampler lists every time.
- Linux: every row is **unmeasured**.

## 4. Prior art

**tmux names the foreground program from the pane's master.**

- On Darwin it calls `tcgetpgrp(fd)`, then
  `proc_pidinfo(pgrp, PROC_PIDT_SHORTBSDINFO, ...)`, and returns `pbsi_comm`
  ([[source: tmux-osdep-darwin-c-9c22ff6]] 45-51).
- On Linux it reads `/proc/<pgrp>/cmdline`
  ([[source: tmux-osdep-linux-c-9c22ff6]] 38-46).
- Both use the group id as a pid. That reads only the leader, and fails for a
  group whose leader has exited.
- It re-reads only when the pane is flagged changed:
  `if (~w->active->flags & PANE_CHANGED)` ([[source: tmux-names-c-9c22ff6]]
  66-69). If a check happened less than `NAME_INTERVAL` ago (500,000 µs,
  [[source: tmux-tmux-h-9c22ff6]] 120), it waits out the rest on a timer
  (names.c 72-82).
- `PANE_CHANGED` is set in several places, among them:
  - every non-empty parse of the pane's output:
    `input_parse_buffer` (`if (len == 0) return;`, then
    `wp->flags |= PANE_CHANGED;`) ([[source: tmux-input-c-9c22ff6]] 1047-1056);
  - a change of active pane ([[source: tmux-window-c-9c22ff6]] 777, 1232);
  - entering a mode (1839) and leaving one (1882);
  - `break-pane` ([[source: tmux-cmd-break-pane-c-9c22ff6]] 182);
  - turning `automatic-rename` on, for every window's active pane
    ([[source: tmux-options-c-9c22ff6]] 1379-1386).
- So the name is re-read after output, a pane change, a mode change and the
  other events among those sites, at most twice a second.

**iTerm2's status bar shows CPU, memory and network for the whole machine,
on a timer.**

- Network sums `ifi_ibytes` and `ifi_obytes` over every interface from
  `NET_RT_IFLIST2` ([[source: iterm2-itermnetworkutilization-964a75d]]
  122-167), skipping `lo` (157-159).
- CPU uses `HOST_CPU_LOAD_INFO`
  ([[source: iterm2-itermcpuutilization-964a75d]] 90-93), and memory uses
  `HOST_VM_INFO64` ([[source: iterm2-itermmemoryutilization-964a75d]] 108).
- Each runs on an `NSTimer` at 1 s: CPU at 58, network at 72 and 181, memory
  at 39 and 129. The network timer is invalidated when nothing subscribes
  (176-179).
- None of them is per session. They answer "is the machine busy", not "is my
  program busy".

**The kernel triggers** (2.1, 2.2):

- A pty has no kernel counter.
- The network trigger watches an interface.
- The disk trigger is driven by calls, not by a poll.
- The CPU trigger has no threshold.

## 5. baud today

### 5.1 The title bar strip

**glassnote's titled shape** (glassnote's src/window.rs
262-276, read only):

- It calls `setTitlebarAppearsTransparent(true)` and
  `setTitleVisibility(NSWindowTitleVisibility::Hidden)`. Its comment drops the
  title text because "the surface draws its own".
- The doc of the getter `title()` (465-469) says the title is carried because
  "it is what the Window menu, Mission Control and accessibility clients
  read". `set_title` (475-478) is documented only as "Change the window's
  title."

**The crate has a titlebar-accessory API since 1.0.0-beta.8.**

- `GlassWindow::add_titlebar_accessory(&self, view, TitlebarSide)` puts a
  view in the title bar as an `NSTitlebarAccessoryViewController`, leading
  or trailing, and returns the controller, which the window also keeps.
- Releases before beta.8 had no `NSTitlebarAccessoryViewController`, no
  `NSToolbar` and no `layoutAttribute`; baud's strip was views of its own
  in its content, placed by hand.
- baud uses the crates.io release `macos-liquid-glass` 1.0.0-beta.8.

**What baud has in the strip now.**

- The lockup is a leading accessory and the lights, on their plate, a
  trailing one (`baud-app/src/strip.rs`, `mark.rs`, `lights.rs`). The
  lights' accessory is sized at every layout: AppKit lets a narrow title
  bar's two ends overlap.
- In full screen the title bar leaves the window ("Fullscreen window
  toolbar is detached from window and hides/shows on rollover",
  [[source: sdk27-nsapplication-h]] 168). Views in baud's content stayed
  behind: the lockup indented for buttons that were gone, and an empty
  bar slid over it on reveal. As accessories they are in the buttons' own
  view, which the window test checks headless; that AppKit shows them with
  the buttons in full screen is not measured, since a window enters full
  screen only when it is shown.
- The terminal's view stops the title bar's height short of the top while
  windowed, measured from `contentLayoutRect`, and takes the whole window
  in full screen. The switch is made on `NSWindowDidEnterFullScreenNotification`
  and `NSWindowDidExitFullScreenNotification` ([[source: sdk27-nswindow-h]]
  976, 978), read by name (`baud-present-macos/src/full_screen.rs`).
- Measured in baud's own test windows (`baud-app/tests/titlebar_mark.rs`,
  never shown): title bar 28 pt, buttons 14 pt wide at x 9, 29 and 49, the
  leading accessory at x 69, the trailing one flush right. That is where
  the hand-placed views were.

**AppKit's trailing accessory, by the SDK header**
([[source: macos-sdk-nstitlebaraccessoryviewcontroller-h]]):

- Line 23: "NSLayoutAttributeRight is also supported, telling the window to
  place the view controller's view on the right side of the window."
- Line 27: "For applications linked on 10.12 and higher,
  NSLayoutAttributeLeading and NSLayoutAttributeTrailing can also be used to
  specify an abstract position that automatically flips depending on the
  localized language."
- Line 31: "For applications linked on macOS 27.0 or higher, leading and
  trailing accessories are positioned within the titlebar area only
  (excluding the toolbar) when the window uses NSWindowToolbarStyleExpanded
  or NSWindowToolbarStylePreference, or when the window has no toolbar."
- Line 64: "changing the width when the layoutAttribute is
  NSLayoutAttributeRight. The other size direction will automatically be
  filled to the maximum size as required for the window."
- The window method is `addTitlebarAccessoryViewController:`
  ([[source: macos-sdk-nswindow-h]] 323).

**Measured** (**M**, cited below). Line 31 makes placement depend on the SDK an app links
against, so what matters is baud's. baud's binary records
`LC_BUILD_VERSION sdk 14.4, minos 14.0` (`otool -l target/release/baud`, recorded in [[source: ah-obs-titlebar-probe-sdk14-out]]).
Two probes built a window with glassnote's style mask and flags and no
toolbar:

- **`titlebar_probe_sdk14.m`** ([[source: ah-obs-titlebar-probe-sdk14-m]]), built with the devenv shell's nix clang and
  apple-sdk 14.4, carries the same `sdk 14.4, minos 14.0` as baud.
- **`titlebar_probe.swift`** ([[source: ah-obs-titlebar-probe-swift]]), built with `/usr/bin/swiftc -sdk MacOSX27.0.sdk`,
  nonetheless records `sdk 14.4, minos 27.0`.

Both gave the same result ([[source: ah-obs-titlebar-probe-sdk14-out]], [[source: ah-obs-titlebar-probe-out]]):

- A 96 pt `.trailing` accessory landed at x 704 to 800 in an 800 pt window:
  flush right, at the strip's full height of 28 pt. `.right` landed in the
  same place.
- `contentLayoutRect` was 472 pt tall in a 500 pt window, which leaves a
  28 pt strip.
- The traffic lights sit at x 9, 29 and 49, each 14 pt wide.

Placement for a binary linked against SDK 27.0 was not measured. Placement
inside baud's own window was measured later, by the window test above:
the same 28 pt and x 69.

The width on the right is ours to choose, whether the LEDs are an accessory
or hand-placed views. The real constraint is the title text, if the title work puts it back in the strip.

### 5.2 Where a sample can run

**macOS: the display link.**

- `fn frame` in `impl Responder for Driver` (`app.rs:4589`) runs on every
  display-link tick, idle ones included. Per the comment at 4692-4693: "Every
  tick, not every frame: an idle screen builds no frame and is the one this
  has to be able to report on".
- `App::report` (`app.rs:1640-1641`) is the existing elapsed-time gate:
  `if self.last_report.elapsed().as_secs() >= 5`.
- An LED sampler gated the same way adds no wakeup on macOS while the link
  ticks.
- If the link is ever paused while idle (P5, "idle CPU on this branch"), a lit
  LED's end has to be scheduled on the link. That is the
  scheduled-never-waited rule.

**Linux: the poll timeout.**

- `linux.rs:248-253` takes
  `[sync, repeat, flash, caret].into_iter().flatten().min()`.
- `caret` is the cursor's next blink flip (`Caret::next_change`,
  `caret.rs:321-345`). It is `None` while the cursor does not blink, and
  blinking depends on focus (`blinks()` passes `self.focus`,
  `linux.rs:344-350`).
- The defaults blink on Linux:
  - the driver sets the power-on cursor with
    `set_initial_cursor_style(CursorStyle::of(cfg.cursor_shape, cfg.cursor_blink.power_on()))`
    (`linux.rs:168-171`), which overrides the core's own default;
  - `[cursor] blink` defaults to `Auto` and `stop_blinking_after` to `Never`
    (`config.rs:467-468`), and `power_on` maps `Auto` to `Blinking`
    (`config.rs:390-393`);
  - the driver's gate is `Gate::of(cfg.cursor_blink, NonBlinking::Off)`
    (`linux.rs:199`), and `Gate::of` steadies the caret only for
    `NonBlinking::On` with `Auto` or `Off` (`caret.rs:183-188`);
  - a cursor hidden by DECTCEM stops the blink: `blinks()` passes
    `cursor_visible()` (`linux.rs:348`).
- So the baseline is this:
  - **focused, default cursor, visible:** the poll already wakes at every
    caret flip;
  - **unfocused, a steady cursor, or the cursor hidden:** nothing wakes.

**What LEDs add on Linux, against that baseline.**

- **pty pair only:** a deadline only while an LED is lit, to turn it off.
  With no traffic, nothing is added in any state.
- **process LEDs:** a deadline every sample.
  - Focused with the default visible cursor, the added wakes are the samples
    that do not coincide with a caret flip.
  - Unfocused, with a steady cursor, or with the cursor hidden, an idle baud
    starts waking at the sample rate.
- **off:** nothing is added.

### 5.3 Byte counts

None exist; the places are named in 3.1. The app's counters are
`baud_render::Tally` (`baud-render/src/units.rs:9`), as `frames` and `ticks`
are. A raw `u64` field would count against the audit's
`crate_audit "baud-app" 1 4`, which is set at the count
(`audit_all.sh:243-252`).

### 5.4 The core's state: "what protocols are used"

`pub fn` on `Term` (`baud-core/src/term.rs`) that could each back a state
LED, held on while set:

| State | Accessor | Line |
|---|---|---|
| input encoding, UTF-8 or ISO 8859-1 | `input_encoding()` | 1749 |
| 8-bit C1 transmission (`S8C1T`) | `c1_transmit()` | 1754 |
| kitty keyboard flags | `kitty_flags()` | 1847 |
| mouse tracking mode (Off, X10Compat, Normal, ButtonEvent, AnyEvent) | `mouse_tracking()` | 2295 |
| bracketed paste (`?2004`) | `bracketed_paste()` | 1719 |
| synchronized output (`?2026`) held open | `synchronized_output()` | 1665 |
| alternate screen | `screen()` | 1656 |
| VT52 mode | `terminal_mode()` | 2496 |
| highlight tracking (`?1001`) | `mouse_tracking()` (`Highlight`), `highlight()` since 2026-10-11 | 2318 |
| alternate scroll (`?1007`) | `alternate_scroll()` | 2307 |
| application cursor keys, keypad | `cursor_keys_application()`, `keypad()` | 2707, 2712 |
| an image placed on screen | `images().any_placed()` | term.rs 2248; `baud-domain/src/image.rs:724` |

Public counters on `Term` that could each back an event LED, blinking when
the count moves:

| Counter | Line |
|---|---|
| `bells` | 819 |
| `clipboard_reads_refused`, `clipboard_writes_dropped` (OSC 52) | 839-840 |
| `reply_overflows` | 798 |
| `links_refused` | 833 |
| `mouse_unencodable` | 870 |

There is **no public accessor** for `modifyOtherKeys` (`other_keys`,
`pub(crate)`, term.rs:412) or for the mouse encoding (`mouse_encoding`,
`pub(crate)`, term.rs:304). Showing either needs one. Kitty graphics is
unbuilt (note V, V19), so the image LED is Sixel only.

### 5.5 Config

- `baud-app/src/config.rs` is a hand-written TOML subset. Its sections are
  the top level, `[palette]`, `[cursor]`, `[screen]`, `[scroll]`, `[images]`
  and `[glass]`. There is no `[titlebar]` and no `[window]`.
- The idiom for an on/off key is a two-state enum, not a `bool`:
  `ScreenTransition` (412), parsed at 676-688 from `"true"` or `"false"`,
  with a warning for anything else.
- `impl_audit.py` flags `^\s+\w+: bool,` as "bool as a flag field".

## 6. Options

### 6.1 The LEDs, each with what lights it

| LED | Lights when | Kind | Source (macOS / Linux) | macOS | Linux | Cost |
|---|---|---|---|---|---|---|
| **RD** | baud read bytes from the master | event | the read itself (`Read.at` / `read_chunk`) | yes | yes | no extra read; on Linux one deadline per lit LED; redraw unmeasured |
| **SD** | the kernel took bytes baud wrote | event | `write_some`'s `at` | yes | yes | as RD |
| **AA**? (Q3) | baud wrote a reply nobody typed (DA, CPR, OSC 4/10/11, DECRQSS, focus, `?2048`, `?2031`) | event | the core's reply buffer, around each place it is filled without a key or mouse event | yes | yes | as RD |
| **CD** | a foreground ssh member holds an established TCP socket | state | `PROC_PIDFDSOCKETINFO` `tcpsi_state` 4 / `sock_diag` | yes | yes | 0.22 µs once per member + 0.86 µs a socket (**C**) / unmeasured |
| **NET pair** | a foreground ssh member's connection moved bytes (TX: acknowledged by the peer; RX: received in order) | event | `pcblist64` `snd_una - iss`, `rcv_nxt - irs`, matched by ports (3.7) / `tcpi_bytes_acked`, `tcpi_bytes_received` | **open**: declared, counts exact on own sockets; other processes' records invisible from inside the sandbox; **UNVERIFIED** outside it (8.1) | yes | the CD row + 4.5 µs a three-record dump; a full dump unmeasured / unmeasured |
| **DK** (or DR/DW) | a foreground member's storage bytes moved | event | `ri_diskio_*` / `read_bytes`, `write_bytes` | yes | yes | 0.47 µs a member (**C**) |
| **CPU** | a foreground member ran | event, rate-mapped (Q5) | `ri_user_time` + `ri_system_time` in ticks, optionally `ri_child_*` / `utime` + `stime`, optionally `cutime` + `cstime` | yes | yes | the same call as DK |
| **MEM** | a foreground member paged in from storage (Q5) | event | `ri_pageins`, optionally `ri_child_pageins` / `majflt`, optionally `cmajflt` | yes | yes | the same call as DK |
| **state LEDs** | a mode in 5.4 is set | state | `Term` accessors | yes | yes | a field read |
| any of the above, for an unreadable member | the read failed (Q8) | — | the read's error (3.8) | yes | yes | — |

**The modem labels, each with an analogue in baud and what it rests on.** The
modem senses are quoted in 2.3. The analogue is baud's own definition, and
the note says so wherever it is used.

| Modem LED | Its sense (2.3) | baud analogue | What it rests on |
|---|---|---|---|
| SD | the computer sends to the modem | baud wrote to the master: keys, paste, replies | `write_some`'s `at` |
| RD | the modem passes data, and its own result codes, to the computer | baud read from the master | each read (3.1) |
| CD | a carrier from a distant modem | a foreground ssh member holds an established TCP socket | `tcpsi_state` 4 / `sock_diag` |
| OH | "using the telephone line" | a job other than the shell is in front | `e_tpgid` differs from the shell's own group (3.2) |
| AA | auto-answer set; "Flashes ON for incoming ring detect" | flashes when baud queued a reply nobody typed. The steady half would always be on, so it is dropped | the reply buffer, filled other than by a key or mouse event: replies queued during a feed and those outside one (note AG: focus, the `?2048` report after a resize, `?2031`) |
| HS | the rate the modem last ran at, held: computer-side in command state, the line's at connect, kept after disconnect | a looser analogue: a read came back at the platform's ceiling, a sign that more was waiting, though not proof (exactly that many bytes may have been all there was) | a read's length: 1024 on Darwin, 4095 on Linux (CONTRIBUTING.md's `READ_CHUNK` row, [[source: linux-n-tty]]), under `READ_CHUNK` = 4096 (`pty.rs:90`). On macOS, each reader-thread chunk's length (`Read.bytes`), not `pump`'s drain |
| CS, **inverted** | ON normally; OFF when the modem lowers CTS to stop the computer | lit when the kernel refused baud's write: the child is not reading its input, or the child is gone (`pty.rs:97-101`, 265-267). The modem's polarity would always be on, so baud's CS is the inverse and must say so, or take another name | `write_some` returns `Wrote::Short` (`pty.rs:272-275`) |
| RS, inverted | ON normally; OFF under `&R2` when the computer lowers RTS | lit when baud stopped reading the master. baud never stops today, on either system, so it would never light (2.3) | none today; a backpressure rule, if one is built |
| ARQ | error control; flashes on a retransmit | the ssh socket is retransmitting or has retransmitted | Linux: `tcpi_total_retrans`, a count ([[source: linux-uapi-tcp-h]] 287). macOS: `pcblist64` `t_rxtshift` > 0 or `snd_nxt` < `snd_max`, a state at the sample, not a count, with the same open visibility and cost as the NET pair (3.7) |
| SYN | synchronous mode | synchronized output (`?2026`) held open | `synchronized_output()` |
| FAX (shares ARQ's LED) | fax mode | an image stored or placed (Sixel) | `images().generation()` moved, or `any_placed()` |
| MR | Hayes 300: always on; Courier: flashes on retrain or test | none: baud has no retrain | -- |
| TR | DTR asserted | none: the master held open is always true while baud runs | -- |
| AL | analog loopback self-test | none | -- |

HS, CS, AA and OH should add no ingest cost, though that is **unmeasured**:

- HS and CS read values baud already has: a read's length and a write's
  result.
- AA compares the reply buffer's length around each place a reply is queued
  without a key or a mouse event.
- OH compares two fields from a call the process LEDs make anyway.

### 6.2 How a blink is timed

Two timings for event LEDs, and one for CPU only. The flash rate under steady
traffic depends on the mechanism, so each is given.

- **A. One flash per counted event.**
  - **Event-driven, 50 ms on and then a 50 ms off lockout** (events during
    the flash and the lockout dropped): under steady traffic, a flash every
    100 ms, about 10 Hz (**D**).
  - **Event-driven, 50 ms on with no off lockout**, re-armed by each event:
    under steady traffic the flashes run together and look close to steady
    (**D**).
  - **Polled every 100 ms, copying the kernel's swallowed one-shot:** a flash
    every other poll, about 5 Hz (2.1, **D**).
  - A single byte shows for 50 ms: 3 frames at 60 Hz, 6 at 120 Hz (**C**).
  - **WCAG.** The 10 Hz and 5 Hz forms fail 2.3.2 under steady traffic. All
    three can fail it for bursts that come more than three times a second.
    2.3.1 is met by area (2.4).
  - **Linux wakes.** An event-driven flash adds one deadline, to end it, per
    flash and per lockout. A polled form adds a deadline every 100 ms while
    the feature is on, traffic or not (5.2).
- **B. Lit for a hold after the last event.** For example 100 ms. Under steady
  traffic it stays lit; with no traffic it is dark.
  - A single byte shows for the whole hold.
  - **WCAG.** It still flashes once per burst. Typing faster than three keys a
    second, with gaps longer than the hold, gives more than three flashes a
    second, which fails 2.3.2. 2.3.1 is met by area (2.4).
  - **Linux wakes.** One deadline per hold, moved later by each event. With no
    traffic there is none.
- **C. Rate-mapped, for CPU only.** This is the activity trigger's mapping
  (2.2): never steady, faint at idle, 10 Hz at one core's worth of load.
  - **WCAG.** It exceeds three flashes a second above light load (**D**).
  - **Linux wakes.** Its own timer at up to 100 ms intervals, and as close as
    10 ms at the shortest duty steps (2.2), for as long as the feature is on,
    including at idle, where it blinks "10ms every second".

Each LED states its sampling interval:

- RD, SD and AA light as the event happens, per read or write.
- Process LEDs take one sample per interval (Q4).
  - An event inside a member that lives across a sample is counted, because
    its counters accumulate. What is lost is *when* within the interval it
    happened.
  - A member started and reaped between two samples is not seen directly. Its
    CPU and page-ins appear only through its parent's `ri_child_*`, once
    reaped, if that option is taken. Its disk bytes do not appear on macOS
    (3.4).

### 6.3 Placement

Each option, with its benefit and its cost:

- **P1. An accessory API in macos-liquid-glass.** For example, a
  `GlassWindow` method that takes a view and a leading or trailing side and
  wraps `NSTitlebarAccessoryViewController`.
  - **Benefit:** it follows the project's rule to extend the crate, never
    duplicate it, and AppKit lays the view out.
  - **Cost:** a crate change, which reaches baud through a release or a
    `[patch]`.
- **P2. A trailing accessory added by baud itself, through `ns_window()`.**
  - baud-app already imports AppKit through
    `macos_liquid_glass::objc2_app_kit` (`app.rs:42-46`).
  - In objc2-app-kit 0.3.2, `addTitlebarAccessoryViewController`
    (`src/generated/NSWindow.rs:699`) and `setLayoutAttribute`
    (`src/generated/NSTitlebarAccessoryViewController.rs:87`) need three
    features, all in its default list (`Cargo.toml` `default` at 2017-2323):
    `"NSTitlebarAccessoryViewController"` (2277), `"NSViewController"` (2299)
    and `"NSLayoutConstraint"` (2130).
  - macos-liquid-glass depends on objc2-app-kit without turning default
    features off.
  - **Benefit:** AppKit lays the view out, and it works today with no crate
    release.
  - **Cost:** it is AppKit code in baud for something the crate's subject
    covers. Whether that counts as duplicating the window crate is open.
- **P3. Hand-placed subviews in baud's container, as the icon is.**
  - **Benefit:** baud's existing idiom (5.1), with no crate change.
  - **Cost:** baud places the views itself and re-places them on every
    resize, as `place_icon` does.
- **P4. Metal in the strip.**
  - **Benefit:** one renderer for the grid and the LEDs, and the same pixels
    on Linux, which has no AppKit title bar. That serves the
    same-pixels-on-every-OS rule.
  - **Cost:** the Metal view would have to extend under the strip; it is
    framed `full.height - inset` today (`app.rs` 4972-4975), with row 0 kept
    clear of the traffic lights. An LED change would build a frame even when
    the grid is idle, where today "an idle screen builds no frame"
    (`app.rs` 4692-4693). Both costs are **unmeasured**.

P1, P2 and P3 change a layer property only when an LED's state changes. That
cost is **unmeasured**.

### 6.4 Labels and colours

- **Two-letter modem labels** (RD, SD, CD, OH, HS, CS, ARQ, SYN) read as a
  modem. Each carries meaning only through its analogue in 6.1. Labels no
  modem had (DK, CPU, MEM) can be set in the same lettering.
- **Icons** (an arrow pair, a disk, a chip) read faster, and lose the modem
  look.
- **Colour.**
  - The manuals read say nothing of the panels' colours, except the
    V.Everything guide's "Configure the HS LED to turn red when your modem
    reaches speeds over 33.3 kbps" ([[source: usr-courier-veverything-1997]]
    PDF p.69). That the lights were mostly red is **UNVERIFIED**.
  - WCAG's red-flash clause makes saturated red the riskiest choice for
    anything that flashes (2.4).
  - The palette's colour roles (note Y, `docs/research/notes/y-palettes.md`)
    are where a colour would come from.
- **How many fit.** The width is ours (5.1). At an assumed 10 pt pitch, eight
  LEDs take 80 pt, under the 96 pt the probes placed. That is **C** from an
  assumed pitch, not a measured one.

### 6.5 Config

`[titlebar] leds = true | false` follows the `[screen] transition` idiom, a
two-state enum. Per-LED keys, for example `leds = ["rd", "sd", "cd", "dk"]`,
were the other option; Q2 decided one switch (8).

What `leds = false` is meant to mean:

- no counter is read;
- no deadline is added on Linux;
- no accessory or view is created;
- the child's pid need not be kept.

That is the design's intent. It is shown by the test in section 7, not by this
list.

### 6.6 What "honest" means, as rules a test could hold

1. Every LED names its source and its sampling interval, in this note and in
   its tooltip if a tooltip is wanted.
2. Lit means an event was counted in the window, and dark means the read
   worked and counted nothing. A failed read is neither (3.8, Q8).
3. Process LEDs read every live member of the foreground group, not the
   shell (3.4), and state the reaped-between-samples blind spot.
4. No LED is lit from the title, from OSC 7 or from any string the
   application sent, unless it is labelled as the application's claim.
5. No LED is always on or never lit. The Hayes 300's MR and baud's TR
   analogue would always be on, and an inverted RS would never light today
   (2.3), so none of them carries information.
6. A pty-pair LED adds no read or write. A process LED adds the calls in 3.9,
   and nothing when the feature is off.

## 7. Red-first tests (whichever options are chosen)

- **RD and SD.** Feed a byte and the LED lights for its timing. Feed nothing
  and it stays dark. The light ends on schedule with no further input.
- **SD counts only what the kernel took.** A full pty (`EAGAIN`) does not
  light SD for bytes still queued.
- **The foreground group.** A child that moves its group to the foreground is
  named within one sample.
- **Disk, a non-leader member.** The test builds a process group itself,
  with `setpgid`. The leader exits at once, and a second member reads an
  uncached file. DK lights while that member is alive.
- **The blind spot, stated.** A member that runs and is reaped between two
  samples does not light DK on macOS. With the `ri_child_*` option, its CPU
  shows once its parent reaps it.
- **No reading.** A root-owned process in front is not shown as dark.
- **Zero cost when off.** With `leds = false`, a run of the app makes no
  `proc_*` call. This is checked with `dtruss` or a counting shim, never by
  argument.

## 8. Open questions

Each needs its options rendered before it is settled.

- **Q1. "What protocols are used": which meaning?**
  - (a) The session's transport, named from the process tree: ssh, mosh,
    telnet or a serial tool.
  - (b) The terminal protocols in play between the program and baud: kitty
    keyboard, mouse mode, bracketed paste, sync output, alternate screen,
    images, 8-bit controls and encoding.
  - (c) Both.
- **Q2. Which LEDs, and how many?**
  - The pty pair.
  - Over ssh: CD, and a network pair. The second request asks for exactly that pair: "it should show the tx/rx of network if we are over
    ssh". What it costs and where it stands:
    - **Linux:** buildable from `sock_diag`; cost unmeasured.
    - **macOS:** the record is declared in the SDK, and its counts were exact
      on the probe's own sockets. Whether baud can see another process's
      socket is **UNVERIFIED**: two unsandboxed runs earlier in this research
      showed the same thing and were not saved (3.7); the check is 8.1.
      If it can, the cost is 4.5 µs per sample on the probe's three-record
      dump plus the CD row (3.9); a full dump, which visibility would return,
      is unmeasured.
    - **What it counts:** socket traffic, which includes traffic that does not
      pass through baud's pty (section 0; that list is UNVERIFIED).
    - **The choice:** a network pair on both systems, on Linux only if macOS
      visibility fails, or none?
  - DK (one LED, or read and write), CPU and MEM.
  - Which state LEDs from 5.4.
  - One switch, `[titlebar] leds`, or one key per LED?
    - **Decided: one switch.** No text governs; it matches the request's one setting to turn them off.
    - **Unbuilt, and why.** The key is `[titlebar] leds = true | false`, a
      two-state enum in the `[screen] transition` idiom (6.5), and off means
      none of 6.5's list. But no light exists: which ones (the rest of Q2)
      and where they are drawn (Q7) are open. A key read and acted on by
      nothing is the config counterpart of the "parsed" state CONTRIBUTING.md calls worse than unbuilt, and section 7's zero-cost test would pass
      vacuously today, since nothing calls `proc_*`. So the key lands with
      the first light, and its test with it.
- **Q3. Replies nobody typed: inside SD, or their own LED (AA)?**
- **Q4. The sampling interval for process LEDs.**
  - The kernel trigger polls every 100 ms (2.1).
  - tmux re-reads a name at most twice a second, after the events that flag
    its pane changed: output, a pane or mode change, `break-pane`, and
    turning `automatic-rename` on, among others (4).
  - iTerm2 samples every 1 s (4).
  - On Linux, focused with the default visible blinking cursor, the poll
    already wakes at every caret flip. Unfocused, with a steady cursor, or
    with the cursor hidden, nothing wakes today, and each sample would be a
    new wake (5.2).
- **Q5. CPU and MEM: event or gauge, and reaped children?**
  - CPU: a threshold, or the activity trigger's never-steady rate mapping?
  - MEM: page-ins (an event, no threshold), or footprint growth (a level that
    needs a rule)?
  - Should members' `ri_child_*` be read too? That catches the CPU and
    page-ins of children reaped between samples once their parent reaps them,
    never their disk bytes on macOS (3.4).
- **Q6. Timing.**
  - Option A (6.2): one 50 ms flash per counted event. Under steady traffic
    it gives about 10 Hz event-driven with an off lockout, near-steady with
    none, or about 5 Hz if polled every 100 ms like the kernel.
  - Option B (6.2): lit for a hold after the last event.
  - Both can exceed WCAG 2.3.2's three flashes a second.
  - Should the LEDs follow Reduce Motion, as the cursor trail does?
- **Q7. Placement and look.**
  - P1: an accessory API in the window crate (follows the extend-the-crate
    rule; needs a release or `[patch]`).
  - P2: an accessory added by baud through `ns_window()` (works today; may
    count as duplicating the crate).
  - P3: subviews, as the icon is (baud's idiom; baud lays them out).
  - P4: Metal in the strip (the same pixels on Linux; extends the view under
    the strip and builds frames for LED changes).
  - Modem two-letter labels or icons? Which colours?
- **Q8. A program baud may not read.** Should it get its own look, such as a
  hollow LED, or should its LEDs hide?
  - **Decided: its own look.**
    No text governs; it keeps rule 2 of 6.6, that a failed read is neither
    lit nor dark.
  - What it means once built: each process light has a third state,
    unreadable, set when the read is refused, and drawn in a look of its
    own: on macOS when `proc_pid_rusage` answers `EPERM` (3.8); on Linux
    only DK's `/proc/<pid>/io` is gated (3.4, 3.8), since `utime`, `stime`
    and `majflt` stay readable. The hollow LED is an example; the look is still
    to be designed, and rendered before it is chosen.
  - **Unbuilt, and why.** Which process lights exist (Q2's DK, CPU and
    MEM, Q5), where they are drawn (Q7) and the look itself are open, so
    the state has no light to belong to. A reading that told `EPERM` apart
    from a quiet process would have no caller. It lands with the first
    process light, with section 7's "No reading" test red first against a
    root-owned process in front (the probe's pid 1 shows `proc_pid_rusage`
    answering `EPERM`, 3.8).
- **Q9. The modem labels' analogues (6.1).** Do these suit, or should baud
  name the signals plainly?
  - OH: a job is in front.
  - HS, looser: a read came back at the platform's ceiling.
  - CS, inverted from the modem's: lit while the child is not reading, or is
    gone.
  - RS, inverted: lit while baud is not reading. It never lights today.
  - SYN: synchronized output.
  - FAX: an image.
  - AA: a reply nobody typed.

### 8.1 Checks to run

These need a run outside the sandbox, which this research made
only without saving its output (3.7, UNVERIFIED). Each one needs an ssh session open in another terminal, and each is
run from an ordinary terminal (not inside the sandbox).

The commands below run the two probes, built from their committed sources,
[[source: ah-obs-pcb64-probe-c]] and [[source: ah-obs-leds-probe-c]], with the
build line in each file's header into `PROBES`, a directory of your choice.

1. **`pcblist64`: can an unsandboxed process see another process's TCP
   socket?** Look for "its record FOUND" or "absent" in the last lines.
   An ssh client with no TCP socket of its own (a ControlMaster client)
   prints neither; "no ssh client running" means none was found.

   ```
   pid=$(pgrep -x ssh | head -1); [ -n "$pid" ] && "$PROBES"/pcb64_probe "$pid" || echo "no ssh client running"
   ```

2. **`pcblist_n`: the same question.** Look at the "sockets in dump" lines
   and at "with so_last_pid == ...".

   ```
   pid=$(pgrep -x ssh | head -1); [ -n "$pid" ] && "$PROBES"/leds_probe --pid "$pid" || echo "no ssh client running"
   ```

3. **netstat: does it list TCP connections?**

   ```
   /usr/sbin/netstat -an -p tcp | head -20
   ```

## 9. Found on the way, decided by no question

- **baud keeps no child pid** (3.2). Every process LED needs it, and so does
  anything else that asks what is in front.
- **`modifyOtherKeys` and the mouse encoding have no public accessor** (5.4).
- **`ri_user_time` is in Mach ticks** (3.5). Any future use of `rusage_info`
  in baud has to convert through `mach_timebase_info`, or it is 41.7 times
  wrong on this machine.
- **`insi_lport` and `insi_fport` hold the port in network byte order**
  (3.7, measured). The header (`in_sockinfo`, proc_info.h 374-385) declares
  them as `int` and does not say so. Matching them against `pcblist64`'s
  `inp_lport`/`inp_fport` needs `ntohs` on both sides.
- **`libproc.h` calls itself private** (3.2). Anything baud builds on
  `proc_pidinfo` or `proc_pid_rusage` rests on interfaces Apple says are
  "subject to change in future releases".
- **The kernel tty trigger cannot watch a pty** (2.1). An "LED for the tty"
  that read kernel counters on Linux would always be dark, so the counts have
  to be baud's own.

## 10. Built: the RD and SD pair (2026-10-24)

Roadmap F1-20 is the pty pair only, so this is what was built of the note.
Q1, Q3 to Q5, Q8 and the rest of Q2 stay open; section 11 answers Q3, Q4,
Q6's Reduce Motion half and Q8, and part of Q1, Q2 and Q9.

- **What lights them.** RD: the stamp the reader already puts on each
  read, the latest a drain brought (3.1). SD: each write the kernel took any of; a write that took none,
  on a full queue, leaves SD dark (section 7's test, held by
  `a_write_the_full_queue_takes_nothing_of_says_so`). Replies nobody typed
  stay inside SD (Q3 unanswered; SD counts what was sent).
- **Placement (Q7).** P3, an image view in baud's container at the strip's
  right, re-placed on every resize. The pixels
  are baud's renderer's. The window crate needs nothing for it; P1,
  a trailing accessory API in the crate, remains the alternative.
  P1 replaced it once the crate had the API (5.1): in full screen, views in
  baud's container stay in the window while the title bar leaves it.
- **Look (Q7, Q9).** RD and SD letters to the left of each light. RD is AppKit's `systemGreenColor`, SD its
  `systemOrangeColor`, resolved under the window's appearance.
- **Timing (Q6).** A light is lit for 200 ms after the last byte. Once it
  changes, it does not change again for 200 ms, so any
  closed second holds at most three flashes of one light. The count holds
  per light: two lights out of phase can make twelve changes, six flashes,
  between them, so the pair passes WCAG 2.3.1 by the area clause, two 7 pt
  lights. For comparison, iTerm2's activity indicator holds 2 s after the
  last read
  ([[source: iterm2-964a75d-ptysession-isprocessing]],
  [[source: iterm2-964a75d-advanced-idle-time]]), and Konsole masks its
  activity notice for 15 s ([[source: konsole-session-a88fae0]]).
- **Accessibility (Q6).** Dim Flashing Lights draws a lit light at the
  most of its colour that makes no WCAG flash over any grey the body
  under the strip can be: the standard material's range, or black to
  white over the glass. Reduce Motion changed nothing; section 11
  replaces that.
- **The setting.** `[window] activity_lights = "on" | "off"`, on by default. It takes effect at once on reload.
- **Linux.** Nothing: the Linux window has no title bar.
- **Open.** Whether Increase Contrast re-colours the lights is
  unverified: their colours are re-read only when the style observer
  fires. The image view has a tooltip but no accessibility label, and the tooltip is a change a user sees that no decision covered;
  section 11 gives each light its own.

Decision log 1473 to 1476.

## 11. Built: seven more lights (2026-11-03)

The strip is now HS AA CD OH RD SD CS FAX SYN, in the order the 1988
Courier manual's table lists them (2.3; the V.34 and V.Everything manuals
list SYN before ARQ/FAX), with TR, MR and RS left out (rule 5 of 6.6) and
FAX in ARQ's slot. What lights each is 6.1's analogue, built as follows:

- **HS**: a reader chunk of `TTYCLSIZE`, 1024 bytes (`lights::full`), one
  compare a read. **AA**: a reply the core queued during a feed (the
  `paced` sink), or one at a flush the terminal made on its own: a focus
  report, `?2048` after a resize, `?2031`, a held DSR. Keys, clicks, the
  wheel and a paste do not light it (Q3: AA, not SD alone; SD still counts
  every write). **CS**: `Wrote::Part` or `Wrote::Nothing`. **SYN**:
  `synchronized_output()` at each display tick, after the tick's pump; a
  program that opens and closes `?2026` within one burst of reads is not
  seen. **FAX**: `images().any_placed()` at a tick, a state (Q2).
- **OH and CD** (`front.rs`, Q4): on every tick while the lights are on,
  the shell's `PROC_PIDTBSDINFO` (`e_tpgid` against `pbi_pgid`) and each
  ssh member's `PROC_PIDLISTFDS` then `PROC_PIDFDSOCKETINFO`; the members
  listed and named only when the group in front changes, when the process
  watch reports a fork, exec or exit, and once a second otherwise. Off, a
  tick makes no process read (section 7's zero-cost test, as a counting
  fake).
- **Q8**: a refused read, `EPERM`, draws the light as a ring in the letters'
  grey, neither lit nor dark; pid 1's record is refused on this Mac. Only
  OH and CD read another process, so only they can show it.
- **Q1 and Q5**: Q1 is answered in part, by its option (b): SYN and FAX
  show two of 5.4's core states, and the rest of that list (the keyboard
  protocol, mouse reporting, bracketed paste, the alternate screen, 8-bit
  controls, the encoding) has no light. Q5 stays open, with DK, CPU and
  MEM unbuilt, and so does Q2's network pair.
- **Look (Q7, Q9)**: one view a light, each with its own tooltip naming its
  letters and its meaning; colours by direction, AppKit's green for what
  comes in (RD, HS), orange for what goes out (SD, AA, CS), blue for a
  state (CD, OH, FAX, SYN); CS's letters carry a bar for its inversion. In
  a narrow window a light that would reach baud's mark is not drawn, the
  leftmost first.
- **Timing (Q6)**: every light, the states too, on the 200 ms hold and
  dwell, so each meets SC 2.3.1's count and the strip its area clause.
  **Reduce Motion** makes both two seconds: a stream of activity is one
  steady light, as blinking text is held steady. Dim Flashing Lights caps
  each lit colour as before, one scan a colour, and draws OH's and CD's
  ring faint on the dark lamp, capped so that none of a light's three
  states is a flash from another (`ring_cap`). The cursor's blink settings
  govern the insertion point only and are not read.
- **ARQ (8.1)**: unbuilt. From this build's test environment a child's TCP
  record was absent from `pcblist64` while the probe's own were present,
  and `netstat -an -p tcp`, the control, printed nothing at all; the
  environment cannot tell the system's filter from a sandbox's, so
  visibility stays **UNVERIFIED** and no light rests on it.
- **Cost**, from a timing run for this build on this Mac, two launches of
  seven rounds, medians and their ranges; an `nc` child connected to a
  listener stood in for the ssh: the shell's record 0.16-0.18 us; one
  connected member's sockets 1.06-1.08 us; listing the group 7.96-8.16 us;
  a steady tick with one connected member in front, the once-a-second
  listing included, 1.30-1.32 us. On a look change, the 27 pictures at 2x
  take 0.59-0.62 ms, and a dimmed look 3.1-5.2 ms over the standard
  material, and over the glass 13.2-13.3 ms dark and 55.0-57.1 ms light,
  most of it the three colours' caps; once a session for each look.
  Nothing is added to a byte's path: HS is one compare a read and AA one
  store a reply.

Decision log 2410 to 2417.

## 12. Built: the state lights on DEC's line, opt-in (2026-11-03)

The owner, after the research on DEC's indicator line: "i would say
default strip but config can change it". So `[window]
status_line_lights`, `"off"` by default (D4), and `"states"` builds D1 at
DEC's timing: while a program shows the indicator, CD, OH, CS, FAX and SYN
are words at columns 62, 66, 70, 73 and 77 (`Term::set_indicator_lamps`),
present while the condition holds, each whole or not at all, within the
first 80. HS, AA, RD and SD never go there; the lights keep all nine
wherever they are drawn.

- **Timing**: no hold, no dwell, nothing shown later. Each macOS display
  tick, after its pump, and each Linux loop turn, after its reads and
  writes, reads the lamps and hands them on, and the core redraws the
  line at once and at every read's end (070-14 p. 14-6). On macOS a
  change waits up to a tick to be read, a tick for the frame and its
  vsync: three ticks, within 80 ms at 37.5 Hz and above, not below. CS
  is a state here, bytes still owed, not the refusal the lights flash
  for. While the words show, OH's and CD's group is listed again 80 ms
  less three ticks after the last listing (`front::line_relist`: 30 ms at
  60 Hz, 55 at 120; every tick below 37.5 Hz, where the bound is not
  met), where the lights alone wait a second, so an ssh the process
  watch missed is drawn within the bound.
- **WCAG 2.3.1 at every size**: the words' 14 cells counted whole, at 85.3
  WCAG pixels an inch, are within a quarter of the 341 x 256 field to 37
  points of JetBrains Mono, so they pass by the area clause at any rate.
  From 38 points they are left out (`line_lamps`), which departs from
  neither text. The departure, holding them to three flashes a second and
  so up to 200 ms late, was put to Jev: not taken, 0.98 and 0.94
  (`ABOVE_LEFT_OUT`).
- **Unknown**: no DEC text gives an indicator an unknown value (searches
  for "unknown", "undefined" and "question mark", control "milliseconds"
  found). DEC reports one as DECRQM's "(0) Unknown" and DECRPSS's
  "UNKNOWN", and draws the reversed question mark only for a character
  with no graphic representation (SUB, p. 5-132). A refused read shows
  `CD?`. Jev leaned to `?` over U+2426, 0.92 and 0.80, below the bar; the
  render showed U+2426 as a missing-glyph box in the embedded font.
- **Linux** reads OH from the terminal's foreground group, the read the
  lights' own line makes (`TIOCGPGRP`, `own_line::FrontRead`), shared at
  one a frame: lit for a job, dark for the shell or for nobody. The loop
  turns on events, and a job that prints nothing makes none, so while
  the words show its wait ends 80 ms less three frames after the last
  read (30 ms at 60 Hz, 55 at 120, a 60 Hz frame below 37.5 Hz, where
  the bound is not met), at the compositor's refresh, a 60 Hz frame
  until one is known. CD has no reader there and shows `CD?`, as Jev
  leaned for both before OH was read (0.82 and 0.30, below the bar).
- **Cost**: nothing on a byte's path. The line is composed at a read's
  end behind the feed's one compare of the status type, as before; a tick
  adds one compare of five lamps, and while the words show a listing
  every 30 ms at 60 Hz, 7.96-8.16 us each (section 11), about 0.27 ms a
  second. The 37-point limit is JetBrains Mono's cell; another font's
  moves it, since the clause is read at the cell drawn.

  Instructions, counted in a release rlib without LTO, this tree against
  its base, trailing alignment padding left out: `feed_read_outlined`
  runs 8,563 against 8,562. The larger `Status` (320 bytes to 328, the
  lamps) reordered `StrBuf`, which had held it first, so its active and
  kind bytes moved from offsets 317 and 319 to 13,261 and 13,263, past
  ldrb's 4,095 immediate: each read takes a mov first, two instructions,
  one in the entry block and one after its loops, outside every loop. A
  16-byte field of the link book moved from 13,176 to 6,944, a multiple
  of 16 that ldr takes as its immediate, and a loop near the function's
  end loses a mov. `write::eight` (469),
  `write::esc_final` (1,210) and `Paint::str_run` (944) keep their
  counts. No timing ran.

Decision log 2468 and 2469.

## 13. Built: hover text over the line's words (2026-11-03)

The owner: "and can we do hover text ? so user can understand it?". The
pointer over a word on DEC's line shows `Led::tip`, the strip's tooltip
text (CD's and OH's now name `CD?` and `OH?` too), on macOS and Linux.

- **Drawn in the frame on both systems** (`baud_render::tip`): the text
  wrapped at 60 cells into lines of the selection's colours, right-aligned
  and ending a row above the line. Wayland has no tooltip protocol;
  `xdg_popup` "can be used to implement ... tooltips" (wp-xdg-shell) but
  is a second surface `baud-present-wayland` does not make. Drawn as
  fields, as the link preview's bar is, the pixels are the renderer's on
  every path: the instance and cell arms agree (`cells.rs`) and the CPU
  frame, the Linux driver's, is pinned (`hover_tip.rs`). The strip keeps
  AppKit's tooltips. Jev leaned to drawing on both, 0.94 and 0.86, below
  the bar.
- **WCAG 2.2's 1.4.13** governs a tip baud draws: hoverable, its area
  takes in the gap row so the pointer crosses from the word onto it;
  persistent, no timer, and it goes when its word leaves the line;
  dismissible, any key hides it and still reaches the program (Jev's lean,
  0.58 and 0.70, below the bar), since Escape is the program's.
- **Hit-testing** counts the line's row (`line_tip::over_at`,
  `Seat::line_over`), where a press or a report is clamped to the grid's
  last row; `Term::indicator_lamp_at` names the word under a column.
- **Screen readers**: the strip's lights now carry their letters as the
  accessibility label and the tooltip text as the help. baud exposes no
  accessibility element for the grid, so DEC's line, its words and the tip
  reach no screen reader, and no keyboard focus reaches them: not fixed.
- **No flash from a word that comes and goes**: a tip its word's leaving
  took stays down until the pointer moves, and a key's dismissal holds
  while it stays (`LineTip::refresh`). OH in a loop of short commands, or
  SYN at a frame's pace, had taken the box down and put it back with every
  change under a still pointer; held over OH flipping each frame for 60
  frames, the box now stays down after the first change, where it came
  back 29 times.
- **The lights' own line, on Linux**: the pointer over any of the nine
  lights there, HS, AA, RD and SD among them, shows its `Led::tip` by the
  same rules, drawn the same way. The light is hit where the last frame
  placed it (`line_tip::LightsLine`, `Renderer::lights_places`), at the
  seat's point times the scale, the frame being drawn from the buffer's
  corner (`Seat::frame_point`). The box sits above a line at the bottom,
  and opens down under one at the top (`tip_look_opening`). It holds from
  the light to the box over the notice, DEC's line and the gutter, in the
  box's columns and the light's; on the line itself only over the light,
  as on DEC's line only over the word; and it goes when a narrow window
  leaves the light out. Its CPU frame is pinned (`hover_tip.rs`). On macOS
  that line keeps AppKit's tooltips. CD's text now says that on Linux
  its light stays dark and DEC's line shows `CD?`.
- **Linux** was type-checked and linted for `aarch64-unknown-linux-gnu`;
  its pointer, key and turn wiring is read off `linux.rs` by a test. No
  headless compositor ran: the docker engine was down, and starting colima
  writes `~/.config`.

## 14. Built: the title strip's colours read against it

WCAG 2.2's SC 1.4.11 asks "a contrast ratio of at least 3:1 against
adjacent color(s)" for the "Visual information required to identify ...
states" ([[source: wcag22]]). AppKit's colours, drawn as they came, did not
meet it on the standard material: against grey 196, the light half's
lightest, green read 1.27, orange 1.32 and blue 2.02, and Increase
Contrast's 2.60-2.62; against the dark half's grey 92, blue read 2.07 and
orange 2.995. The letters, 8 pt bold, read 3.26 dark and 3.48 light, under
SC 1.4.3's 4.5:1 for text that is not large (18 pt, or 14 pt bold); the
HIG's table, which it says is "from WCAG Level AA", gives 3:1 for "bold at
any size" ([[source: apple-hig-accessibility]]), and the WCAG text governs.

- **Lit** (`lights::strip_colours`): each of AppKit's colours lifted with
  `reads_at`, the lights' line's own lift, to 3:1 against every grey of the
  half, keeping its hue. Light: #1C7C34, #A45815, #006BCC. Light with
  Increase Contrast: #007D2D, #B54B00, #1A65E1. Dark: green and Increase
  Contrast's three kept; orange #FF9332, blue #7DAFFF. The letters are
  AppKit's made opaque over the strip's grey and lifted to 4.5:1: #515151
  light, #353535 light with Increase Contrast, #D4D4D4 dark.
- **Dark lamps**: the system colour, unlifted, mixed into the grey as
  before, byte for byte (`LightsLook::new` takes the lamp's colour apart
  from the lit one), so a lit light now differs from its dark one by
  lightness: 1.35, 1.40 and 1.88 light become 3.22, 3.20 and 2.81. The
  lights' line passes one colour for both, and its pixels are pinned in
  every state, both themes and both glows.
- **Glass**: colours, letters and the grey all come from the window's
  `effectiveAppearance`, the traffic lights' and the lockup's; the grey came
  from the material's, so a Dark window over ClearLight glass mixed dark
  colours into the light grey. Nothing is lifted over the glass: no colour
  reads at 3:1 against black to white, and the strip's range there, the
  page dimmed over the glass, is unmeasured. Open.
- **Increase Contrast** draws the pictures again when the tick reads it
  changed (`poll_display`). That a window's `effectiveAppearance` is the
  accessibility appearance while it is on is unverified; the tests take
  those colours from AppKit's own class methods, headless.
- **Dim Flashing Lights**: the caps are computed from the lifted colours.
  Light falls from 119, 107 and 69 to 40, 40 and 48, light with Increase
  Contrast from 60, 59, 59 to 53, dark blue from 154 to 102; the rings rise
  (light 23 to 39, dark 55 to 74).
- **A correction**: light with Increase Contrast's orange, #C55300, is a
  saturated red by the 0.80 test, R/(R+G+B) 0.866 on linear values, and
  lifted, #B54B00, 0.868; the other three oranges read 0.69-0.78. The
  drawing conforms through the clock, at most three flashes
  a second, and the area clause, and the dimmed caps check the red rule;
  the flash test now checks red flashes, not red pixels, for that look.
- **Tests**: 3:1 and 4.5:1 for all four appearances (`activity_lights.rs`,
  and AppKit's own colours headless in `baud-present-macos`), the window's
  appearance over the glass and the redraw on a change of Increase Contrast (`baud-app`, a check of the driver's source that `poll_display` marks the pictures stale), each red on the old behaviour; the dark lamps and
  the line's pixels pinned from the old drawing, red with the lamp mixed
  from the lifted colour, the line lifted to 5:1 or its lamps mixed from
  the letters' colour. Nothing on a byte's path changed.

## 15. Built: the lights' line drawn as lifted, against one backdrop

Read from the drawn pixels headless (the CPU path at 2x, the app's
faces, each frame over every grey as the presenter composites it), the
line missed its floors by a level: dark on Linux's glass, FAX and SYN's
lamps 2.97 and the letters 4.4999; light reversed, FAX 2.99.

- **Coverage rounds** (`Canvas::coverage`). It truncated `255 * y`, and
  the prefix sum lands at 0.9999999 inside a shape whose edges fall
  between pixels, so a wholly covered pixel was 254 -- 99 of the FAX
  dot's 126 at 2x, the letters' stems -- and a colour lifted to 3:1 or
  4.5:1 exactly drew just under it. Rounded, every inside is 255, and
  the masks moved toward FreeType's: 63% to 68% of pixels identical.
- **One backdrop** (`LookKey`, `place_lights`). The line was lifted
  against `theme.backdrop`, the standard material's greys on macOS and
  black to white on Linux, so one theme drew two lines (dark state
  lights #3771FF and #8DA2FF, letters #959595 and #CDCDCD). The key has
  no backdrop now: the line is lifted against `Backdrop::any` on both,
  and macOS draws Linux's colours. The cell path's `lift_on_glass` still
  reads the window's text against the material on macOS, as the Linux
  driver says ("Only the backdrop differs").

Drawn, at 2x, over grey 0 to 255, opaque and at the shipped dims, both
videos: the worst lamp reads 3.009 and the worst letters 4.51, but for
the light theme reversed on the glass, whose letters are white already
and read 4.12; over macOS's material that arm read 4.49 before and reads
4.61 now.

- **Tests, each red first**: in `baud-font/tests/raster.rs`, a wholly
  covered pixel is 255 (39 disc pixels read 254); in
  `baud-render/tests/lights_line.rs`, the line is the same bytes on
  macOS and Linux (24 of 48 lines differed), and its drawn lamps and
  letters read 3:1 and 4.5:1, red on truncation alone at the probe's
  2.9744, 4.4996 and 2.9912, and red on the old backdrop alone.
- **Re-pinned**, by rounding alone: the ab_glyph oracle's quantisation;
  the box masks, 240,665 bytes each by one; seven of `golden.rs`'s nine
  recordings, its light git-log, `system.rs`'s cat-cjk, the Hindi line,
  two hover-tip frames and the line's two pins. Channels moved by up to
  3, the text-contrast curve stepping up to 4 codes a level; with it
  held flat, every changed channel of the nine moves by one.
- **Open**: the light theme reversed at its shipped dim on the glass,
  whose page runs from #171717 to #7D7D7D, has no letter colour that
  reaches 4.5:1, so the letters are drawn white and read 4.12 (SC 1.4.3);
  at 1x no letter pixel is wholly covered; the lift keeps no margin for
  the band's own eight bits; Dim Flashing Lights and DEC's status-line
  words are their own questions.

## 16. Built: every lamp a palette entry, on a plate, in three sets

Sections 14 and 15 lifted each lit colour until it read; this replaces
both. **Every lamp, lit and unlit and an unread light's ring, is one of
`Theme::palette[0..16]` exactly**, never lifted or mixed, in the title
strip and on the line alike (`LightsLook::of`).

- **The sets**, `[window] activity_lights_colours`, tables compiled in
  (`LightsColours::entries`), by the half the background is against the
  foreground: `"meaning"`, the default, dark HS 10, AA 11, CD 4, OH 6,
  RD 2, SD 3, CS 1, FAX 5, SYN 13, light the same but AA 6, OH 12, SD 14;
  `"iec"`, IEC 60073:2002's Tables 7 and 8 (not in `sources/`), HS and CD
  2, CS 3, FAX and SYN 6, the rest 7; `"modem"`, every lamp 1.
- **The plate**: a rounded rectangle of the theme's background, opaque,
  4 pt past the end lights, 20 pt tall in the strip (radius 5), the line
  less a point top and bottom (radius 4). Letters are the foreground,
  lifted only under 4.5:1.
- **Unlit**: a dot half the lit one's diameter (SC 1.4.1). In the meaning
  set entry 8; in iec and modem 8 or 0, whichever is visible on the plate
  and no louder than the quietest lit lamp, the more visible; else the
  quieter visible one.
- **The glow**: a halo of the lamp's colour from 1 pt off the dot to 3 pt,
  `(1 - t)^1.6`, alpha 0.8 x `activity_lights_glow` (0..1, 0.4 default).
  Dim Flashing Lights draws none and leaves the dot its entry.

**Measured** (2x, the built code; `activity_lights.rs`, `lights_line.rs`):

| theme | set | unlit | worst lit vs plate | worst lit vs letters |
|---|---|---|---|---|
| dark | meaning | 8, 4.75:1 | CD 4.50 | HS 1.04 |
| dark | iec | 8, 4.75:1 | HS 8.82 | AA 1.13 |
| dark | modem | 0, 1.10:1 | 4.50 | 2.97 |
| light | meaning | 8, 6.09:1 | OH 6.13 | FAX 1.33 |
| light | iec | 8, 6.09:1 | AA 4.57 | HS 2.12 |
| light | modem | 8, 6.09:1 | 6.97 | 1.86 |

Letters 13.35:1 dark, 12.99:1 light. Every full-coverage lamp pixel of
the strip and the line's CPU frame is its entry, with moved-mask and
mixed-colour controls; all nine changing at once flash 1,422 WCAG pixels
at most, 6.5% of 21,824. On 234 base16 schemes (data, not a gate) the
meaning set has all nine at 3:1 in 130 of 168 dark and 31 of 66 light.
**Open**: the dark modem set's unlit lamp is entry 0, 1.10:1 on #101010,
which the rule gives because entry 8 is louder than its red.

## 17. Built: CS in the modem's sense, lit while the line is clear

6.1 took CS inverted, lit when the kernel refused a write, because the
modem's polarity "would always be on". It is the modem's polarity now:
the Courier's CS is "ON until the modem lowers CTS" (2.3), so baud's is
lit while the program takes what baud sends and dark while bytes are owed
it that the kernel has not taken. Being lit nearly all of a session is
what a modem's CS was, and dark says the line closed.

- **The clock** (`Activity`): each lamp has a state at rest and one while
  its thing is seen (`Led::at_rest`, `Led::when_seen`), dark and lit for
  eight, lit and dark for CS, and the hold and dwell work on those. What
  CS's clock is told of is the line closing: a write the kernel refused
  any of (`Wrote::Part`, `Wrote::Nothing`) and, at each macOS tick and
  each sampled Linux turn, an outbox not yet empty. So CS is lit from the
  first frame, dark from the step after bytes were owed until the hold
  after the last tick that found them owed, and at rest no lamp owes a
  change: a lit CS wakes nothing (`next_change` none).
- **DEC's line** shows the word CS while the outbox is empty, the same
  state, and the hover text over both says the same.
- **The bar is gone**, with nothing inverted to mark. The line's picture
  keeps its 11 points, from the dot and the 2 points of halo the line
  draws (`LINE_HALO`), which the bar's height had set before.
- **Colours**: in `"iec"` CS is green 2, IEC 60073's normal condition,
  where its yellow 3 had said abnormal; in `"meaning"` green 2 on both
  halves, red's reason (the one light that says something went wrong)
  having gone, and green the colour of a clear line; `"modem"` stays red.
  On the built-in plates green reads 8.82:1 dark and 6.13:1 light, and
  the iec set's quietest lit lamp, and so its unlit entry, is unchanged.
  **Open**: the meaning set's entry is a lean: green, the out kind's hue (yellow 3
  dark, cyan 14 light, each SD's), a state's hue (12 dark, 4 light) and a
  neutral lamp (7 dark, 0 light) were weighed, and none was clearly the
  better.
- **The README session** lights all nine; its harness writes until the
  kernel has taken every byte, so CS goes out there only for a write the
  kernel took part of at once (`Under::shorts`), and the run prints when.
- **Tests**: red first, `cs_is_lit_while_the_line_is_clear` (CS lit at
  rest, nothing to wake for, dark while owed and for the hold after, SD
  the other way round as its control), the flash count with CS's sense
  reversed, the sets' tables and no ink above any light's caps; red under
  the old behaviour put back, the Linux line's stamps, both drivers'
  outbox feeds, DEC's word and the session's lights. Re-pinned: the line's twelve
  digests and the light's tip frame, by the bar's pixels and, lit, CS's
  colour.
