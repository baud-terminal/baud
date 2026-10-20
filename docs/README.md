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
| [guide/status.md](guide/status.md) | what works today and what is not built yet |
| [guide/performance.md](guide/performance.md) | how fast baud is, what was measured, and what was not |
| [cursor.md](cursor.md) | the cursor trails, a still of each; [cursor.html](cursor.html) plays them side by side from a checkout |
| [guide/licensing.md](guide/licensing.md) | what the licence allows, a commercial licence, and sponsoring |
| [../config.toml.example](../config.toml.example) | the config file to copy, with every key explained |
| [../SECURITY.md](../SECURITY.md) | what baud trusts, what it refuses, and how to report a vulnerability |

## Embed the core

| | read it for |
|---|---|
| [embedding.md](embedding.md) | using `baud-core` in your own program, with a compiled example |

## Contribute

| | read it for |
|---|---|
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | what a change needs before it lands |
| [contributing/build.md](contributing/build.md) | setting up, the suites and the gates |
| [contributing/architecture.md](contributing/architecture.md) | the data path, the two core crates and the constraints that shaped both; read it before touching code |
| [contributing/measuring.md](contributing/measuring.md) | which benchmark answers which question, and how to run it |
| [../CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md) | how contributors treat each other |

## Reference

| | read it for |
|---|---|
| [vt-coverage.md](vt-coverage.md) | whether a specific control sequence works: one row per operation, against its primary source |
| [input-coverage.md](input-coverage.md) | what baud sends for each key, modifier and input mode |
| [protocols/](protocols/) | one document per level (VT52, VT100, VT220, VT420, xterm), mirroring the code and tests, and a page per `$TERM` profile |
| [protocol-decisions.md](protocol-decisions.md) | whether something that looks like a gap is deliberate, and the open departures from the standards |
| [../MEASUREMENTS.md](../MEASUREMENTS.md) | every number, stamped with the host and commit that produced it |
| [../THIRD_PARTY.md](../THIRD_PARTY.md) | every third-party file, with its licence and source |

## History and research

