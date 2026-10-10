# Security Policy

baud is a `no_std` library that parses **wholly untrusted input**: the byte
stream from a PTY is whatever the program on the far end sent, and over an ssh
session that program may be hostile. Everything below follows from that.

## The security-relevant surface

**Untrusted input is the entire input.** There is no trusted mode. The parser
must terminate, stay in bounds, and never execute a payload, for any byte
sequence at all — which is why `the_screen_does_not_depend_on_where_the_reads_fell` and the
chunk-invariance properties exist.

**Five crates forbid `unsafe` outright.** `baud-core`, `baud-domain`,
`baud-font`, `baud-render` and `baud-xkb` carry `#![forbid(unsafe_code)]`
as a crate attribute, so the parser, the grid, the font stack, the
compositor's conversion and the keymap compiler cannot contain an
`unsafe` block at all -- `forbid` cannot be relaxed by an `allow` further
down.

**Three more deny it at the root and name the single module that may.**
`baud-present-macos` denies and allows in `metal.rs`; `baud-wayland` and
`baud-present-wayland` deny and allow in their `sys.rs`. There the
compiler holds the boundary: a second module that wanted `unsafe` would
have to say so in the diff.

**What is left carries no such attribute, and this is where to look.**
`baud-app` is a binary whose `app.rs` forbids for itself, with the
platform calls confined to `pty.rs` and `sys.rs` -- the pty, the
read-only mapping, `poll`, and two `unsafe extern "C"` blocks holding ten
declarations between them. `baud-isa`, `baud-isa-aarch64` and
`baud-isa-x86-64` are the vector and cache-probe crates, where `unsafe`
is the subject rather than an exception: `Scan`'s methods are `unsafe fn`
because on x86-64 a caller can invoke AVX2 on a machine that cannot
execute it. `baud-bench` has two, in harnesses that are not in the
shipped binary. Six crates carry no attribute in all -- those five and
`baud-difftest`, which contains no `unsafe` at all -- so the confinement
there is a convention this paragraph asserts and nothing enforces.

**This paragraph previously said `unsafe` appeared in exactly one place,
`scan_avx2` in an avx2.rs under the baud-isa crate** -- named without
backticks on purpose, because the gate below reads a backticked path as a
citation and cannot tell one from an explanation of a dead one. That file
does not exist, the function went with `baud-core2`, and the claim was
false besides. It survived because
`every_repository_path_named_in_the_docs_exists` did not list
`baud-isa/` among its roots, so neither the dead path nor the claim
resting on it could be checked. The root is listed now.

**Attacker-controlled data is stored in bounded, evicting tables.** OSC payloads
(`OSC_MAX`), hyperlink URIs (`NLINK` × `LINK_MAX`) and style interning
(`NSTYLE`) are all keyed on content the remote end chooses. Each is fixed-size
and evicts on collision. An unbounded table keyed on attacker input is the
denial-of-service shape this design exists to avoid; overflow is *counted*, not
silent.

**Control strings are consumed, never executed.** DCS, SOS, PM and APC payloads
are read to their terminator and discarded. This was once a real bug: those
introducers fell through to ground, so a stream could hide `CSI 2 J` inside an
APC string and clear the screen. Note the precise guarantee — a bare `ESC`
*cancels* a control string, so what is promised is that a payload containing no
`ESC` is inert.

## Deliberate refusals

These look like missing features and are security decisions. Please do not
report them as gaps; if you think one is wrong, the reasoning is in
[docs/protocol-decisions.md](docs/protocol-decisions.md).

- **`OSC 52` clipboard *reads* get no reply.** A hostile program could otherwise
  read back whatever the user last copied, which may be a password. Any answer —
  including an empty one — tells it the feature works, so the refusal is
  silence.
- **Bracketed paste (`?2004`) is implemented as a security mode.** Without it a
  pasted string containing a newline is delivered to the shell as typed input
  and runs. That is the paste-jacking class.
- **`?1001` highlight tracking cannot hang baud.** `[[source: ctlseqs]]` warns
  that it "requires a cooperating program, else xterm will hang": xterm ignores
  every event until the program answers. baud records the mode and the region
  the program sends, answers DECRQM from the flag, and waits on nothing --
  `baud-core` has no event loop to block. Drawing the region and sending the
  release reports are unbuilt (note V's V12,
  `docs/research/notes/v-open-decisions.md`). This said `?1001` was not
  implemented until 2026-10-10.
- **Queries are never answered with the wrong shape.** An application parses
  whatever arrives as the report it asked for, so a wrong answer is worse than
  silence — DA under-claims as a VT102, and an unimplemented DSR gets no reply.

## What is worth reporting

- A byte sequence that panics, hangs, or reads or writes out of bounds.
- Any path where a control-string payload is executed rather than consumed.
- Any unbounded growth driven by remote input.
- Any query answered with a report of the wrong shape, or a mode reported as
  supported that is not acted on.
- `RUSTSEC` advisories in the dependency tree (`zerocopy` is the only runtime
  dependency).

## Out of scope

There is no network code, no filesystem access, no process execution and no
renderer. A "vulnerability" in rendering or input handling is out of scope
because neither exists yet.

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

Only the latest published release is supported. There is no LTS branch.
