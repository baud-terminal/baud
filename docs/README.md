# Documentation

Every document in the repository, grouped by who it is for. Start with the
first group if you want to use baud, and skip to the others when you need
them.

## Use baud

| | read it for |
|---|---|
| [guide/features.md](guide/features.md) | everything baud does for you, by area, each line backed by a test |
| [roadmap.md](roadmap.md) | what is being built, what is planned, and what has shipped |
| [guide/install.md](guide/install.md) | building baud and getting it running |
| [guide/configuration.md](guide/configuration.md) | the config file: where it goes, every key, its default and what it changes |
| [guide/starship.md](guide/starship.md) | baud's prompt for starship: the folder and an amber ❯, a light palette, and what to add |
| [guide/status.md](guide/status.md) | what works today and what is not built yet |
| [guide/performance.md](guide/performance.md) | how fast baud is, what was measured, and what was not |
| [cursor.md](cursor.md) | the cursor trails, a still of each; [cursor.html](cursor.html) plays them side by side from a checkout |
| [guide/licensing.md](guide/licensing.md) | what the licence allows, a commercial licence, and sponsoring |
| [../config.toml.example](../config.toml.example) | the config file baud writes when it finds none, every key commented out at its default and explained |
| [../SECURITY.md](../SECURITY.md) | what baud trusts, what it refuses, and how to report a vulnerability |

## Embed the core

| | read it for |
|---|---|
| [embedding.md](embedding.md) | using `baud-core` in your own program, with a compiled example |

## Contribute

| | read it for |
|---|---|
| [../CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md) | how contributors treat each other |

## Reference

| | read it for |
|---|---|
| [vt-coverage.md](vt-coverage.md) | whether a specific control sequence works: one row per operation, against its primary source |
| [input-coverage.md](input-coverage.md) | what baud sends for each key, modifier and input mode |
| [protocols/](protocols/) | one document per level (VT52, VT100, VT220, VT420, xterm), mirroring the code and tests, and a page per `$TERM` profile |
| [bidi.md](bidi.md) | right-to-left text: lines drawn in their visual order from the start, what is decided, what is open and what is next |
| [protocol-decisions.md](protocol-decisions.md) | whether something that looks like a gap is deliberate, and the open departures from the standards |
| [../MEASUREMENTS.md](../MEASUREMENTS.md) | every number, stamped with the host and commit that produced it |
| [../THIRD_PARTY.md](../THIRD_PARTY.md) | every third-party file, with its licence and source |

## History and research