| | read it for |
|---|---|
| [record.md](record.md) | the dated record of every measured change and every decision |
| [../CLAUDE.md](../CLAUDE.md) | the working rules for agents and contributors: the measurement discipline, and what was tried and lost |
| [the notes below](#research) | the research behind each design, one note per question |

## Research

`research/sources/` holds the primary sources every protocol claim cites:
xterm's `ctlseqs`, Paul Flo Williams' DEC ANSI parser, ECMA-48, the VT510
programmer reference, and the papers behind the SIMD and FSM work. Each is
recorded in [`MANIFEST.toml`](research/sources/MANIFEST.toml) with a SHA-256,
and `scripts/check-sources.sh` re-hashes them, so a citation to a key the
manifest does not carry — or one recorded as `gated`, meaning never obtained —
fails the build.

`research/notes/` is where a source gets turned into an argument about baud:
each carries its own status line, and many are a dated record of what was
decided. The status below says done, with the date and the commit, only
when every question a note asks is answered or built. A note's letter is
its filename's prefix, and several notes share one.

| note | title | status |
|---|---|---|
| [a-byte-at-a-time.md](research/notes/a-byte-at-a-time.md) | What the kernel and an ATMega do — and why it is not a throughput idea | open |
| [a-simd-parsing.md](research/notes/a-simd-parsing.md) | SIMD parsing — the decoder is the part with a known better algorithm | open |
| [aa-shell-locale.md](research/notes/aa-shell-locale.md) | AA. The shell's locale, and Hebrew | open |
| [ab-right-to-left.md](research/notes/ab-right-to-left.md) | AB. Right-to-left text | open |
| [ac-built-font.md](research/notes/ac-built-font.md) | AC. The embedded font, built from parts | open |
| [ad-unrenderable-in-a-cell.md](research/notes/ad-unrenderable-in-a-cell.md) | AD. What a cell draws for a code point no face draws | open |
| [ae-code-extension.md](research/notes/ae-code-extension.md) | AE. Code extension: the four G slots, GL and GR, and the shifts | open |
| [af-character-sets.md](research/notes/af-character-sets.md) | AF. Character sets against DEC's own tables: the national replacement sets and the DEC sets baud designates but does not translate | open |
| [ag-replies-and-images-under-load.md](research/notes/ag-replies-and-images-under-load.md) | AG. Under load: the reply buffer and the Sixel body | open |
| [ah-title-bar-leds.md](research/notes/ah-title-bar-leds.md) | AH. Lights on the title bar: what each LED may honestly light for | open |
| [ai-dock-launch-logging.md](research/notes/ai-dock-launch-logging.md) | AI. Where a Dock-launched baud's warnings go | open |
| [aj-launch-splash.md](research/notes/aj-launch-splash.md) | AJ. The launch splash: the colour logo, and the plasma it vanishes in | open |
| [ak-window-title.md](research/notes/ak-window-title.md) | AK. The window title: what a program sets, what baud derives, and where it shows | open |
| [al-trail-flashes.md](research/notes/al-trail-flashes.md) | AL. The cursor trails' flashes: what the texts require, and four ways to bound them | open |
| [am-xtgettcap.md](research/notes/am-xtgettcap.md) | AM. XTGETTCAP: what baud says about its own capabilities | open |
| [an-dcs-bodies-and-sixel.md](research/notes/an-dcs-bodies-and-sixel.md) | AN. DCS bodies and Sixel: what the texts say per byte | open |
| [ap-da-level.md](research/notes/ap-da-level.md) | AP. The Primary DA level: can baud answer `?61;4c`? | open |
| [aq-docs-structure.md](research/notes/aq-docs-structure.md) | AQ. How the documentation is arranged, and for whom | open |
| [ar-theme-formats.md](research/notes/ar-theme-formats.md) | AR. Theme file formats, and telling them apart by content | open |
| [as-glass-legibility.md](research/notes/as-glass-legibility.md) | AS. Glass legibility: what Apple recommends, and what baud draws | open |
| [at-standard-materials.md](research/notes/at-standard-materials.md) | AT. A standard material for the window body | open |
| [au-floor-hue.md](research/notes/au-floor-hue.md) | AU. How the contrast floor keeps a lifted colour's hue | open |
| [av-counted-and-dropped.md](research/notes/av-counted-and-dropped.md) | AV. Everything baud reads and then drops: the walk | open |
| [aw-scope-calls.md](research/notes/aw-scope-calls.md) | AW. The scope calls: printing, the locator, the status line, the keyboard lights, bell volume, the VT525 colours, ReGIS and Tektronix | open |
| [b-data-parallel-fsm.md](research/notes/b-data-parallel-fsm.md) | Data-parallel finite-state machines — vectorising the parser itself | open |
| [b-parser-as-a-graph.md](research/notes/b-parser-as-a-graph.md) | The parser as a graph — how to store 20 nodes and 5,120 edges | open |
| [c-vt-parser.md](research/notes/c-vt-parser.md) | C — the terminal parser as specified | done 2026-09-11, `f920ea3` |
| [d-memory-hierarchy.md](research/notes/d-memory-hierarchy.md) | Memory hierarchy — why a read cost 2.5× where a write did not | open |
| [e-tty-layer.md](research/notes/e-tty-layer.md) | The tty layer — why a PTY read is 1024 bytes on Darwin and 4095 on Linux | open |
| [f-cluster-storage.md](research/notes/f-cluster-storage.md) | Where a multi-codepoint cluster lives — three terminals, three answers | open |
| [f-row-extent.md](research/notes/f-row-extent.md) | The per-row used extent — how four terminals decide what to blank | open |
| [f-scrollback-ring.md](research/notes/f-scrollback-ring.md) | F — Where a retired line lives: the grid as a ring, checked against the field | open |
| [g-property-lookup.md](research/notes/g-property-lookup.md) | Two lookups a character, unconditionally — and the design that does fewer | open |
| [g-text-encoding.md](research/notes/g-text-encoding.md) | G — text encoding, width and grapheme clustering | open |
| [h-session-context-budget.md](research/notes/h-session-context-budget.md) | H. Every session starts half full, and one edit to CLAUDE.md overflows it | open |
| [i-isa-dispatch.md](research/notes/i-isa-dispatch.md) | Instruction-set dispatch — why the seam is a trait and not a crate | open |
| [j-colour-bitmap-limits.md](research/notes/j-colour-bitmap-limits.md) | J: colour-bitmap limits -- a presence index, its bound, CBLC subtables, flip chains | open |
| [j-rendering-evidence.md](research/notes/j-rendering-evidence.md) | Rendering -- the evidence behind the register | open |
| [j-rendering.md](research/notes/j-rendering.md) | Rendering — the decisions register | open |
| [j-text-antialiasing.md](research/notes/j-text-antialiasing.md) | J. Text anti-aliasing: weight, blend, hinting and placement | open |
| [k-selection.md](research/notes/k-selection.md) | K. Selection, copy and paste | open |
| [l-scrollbar.md](research/notes/l-scrollbar.md) | L. The scrollbar | open |
| [m-resize-and-reflow.md](research/notes/m-resize-and-reflow.md) | M. Resize, reflow, and the prompt the shell redraws | open |
| [n-make-faster.md](research/notes/n-make-faster.md) | N. What baud can make faster, as a makefaster checklist | open |
| [o-keystroke-latency.md](research/notes/o-keystroke-latency.md) | O. Keystroke to pixel: the instrument, and what it read first | open |
| [p-shader-cache.md](research/notes/p-shader-cache.md) | P. The shader cache at launch: 350 ms cold, 12 warm, and what each remedy would buy | open |
| [q-local-echo.md](research/notes/q-local-echo.md) | Q. Local echo prediction: what row 9 draws before the pty answers, and what judges it | open |
| [r-sfte.md](research/notes/r-sfte.md) | R. sfte as the second oracle | open |
| [s-sixel.md](research/notes/s-sixel.md) | S. Sixel graphics | open |
| [t-images.md](research/notes/t-images.md) | T. Images: the phase register for Sixel and Kitty graphics | open |
| [u-resize-and-the-window.md](research/notes/u-resize-and-the-window.md) | U. Who holds the window over the history across a resize | open |
| [v-open-decisions.md](research/notes/v-open-decisions.md) | V. The owner's open decisions, researched | open |
| [w-cursor-trails.md](research/notes/w-cursor-trails.md) | W. The cursor trails: where each design came from, what was decided, and what is lost | open |
| [w-fallback-fonts.md](research/notes/w-fallback-fonts.md) | W. Fallback fonts: what other terminals do, and what baud could bundle | superseded by AC |
| [x-cursor-motion-and-blink.md](research/notes/x-cursor-motion-and-blink.md) | X. The cursor's motion and blink: the designs as reviewed | open |
| [y-palettes.md](research/notes/y-palettes.md) | Y. Palettes: the light/dark pair, the scheme type, the floor, and what the terminal reports | open |
| [z-variable-fonts.md](research/notes/z-variable-fonts.md) | Z. Variable fonts in baud-font | open |

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
| BL | the bell: what a `BEL` does in a macOS window -- the alert sound, an attention request, a raise (no note; `baud-app/src/bell.rs` and the record's section cite them) |
| BU | bare URIs in text: what a URI is, where one ends in prose, and how terminals find one under a click (no note; `baud-app/src/link.rs` and the record's section cite them) |

A group's letter and a note's need not match: group H's sources are key
encoding, and note H is the session context budget. Both tables are held to
the directory and the manifest by
`every_research_note_and_source_group_is_in_the_index`
(`baud-core/tests/docs.rs`), which fails when a note has no row or a group
has none.

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
| `baud-core/tests/coverage.rs` | every **done** row names an operation some test mentions; no row is a plain `partial`, `parsed` or `todo`, though a bold `**partial**` passes it, an open defect CLAUDE.md records; **every `out` row states a reason**; the operation counts quoted in README, CLAUDE.md and MEASUREMENTS.md match the table |
| `baud-core/tests/conformance/enumerated.rs` | every **done** row's sequence, synthesised from the table and fed to a real `Term`, reaches a handler rather than being counted unsupported |
| `baud-core/tests/conformance/spec_inventory.rs` | the reverse direction: every plain CSI final, every CSI *form* (markers and intermediates), every `ESC Fe`, **every `ESC` form with an intermediate — including all four G-slots × every charset final**, every `OSC Ps` and every `DCS` form that **`[[source: ctlseqs]]` defines** is either acted on or counted; and a final byte the specification gives two *arities* means two things — so an operation baud never enumerated cannot be silently swallowed |
| `baud-core/tests/encoding/graphemes.rs` | all 766 cases of Unicode's `GraphemeBreakTest.txt`, and the generated tables still match the pinned data |
| `baud-core/tests/encoding/width.rs` | the generated width tables still match the pinned `EastAsianWidth.txt` and general category |
| `baud-core/tests/docs.rs` | every relative link resolves; every repository path named in backticks exists; every test named as evidence exists; **nothing listed as absent is also marked done**; every research note and every manifest group has a row in the index above |
| `scripts/check-sources.sh` | every `[[source: …]]` citation — in the docs **and** in the code — resolves to a document pinned by SHA-256 in `MANIFEST.toml` |
| `baud-core/tests/feature_list.rs` | `guide/features.md` and `roadmap.md` are what `scripts/gen-features.py` writes from `docs/data/`; every proof path and test a feature names exists; a feature marked built names a test, a done coverage row or a measurement; every shipped roadmap item is claimed by a feature |
| `baud-core/tests/embedding.rs` | the usage example in `docs/embedding.md` is compiled, and its block matches the example's body line for line, apart from blank lines, `use` lines and the example's `println!` |
| `baud-app/src/config.rs` | `docs/guide/configuration.md` lists every key `Config::apply` matches and no other; `config.toml.example` writes only keys the reader acts on; `docs/cursor.md` and `docs/cursor.html` show every trail |

Each was written after the corresponding claim turned out to be false. The
coverage document spent most of this project's life naming `tests/coverage.rs`
as the thing that kept it honest, in a file that did not exist; a note pointed
at a vt_parser_states.rs that the SRP split had moved; the README pointed at a
bench/src/payload.rs that the rename to baud-bench had left behind. (Neither
path is written in backticks here, because both are gone and the check below
would rightly flag them — which is the check working on this very paragraph.)
The
failure is always the same shape — prose stays put while the thing it names
moves — and it is mechanical enough that a test can hold it.
