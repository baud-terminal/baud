# Security Policy

baud is a terminal emulator that runs on macOS, with a Linux port that has run
only headless. Its core parses **wholly untrusted input**: the byte stream from
a pty is whatever the program on the far end sent, and over an ssh session
that program may be hostile. Everything below follows from that.

## What baud does on your machine

`baud-app`, the binary `baud`, starts your `$SHELL` as a login shell on a pty
(`forkpty` and `execve`), with the variables other terminals set removed --
in your home directory when baud's parent is launchd (`open`, and so,
inferred, the Dock and Finder), where baud was started otherwise. It reads
its configuration (`$BAUD_CONFIG`, then `$XDG_CONFIG_HOME/baud/`, then
`~/.config/baud/`; no entry of its own under the directory it starts in,
though a relative `$BAUD_CONFIG` is read against it), maps font files
read-only -- the
ones the config names, or the system faces behind its embedded font when it
names none -- and draws with Metal
on macOS and on the CPU into `wl_shm` on Linux. On macOS it copies a selection
to the clipboard and pastes from it when you ask, puts a program's `OSC 52` copy on it unless `[clipboard] osc52 = "off"`, and opens the links below.
It has no network code: the Linux driver talks only to the local Wayland
compositor's Unix socket.

## The security-relevant surface

**Untrusted input is the entire input.** There is no trusted mode. The parser
must terminate, stay in bounds, and never execute a payload, for any byte
sequence at all — which is why `the_screen_does_not_depend_on_where_the_reads_fell` and the
chunk-invariance properties exist.

**Five crates forbid `unsafe` outright.** `baud-core`, `baud-domain`,
`baud-font`, `baud-render` and `baud-xkb` carry `#![forbid(unsafe_code)]`
as a crate attribute, so the parser, the grid, the font stack (its PNG
decoder included), the compositor's conversion and the keymap compiler cannot
contain an `unsafe` block at all -- `forbid` cannot be relaxed by an `allow`
further down.

**Three more deny it at the root and name the single module that may.**
`baud-present-macos` denies and allows in `metal.rs`; `baud-wayland` and
`baud-present-wayland` deny and allow in their `sys.rs`. There the
compiler holds the boundary: a second module that wanted `unsafe` would
have to say so in the diff.

**What is left carries no such attribute, and this is where to look.**
`baud-app` is a binary whose `app.rs` and `link.rs` forbid for themselves,
with the platform calls in `pty.rs` and `sys.rs` -- the pty, the read-only
mapping, `poll`, and the `unsafe extern "C"` blocks that declare them.
`baud-isa`, `baud-isa-aarch64` and `baud-isa-x86-64` are the vector and
cache-probe crates, where `unsafe` is the subject rather than an exception:
`Scan`'s methods are `unsafe fn` because on x86-64 a caller can invoke AVX2 on
a machine that cannot execute it. `baud-bench` has two, in harnesses that are
not in the shipped binary, and `baud-sfte-difftest` calls a C terminal it
compiles from a pinned header, also outside the shipped binary. Seven crates
carry no attribute in all -- those six and `baud-difftest`, which contains no
`unsafe` at all -- so the confinement there is a convention this paragraph
asserts and nothing enforces.

**Attacker-controlled data is stored in bounded tables, and each says what it
does when full.** Every one is fixed-size and filled by what the remote end
sends; an unbounded table keyed on attacker input is the denial-of-service
shape this design exists to avoid.

* An OSC payload past `OSC_MAX` is truncated, and counted in
  `string_truncations`.
* An `OSC 8` hyperlink URI goes in a table of `NLINK` slots of at most
  `LINK_MAX` bytes, which **refuses** a colliding link rather than evicting
  one, because an evicted link would retarget cells already on screen. A
  refusal is counted in `links_refused`.
* The rendition table (at least `NSTYLE` slots, sized to the screen) evicts only once
  every slot is filled, and a cell still naming the evicted slot is recoloured
  -- an open bug, a wrong colour and never an out-of-bounds read.
* A Sixel image is decoded into caller memory with no external decoder,
  as its bytes arrive, bounded by a pixel arena (`[images] pixels_mb` in the
  config), the 65535 pixels a coordinate holds and a slot table; no copy of
  the bytes is kept, so their length is no limit. A refused image is counted
  by the limit it hit, and the Primary DA claims Sixel only when `[images]
  advertise_sixel` is set.

**Control strings are consumed, never executed.** DCS, SOS, PM and APC payloads
are read to their terminator and discarded unless a handler reads them -- a
DECRQSS, a Sixel. This was once a real bug: those introducers fell through to
ground, so a stream could hide `CSI 2 J` inside an APC string and clear the
screen. Note the precise guarantee — a bare `ESC` *cancels* a control string,
so what is promised is that a payload containing no `ESC` is inert.

