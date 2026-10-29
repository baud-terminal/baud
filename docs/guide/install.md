# Installing baud

baud installs with Nix today, and with Cargo once its first release is on
crates.io. Both give you the `baud` binary; to have an app you can put in
Applications and the Dock, build it from the repository.

## What you need

- **A Mac running macOS 26 or later.** On an older system baud says what
  it needs and quits.
- **[Nix](https://nixos.org)** with flakes turned on, or
  **[rustup](https://rustup.rs)** for Cargo. Either fetches the exact
  compiler baud is built with, so you do not pick a Rust version yourself.

## With Nix

```sh
nix run github:i-am-logger/baud
```

This builds baud and opens a window running your login shell. To keep it
on your `PATH`:

```sh
nix profile add github:i-am-logger/baud
```

The package carries the licences of the fonts baud draws with in
`share/doc/baud/`, as the app does.

**About the licence.** baud is CC BY-NC-SA 4.0: free to use and share for
anything that is not commercial. nixpkgs counts a NonCommercial licence as
unfree. The flake's own packages allow baud, so the commands above work as
typed. If you build baud in your own nixpkgs, through the flake's
`overlays.default`, your nixpkgs refuses it until you allow it there:

```nix
nixpkgs.config.allowUnfreePredicate = pkg: lib.getName pkg == "baud";
```

## With Cargo, once published

Once baud's first release is on [crates.io](https://crates.io/crates/baud),
Cargo installs it with the nightly Rust toolchain baud is built with, which
[rustup](https://rustup.rs) provides, and with the dependency versions baud
was released with (`--locked`):

```sh
cargo +nightly-2026-04-11 install --locked baud
```

This puts the `baud` binary in `~/.cargo/bin`; run `baud` to open a window.
It is built the way the repository builds it, with whole-program link-time
optimisation, and for macOS 26. A stable toolchain stops before it builds
anything, with an error that says a nightly Cargo is needed.

## Build the app

Clone the repository with git and build from the clone, with
[Nix](https://nixos.org) and [devenv](https://devenv.sh):

```sh
git clone https://github.com/i-am-logger/baud
cd baud
devenv shell -- scripts/bundle-macos.sh
```

A ZIP download from GitHub will not build: it has no `.git` folder, and
the dev shell does not start without one.

This leaves `baud.app` in `target/`. Open it from Finder, or drag it to
Applications and launch it from there or from the Dock.

To try baud without making the app, run it from the terminal you are in:

```sh
devenv shell -- cargo run --release
```

## What a program sees

baud tells the programs it runs who it is the usual way: `TERM` is
`xterm-256color`, `COLORTERM` is `truecolor`, `TERM_PROGRAM` is `baud`, and
`TERM_PROGRAM_VERSION` is baud's version (the one XTVERSION reports).
Programs read these to decide which keys, colours and features to use.

## Make it yours

baud works with no configuration at all. To change the font, colours,
cursor or window, see [Configuration](configuration.md).

## If something looks wrong

- **Warnings go to the terminal baud was started from.** When you launch
  the app from Finder or the Dock there is no such terminal, so start it as
  `target/baud.app/Contents/MacOS/baud` from another terminal to see them.
  `BAUD_LOG=info` shows more; `BAUD_LOG=err` shows only errors.
- **On a release before macOS 26** the app refuses to open, and the bare binary
  prints why and exits.

## Linux

The Linux version builds and has run in headless sessions. There, given
the same fonts and colours, it draws the same pixels as the Mac version,
it copies and pastes with the desktop's clipboard, and a mouse selects,
scrolls back, pastes with the middle button, opens links and works in
programs that use the mouse; characters drawn from each system's own
fonts, and some programs' dim colours, can still look different. Nobody
has used it on a real screen yet, so it is not ready to use.
[Status](status.md) says what is missing.

## For contributors

The test suites, the benchmarks and the gates are in
[docs/contributing/build.md](../contributing/build.md).
