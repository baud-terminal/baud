# Building and running baud

Everything below runs inside `devenv shell`, which pins the nightly toolchain
(`nix/rust.nix`, imported by `devenv.nix`); `devenv test` is the whole gate, as
CI and the pre-push hook run it.

## The terminal

```sh
cargo run --release                   # the terminal, `baud`, from the shell you are in
scripts/bundle-macos.sh               # target/baud.app: the release build as a macOS application
scripts/bundle-macos.sh --no-build    # the same, around a binary already built
```

`baud-app` builds the binary `baud`, the workspace's only binary: a login
shell on a pty, `baud-core`, `baud-render`, and the platform's window --
`baud-present-macos` on Metal, or `baud-present-wayland` on `wl_shm`. The
root's `default-members` makes `cargo run` build it as the bundle does. The bundle gives it an `Info.plist`
(`packaging/macos/`), a Dock icon rendered from the logo, and the embedded
fonts' licences; it needs `iconutil` (macOS) and takes `rsvg-convert` from
`nix shell nixpkgs#librsvg`.

**Configuration** is optional. Copy `config.toml.example` to one of the places
baud looks -- `$BAUD_CONFIG`, then `$XDG_CONFIG_HOME/baud/config.toml`,
`~/.config/baud/config.toml`, then `./config.toml` -- and keep only the keys
you change. With no config baud draws with the fonts compiled into it, and the
system's own faces behind them for what those lack.

**The Metal shaders ship compiled.** `baud-present-macos/src/baud.metallib` is
committed, so a build needs no Metal toolchain.
`scripts/build-metallib.sh [xcode-developer-dir]` rebuilds it from
`shaders.metal` with Xcode's Metal component, and
`the_library_was_built_from_the_shaders_beside_it`
(`baud-present-macos/tests/metallib.rs`) fails if the two drift.

**Linux is type-checked from a Mac.** The Wayland driver is
`cfg(target_os = "linux")`, so a host build here never compiles it;
`cargo clippy -p baud-app --target aarch64-unknown-linux-gnu --all-targets -- -D warnings`,
the `test:linux` task, does.

## The engine

```sh
cargo test --release -p baud-core    # the core's suite: VT conformance, properties, golden captures, coverage
cargo test --release -p baud-difftest -- --nocapture        # cell-by-cell diff against alacritty, its table printed
cargo test --release -p baud-sfte-difftest -- --nocapture   # grid, cursor and replies against sfte, at every read boundary
cargo nextest run --release --workspace && cargo test --release --doc --workspace   # every crate, as `devenv test` runs it
bash audit_all.sh                    # the type-system gate
cargo build --release -p baud-core --target x86_64-unknown-none   # bare metal
```

`baud-domain` and `baud-core` are `#![no_std]` with no allocator, and
`zerocopy`, for its derives and its byte views of cells, is their one
dependency outside this repository. `baud-core`'s `alloc` feature adds
`Owned`, which allocates the buffers a `Storage` borrows, for callers with an
allocator; it is off by default, and the bare-metal build above proves it.

## Benchmarks

```sh
# One payload, in memory, no fork and no pty: the instrument CLAUDE.md's "How
# to verify a change" times a core change with. A recording name, or a ladder rung
# (flat/wrap/lines/sgr), or a synthetic payload (cjk/streaming), or `mix`.
cargo bench -p baud-bench --bench ingest2 -- cat-cjk 250

# The transport with no parser behind it -- the ceiling a PTY row is measured
# against, and the reason a PTY row is a claim about the tty rather than the
# parser.
cargo bench -p baud-bench --bench transport -- utf8/btop.raw 48
cargo bench -p baud-bench --bench ceiling    # the reference ceilings, on mains
cargo bench -p baud-bench --bench opcost     # what one character shape costs
```

The test suite is layered, and each layer catches a different class:

