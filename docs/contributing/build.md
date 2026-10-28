# Building and testing baud

Everything below runs inside `devenv shell`, which pins the nightly toolchain
(`nix/rust.nix`, imported by `devenv.nix`); `devenv test` is the whole gate, as
CI and the pre-push hook run it.

It requires [Nix](https://nixos.org) and [devenv](https://devenv.sh).

```sh
devenv test                 # the gate: what CI and the pre-push hook run
devenv shell -- dev-ci      # the same checks as one script, less the per-feature and path tasks
devenv shell -- dev-test    # tests only
```

`.github/workflows/ci.yml` runs `devenv test`, so a green gate here is the
same task set CI runs. A red unit test does not hide the rest: the five
checks after it (the bare-metal build, the Linux lint, the audit, the
feature arms and the path check) still run, and the gate still fails, so
one run lists every independent failure. A formatting or compile error
skips the unit tests, and the five checks still run after it. Run the
gate whole: a single later check run alone,
`devenv tasks run test:features`, runs the checks before it and exits 0
over a red unit test (the comment above `enterTest` in `devenv.nix`).
**Add a check by adding a task, never by adding a step to the workflow**
([CONTRIBUTING.md](../../CONTRIBUTING.md)).

## The terminal

For installing and running baud as a user, see
[the install guide](../guide/install.md); this section is how the build
fits together.

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
the dev shell, which runs it on macOS from the librsvg of the nixpkgs
`devenv.lock` pins.

**macOS 26 and later** (the owner, 2026-10-13): the window is Liquid Glass,
`NSGlassEffectView`, which macOS 26 introduced
(`[[source: sdk27-nsglasseffectview-h]]`). The release is said in three
places and `baud-app/tests/macos_floor.rs` holds them equal: the bundle's
`LSMinimumSystemVersion`, on which Launch Services refuses to open it on an
older system (`kLSIncompatibleSystemVersionErr`,
`[[source: sdk27-lsconstants-h]]`); the deployment target in
`.cargo/config.toml`, forced over the devenv shell's 14.0, which the linker
writes into the binary as `LC_BUILD_VERSION minos`; and the Metal library's target. A bare `baud` run
on an older system meets the app's own check, which prints why on stderr
and exits 1 (forced on macOS 27; not run below 26).

**Configuration** is optional; [the configuration guide](../guide/configuration.md)
has every key and every place baud looks, and `config.toml.example` is the
file to copy.

**The Metal shaders ship compiled.** `baud-present-macos/src/baud.metallib` is
committed, so a build needs no Metal toolchain.
`scripts/build-metallib.sh [xcode-developer-dir]` rebuilds it from
`shaders.metal` with Xcode's Metal component, for macOS 26, and
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

## The suites

The test suite is layered, and each layer catches a different class:

| suite | catches |
|---|---|
| `vt100/`, `vt220/`, `vt420/`, `vt52/`, `xterm/` | per-operation semantics: defaults, clamping, and the cases where two operations are easily conflated |
| `conformance/` | the specification checking baud rather than the reverse — ctlseqs' own lists of finals, forms and `OSC Ps` must each be acted on or counted |
| `properties.rs` | chunk invariance — the kernel hands you arbitrary slices, and a sequence split across two reads must parse identically |
| `golden.rs` | what `baud-core2` rendered for 54 fixtures and 9 recordings, frozen as data before that crate was deleted, and moved since only where `recordings.txt` records a decision (two recordings). It finds regressions on recorded bytes and nothing else |
| `cell_table.rs` | the three-byte cell table against the write path, on all 65,536 sequences — so turning `cell-table` on cannot change the screen |
| `docs.rs`, `coverage.rs`, `embedding.rs`, `api_coverage.rs` | the documents checking the code: every cited path and test name resolves, every `done` row has a test, the embedding guide's example compiles, every `pub fn` has a caller |
| `feature_list.rs` | the feature list and the roadmap: it runs `python3 scripts/gen-features.py --check`, which fails when `docs/guide/features.md` or `docs/roadmap.md` differs from what the script writes from `docs/data/`, when a feature's proof path or test is gone, or when a feature marked built names none. Edit the data and rerun the script; the pages are never edited by hand |
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
the absolute path `/home/<user>/t/alacritty/alacritty_terminal`, which no
checkout has, so **it had never compiled** while this table listed it as "the
only oracle outside this repo" and its own doc comment described what it fed to
`baud-core`. Being excluded from the workspace is what kept that quiet. Running
it at `f87eadf`: **32 of 32 cases agree**, with two representation differences
normalised and counted rather than pinned -- baud stores `Content::WIDE_TAIL` where
alacritty stores a space flagged `WIDE_CHAR_SPACER`, and alacritty keeps the
`\t` byte in a blank cell for selection fidelity where baud leaves the cells
untouched. Both draw nothing; the counts are asserted exactly, so a
normalisation that starts firing somewhere new has to come and change them.

The benchmarks, in this repository and in `bench-others/`, are in
[measuring.md](measuring.md).

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
