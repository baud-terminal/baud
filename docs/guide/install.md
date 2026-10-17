# Installing baud

baud is built from source. There are no downloads or packages yet, so the
steps below build it on your Mac and leave you with an app you can put in
Applications.

## What you need

- **A Mac running macOS 26 or later.** baud's window is made of macOS 26's
  Liquid Glass, so it will not open on an older system.
- **[Nix](https://nixos.org) and [devenv](https://devenv.sh).** They fetch
  the exact compiler and tools baud is built with, so you do not install a
  Rust toolchain yourself.

## Build the app

From a checkout of the repository:

```sh
devenv shell -- scripts/bundle-macos.sh
```

This leaves `baud.app` in `target/`. Open it from Finder, or drag it to
Applications and launch it from there or from the Dock.

To try baud without making the app, run it from the terminal you are in:

```sh
devenv shell -- cargo run --release
```

Either way baud opens a window running your login shell.

## What a program sees

baud tells the programs it runs who it is the usual way: `TERM` is
`xterm-256color`, `COLORTERM` is `truecolor`, and `TERM_PROGRAM` is `baud`.
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

The Linux version builds and has run in a headless session, but it has no
mouse input yet and nobody has looked at what it draws, so it is not ready
to use. [Status](status.md) says what is missing.

## For contributors

The test suites, the benchmarks and the gates are in
[docs/contributing/build.md](../contributing/build.md).