| | read it for |
|---|---|
| [../MEASUREMENTS.md](../MEASUREMENTS.md) | every measured number, with the commit, host and payload it was taken on |
| [the sources below](#research) | the primary sources every protocol claim cites, by group |

## Research

`research/sources/` holds the primary sources every protocol claim cites:
xterm's `ctlseqs`, Paul Flo Williams' DEC ANSI parser, ECMA-48, the VT510
programmer reference, and the papers behind the SIMD and FSM work. Each is
recorded in [`MANIFEST.toml`](research/sources/MANIFEST.toml) with a SHA-256,
and `scripts/check-sources.sh` re-hashes them, so a citation to a key the
manifest does not carry — or one recorded as `gated`, meaning never obtained —
fails the build.

The manifest files each source under a group. Its header comment names the
groups' topics, and two groups it has no line for are described here from
their entries' titles:

| group | sources on |
|---|---|
| A | SIMD parsing and vectorised classification |
| B | data-parallel finite-state machines |
| C | the terminal parser as specified |
| D | memory hierarchy and write amplification |
| E | the tty layer and syscall batching |
| F | text sequence data structures |
| G | text encoding, width and grapheme clustering |
| H | terminal input: key encoding and the extensions to it |
| I | instruction-set dispatch and how per-ISA code is organised |
| J | rendering: font formats, shaping, rasterisation, compositing, memory placement, presentation |
| L | the scrollbar, and the window over the history it indicates |
| O | keystroke to pixel: the clocks on the path, and how typing latency has been measured |
| P | the shader cache at launch: binary archives, precompiled libraries, the Metal toolchain |
| Q | Mosh's local echo prediction: its paper and its prediction engine (no line in the header) |
| R | sfte: a second external implementation, as an oracle and a feature catalogue |
| U | who holds the window over the history across a resize: the re-cut, the viewport, the anchors |
| V | cursor trails and cursors drawn by programs: kitty, Neovide and helix (no line in the header) |
| Y | palettes: light/dark pairs, scheme formats, colour-scheme notifications, contrast floors |
| AA | the shell a launched terminal starts: its locale, working directory and terminal settings |
| AH | title-bar activity lights: the tty LED trigger, modem panels, per-process counters |
| AI | where a launched terminal's log lines go: unified logging, files, the journal |
| AJ | a launch splash: launch guidance, flashing, splash prior art |
| AK | the window title: OSC titles, derived facts, platform conventions |
| AL | the cursor trails' flashes: WCAG 2.3.1, broadcast flash rules, OS settings, trail prior art |
| AM | XTGETTCAP: what a terminal reports about its own capabilities |
| AN | the build's own tooling: cargo's publish field, release-plz, the dev shell's treefmt wrapper |
| AO | the rendition table: how terminals store a cell's colours, and how a table of renditions frees its slots |
| AS | the glass's legibility: Apple's materials, contrast, Reduce Transparency and Increase Contrast, and Terminal's own profiles |
| AT | a standard material behind the window body: Apple's NSVisualEffectView materials, the shipped material catalogs, Apple's own text apps, and the modelled renders |
| AW | the scope calls: printing on macOS, and the alert sound at a volume |
| AX | notifications and progress from programs: `OSC 9`, `777`, `99`, `9;4` and `1337`, and where each platform shows them |
| AZ | a window in the background: the unfocused cursor, the inactive selection, theme cursor colours |
| BE | the Linux app under a pointer: the virtual pointer and keyboard protocols a headless sway takes, and what the app did there; and what a Linux frame copies and damages on two sways |
| BG | where baud-font's code came from: HarfBuzz 10.2.0's and 12.3.0's hb-set-digest.hh and the ttf-parser 0.25.1 files baud-font's code is read against |
| BL | the bell: what a `BEL` does in a macOS window -- the alert sound, an attention request, a raise (`baud-app/src/bell.rs` cites them) |
| BU | bare URIs in text: what a URI is, where one ends in prose, and how terminals find one under a click (`baud-app/src/link.rs` cites them) |
| CR | the config read again while baud runs: file-change notification, atomic saves, and how terminals reload (`baud-app/src/reload.rs` cites them) |
| WN | more than one window: a new window, a window's close, the last window closed, and the Dock's reopen (`baud-app/src/windows.rs` cites them) |
| GP | the brand pictures: GitHub's picture limits, the five-unit code the link picture's tape spells, the tape's hole sizes, and the font release the letters are traced from (`tools/baud-pictures` and `assets/logo/README.md` cite them) |
| SP | the shell prompt baud ships: starship 1.27.0's configuration schema, which `assets/starship/baud.toml` is checked against (`baud-app/tests/starship_preset.rs` cites it) |
| FM | the tour film's pieces: the wallpaper photograph's licence, CC0, with CC0's legal code, and the licences of the Noto emoji, CJK and other fonts the demo fetches to draw its scenes (`assets/demo/LICENSES.md` cites them) |

The table is held to the manifest by `every_source_group_is_in_the_index`
(`baud-core/tests/docs.rs`), which fails when a group has no row.

## The rule these documents follow

A claim is either mechanically checked or it is not made.

- `vt-coverage.md`'s **done** rows are checked by
  `baud-core/tests/coverage.rs`.
- The embedding guide's Rust example is `baud-core/examples/quickstart.rs`,
  compiled by the gate, with `baud-core/tests/embedding.rs` asserting they
  have not drifted.
- Every source citation is checked by `scripts/check-sources.sh`, which
  re-hashes the file behind each key.
- Type-system claims are checked by `audit_all.sh`.

This rule is written down because it was broken: `vt-coverage.md` claimed for
most of the project's life to be verified by a `tests/coverage.rs` that did not
exist, in the same paragraph where it warned against documentation claiming
coverage it does not have.

## How these documents are kept true

Nothing here is trusted because someone read it carefully. These checks fail
the build instead:

| check | what it holds |
|---|---|
| `baud-core/tests/coverage.rs` | every **done** row names an operation some test mentions; no row is `parsed` or `todo`, bold or bare, every `partial` row names its gap, and the rows at `partial` equal the counts `vt-coverage.md` and `input-coverage.md` state; **every `out` row states a reason**; the operation counts quoted in README and MEASUREMENTS.md match the table |
| `baud-core/tests/conformance/enumerated.rs` | every **done** row's sequence, synthesised from the table and fed to a real `Term`, reaches a handler rather than being counted unsupported |
| `baud-core/tests/conformance/spec_inventory.rs` | the reverse direction: every plain CSI final, every CSI *form* (markers and intermediates), every `ESC Fe`, **every `ESC` form with an intermediate — including all four G-slots × every charset final**, every `OSC Ps` and every `DCS` form that **`[[source: ctlseqs]]` defines** is either acted on or counted; and a final byte the specification gives two *arities* means two things — so an operation baud never enumerated cannot be silently swallowed |
| `baud-core/tests/encoding/graphemes.rs` | all 766 cases of Unicode's `GraphemeBreakTest.txt`, and the generated tables still match the pinned data |
| `baud-core/tests/encoding/width.rs` | the generated width tables still match the pinned `EastAsianWidth.txt` and general category |
| `baud-core/tests/state_fields.rs` | every named field of every `baud-core` struct is read by code, not only assigned, so a mode the terminal records is one it acts on and a counter it keeps is one it reports; `rustc` does not check this, because it counts an assignment as a use |
| `baud-core/tests/docs.rs` | every relative link resolves; every repository path named in backticks exists; every test named as evidence exists; every manifest group has a row in the index above |
| `scripts/check-sources.sh` | every `[[source: …]]` citation — in the docs **and** in the code — resolves to a document pinned by SHA-256 in `MANIFEST.toml`, and every learn.microsoft.com page there carries the SHA-256 of its article text, which `cargo xtask page-text` fetches and compares on demand |
| `xtask/tests/no_python.rs` | `cargo xtask no-python`: no tracked file is Python or runs it, in a script, a shebang, a nix attribute, a `Command::new` or a recipe in a document; each form is planted and red, and the repository itself passes |
| `xtask/tests/features.rs` | `guide/features.md` and `roadmap.md` are what `cargo xtask features` writes from `docs/data/`; every proof path and test a feature names exists; a feature marked built names a test, a done coverage row or a measurement; every shipped roadmap item is claimed by a feature |
| `baud-core/tests/embedding.rs` | the usage example in `docs/embedding.md` is compiled, and its block matches the example's body line for line, apart from blank lines, `use` lines and the example's `println!` |
| `baud-app/src/config.rs` | `docs/guide/configuration.md` lists every key `Config::apply` matches and no other; `config.toml.example` names every key the reader acts on and no retired one, each commented out and, uncommented alone, read as its default but for a marked list of examples, and the file baud writes from it reads as the defaults with nothing said (`baud-app/build.rs` stops on a line in force, a key with no comment, a key or section given twice or a section out of place); `docs/cursor.md` and `docs/cursor.html` show every trail |

Each was written after the corresponding claim turned out to be false. The
coverage document spent most of this project's life naming `tests/coverage.rs`
as the thing that kept it honest, in a file that did not exist; a document pointed
at a vt_parser_states.rs that the SRP split had moved; the README pointed at a
bench/src/payload.rs that the rename to baud-bench had left behind. (Neither
path is written in backticks here, because both are gone and the check below
would rightly flag them — which is the check working on this very paragraph.)
The
failure is always the same shape — prose stays put while the thing it names
moves — and it is mechanical enough that a test can hold it.