**A hyperlink opens only what is on a short list.** Cmd-click on an `OSC 8`
link opens `http`, `https` and `mailto` with the system's handler, reveals a
`file` link on this host in Finder, and refuses everything else, because every
other registered scheme is an application the opener would launch on output an
attacker controls (`baud-app/src/link.rs`). A URI with a byte outside
printable ASCII is refused, and a `mailto` may carry only `subject` and
`body`, so `bcc=` cannot add a hidden recipient. An `https` link reaches
whatever LaunchServices routes it to, universal-link applications included:
an accepted risk rather than a closed one.

## Deliberate refusals

These look like missing features and are security decisions. Please do not
report them as gaps; if you think one is wrong, the reasoning is in
[docs/protocol-decisions.md](docs/protocol-decisions.md).

- **`OSC 52` clipboard *reads* get no reply.** A hostile program could otherwise
  read back whatever the user last copied, which may be a password. Any answer —
  including an empty one — tells it the feature works, so the refusal is
  silence, counted in `clipboard_reads_refused`.
- **A screen checksum request (DECRQCRA) is answered by default**, and `[screen] checksum = "off"` turns it off.
  A program can ask for the checksum of one cell at a time and so read back
  what is on the screen, other programs' output included; off, every request
  gets no reply and is counted in `checksums_refused`, and no sequence, a
  reset included, turns it back on.
- **A program can move, resize, minimise, raise, lower, maximise and
  full-screen the window by default** (xterm's window operations, `CSI 1 t`
  to `CSI 10 t`, and DECSLPP), and `[window] operations = "off"` turns them off. Anything printed to the
  terminal -- a remote program, a file you `cat` -- can otherwise move the
  window off the screen, shrink it, hide it in the Dock or put it behind
  others; off, every request is refused and counted in
  `Term::window_ops_refused`, and no sequence, a reset included, turns them
  back on. A program cannot read the title back either way (`CSI 20 t` and
  `CSI 21 t` get no reply). A resize is no larger than the screen; with no
  screen size known, one in pixels is refused and one in cells is held to
  the largest grid.
- **`OSC 52` clipboard *writes* are honoured by default** (V11, `docs/research/notes/v-open-decisions.md`), and `[clipboard] osc52 = "off"`
  turns them off. The threat is the write itself: anything printed to the
  terminal -- a remote program, a file you `cat` -- can replace your clipboard
  without asking, and you might paste that into a shell. Bracketed paste
  blunts the worst of it in a modern shell; baud's paste does not strip other
  control bytes. Every accepted write is counted in the five-second report,
  which the log shows at `BAUD_LOG=info` (the default, `warn`, shows only refusals), and a copy is never cut: one past `[clipboard] body_kb` is refused
  whole and logged. There is no focus gate until one is tested over a slow
  ssh link. A program still never reads the clipboard back.
- **Bracketed paste (`?2004`) is implemented as a security mode.** Without it a
  pasted string containing a newline is delivered to the shell as typed input
  and runs. That is the paste-jacking class.
- **`?1001` highlight tracking cannot hang baud.** `[[source: ctlseqs]]` warns
  that it "requires a cooperating program, else xterm will hang": xterm ignores
  every event until the program answers. baud reports the press, takes the
  program's answer when it arrives, and waits on nothing -- `baud-core` has no
  event loop to block: an event before the answer is handled as normal
  tracking handles it, a release before it is a normal release, and an answer
  no press is owed changes nothing. A release report with a position the
  encoding cannot name is refused whole and counted, never written naming a
  different cell.
- **Queries are never answered with the wrong shape.** An application parses
  whatever arrives as the report it asked for, so a wrong answer is worse than
  silence — DA under-claims as a VT102, and an unimplemented DSR gets no reply.

## What is worth reporting

- A byte sequence that panics, hangs, or reads or writes out of bounds.
- Any path where a control-string payload is executed rather than consumed.
- Any unbounded growth driven by remote input.
- Any query answered with a report of the wrong shape, or a mode reported as
  supported that is not acted on.
- A link a program prints that baud opens or launches outside the list above.
- `RUSTSEC` advisories in the dependency tree. `zerocopy` is the only
  dependency of `baud-core` and `baud-domain`; the macOS app adds the `objc2`
  family, `block2`, `dispatch2` and `macos-liquid-glass`, and through them
  `libc` and `bitflags`. `devenv tasks run test:advisories` checks every
  lockfile in the repository against the RustSec database with `cargo audit`.
  It needs the network, so it is run by hand rather than on every push.

## Reporting a Vulnerability

Please report security issues privately rather than opening a public issue —
[GitHub Security Advisories](https://github.com/i-am-logger/baud/security/advisories/new)
if private vulnerability reporting is enabled on the repo, otherwise contact
the maintainer directly via GitHub ([@i-am-logger](https://github.com/i-am-logger)).

Include the affected version or commit, a minimal reproduction, and the impact
as you see it. There is no bug bounty — this is a solo-maintained open-source
project — but reports are taken seriously and credited unless you ask
otherwise.

## Supported Versions

No release has been published. Report against the commit you built. There is
no LTS branch.