| suite | catches |
|---|---|
| `vt100/`, `vt220/`, `vt420/`, `vt52/`, `xterm/` | per-operation semantics: defaults, clamping, and the cases where two operations are easily conflated |
| `conformance/` | the specification checking baud rather than the reverse — ctlseqs' own lists of finals, forms and `OSC Ps` must each be acted on or counted |
| `properties.rs` | chunk invariance — the kernel hands you arbitrary slices, and a sequence split across two reads must parse identically |
| `golden.rs` | what `baud-core2` rendered for 54 fixtures and 9 recordings, frozen as data before that crate was deleted, and moved since only where `recordings.txt` records a decision (two recordings). It finds regressions on recorded bytes and nothing else |
| `cell_table.rs` | the three-byte cell table against the write path, on all 65,536 sequences — so turning `cell-table` on cannot change the screen |
| `docs.rs`, `coverage.rs`, `readme.rs`, `api_coverage.rs` | the documents checking the code: every cited path and test name resolves, every `done` row has a test, the README's example compiles, every `pub fn` has a caller |
| `baud-difftest/tests/alacritty.rs` | cell-by-cell agreement with alacritty |
| `baud-sfte-difftest/tests/sfte.rs` | agreement with sfte, a second implementation, on the grid, the cursor and the replies, at every 4096-byte read of every fixture and recording; what differs is pinned with its reason. Its build copies the header pinned in `docs/research/sources/`, applies `baud-sfte-difftest/patches/` to the copy, and compiles that with `cc` |

Every test is deterministic. `.proptest-regressions` is checked in so a property
failure replays exactly, and a case worth finding once is also pinned by a
deterministic test rather than left depending on a saved seed.

`baud-difftest` needs no checkout and no edit. It pins `alacritty_terminal` to
the registry version `multibench` already uses, so the two external comparisons
answer to one alacritty, and it is a **workspace member** -- `cargo test
--workspace` builds it and runs its tests. Both differentials are test targets
and not binaries, so `devenv test`'s `cargo nextest run --workspace` runs them
with the rest; `baud` is the one binary. A bare `cargo test`, `cargo build` or
`cargo clippy` at the root covers `baud-app` alone, because `default-members`
names only it; the gate passes `--workspace`.

That is new, and the reason it is worth a paragraph: it used to pin alacritty by
the absolute path `/home/user/t/alacritty/alacritty_terminal`, which no
checkout has, so **it had never compiled** while this table listed it as "the
only oracle outside this repo" and its own doc comment described what it fed to
`baud-core`. Being excluded from the workspace is what kept that quiet. Running
it at `d7bddf0`: **32 of 32 cases agree**, with two representation differences
normalised and counted rather than pinned -- baud stores `Content::WIDE_TAIL` where
alacritty stores a space flagged `WIDE_CHAR_SPACER`, and alacritty keeps the
`\t` byte in a blank cell for selection fidelity where baud leaves the cells
untouched. Both draw nothing; the counts are asserted exactly, so a
normalisation that starts firing somewhere new has to come and change them.

The cross-implementation comparison (`multibench`) and the with/without-PTY
split (`alacritty-bench`) ship separately, in `bench-others/`.

**And `alacritty-bench` carried the same defect as the paragraph above, found
by looking for it.** It pinned alacritty at `../../../alacritty/alacritty_terminal`,
a sibling checkout this one does not have, and `bench-others` is outside the
workspace, so nothing ever tried to build it and nothing reported that it could
not. `bench-others/README.md` listed that path as a requirement rather than as a
bug. It takes the registry version its two siblings already pin, and it now
builds and produces output on a single run -- which is a liveness check and not
a measurement: those are alacritty's own synthetic payloads, one launch, no
interleaving and no control, so no number from it belongs in a table.

## The source gate

`scripts/check-sources.sh`, run by `tasks."test:sources"`, fails if the research
corpus and its manifest have drifted apart — a missing file, a file whose
contents no longer hash to the recorded SHA-256, a file with no manifest entry,
or a doc citing a key the manifest does not carry.

The last one is the point: it stops a design document resting on a citation
nobody could check. A claim about a control function must trace to a primary
source in `docs/research/sources/`, cited as `[[source: <key>]]`. Sources marked
`gated` were never retrieved and **cannot** be cited.

Do not implement a control function from memory. `docs/vt-coverage.md` records
what the specifications say, including where two of them disagree.

## The audit gate

`audit_all.sh` fails if raw integers appear outside the places they belong. Each
per-file limit has its reason written into the script — array dimensions, SIMD
lane masks, the syscall ABI. A rise means something untyped was reintroduced,
not that the limit was too tight.
