# Installing baud

There is no release of baud yet, so Nix is the one way to install it for
now. Nix builds baud from the repository: the `baud` command and, on a
Mac, the app, `baud.app`, which you can put in Applications and the Dock.
A clone of the repository builds the same app with devenv.

## What you need

- **A Mac running macOS 26 or later.** On an older system baud says what
  it needs and quits.
- **[Nix](https://nixos.org)** with flakes turned on. It fetches the exact
  compiler baud is built with, so you do not pick a Rust version yourself.

## With Nix

```sh
nix run github:baud-terminal/baud
```

This builds baud and opens a window running your login shell. To keep it
on your `PATH`:

```sh
nix profile add github:baud-terminal/baud
```

The package carries the licences of the fonts baud draws with in
`share/doc/baud/`, as the app does. On a Mac it carries the app too, in
`Applications/baud.app`, signed ad hoc as a clone's build signs it, and
its `bin/baud` is the app's own program. On Linux it carries a desktop
entry, `share/applications/org.baud.terminal.desktop`, and baud's icon
under `share/icons`, which a desktop reads to list baud among its apps.

**About the licence.** baud is CC BY-NC-SA 4.0: free to use and share for
anything that is not commercial. nixpkgs counts a NonCommercial licence as
unfree. The flake's own packages allow baud, so the commands above work as
typed. If you build baud in your own nixpkgs, through the flake's
`overlays.default`, your nixpkgs refuses it until you allow it there:

```nix
nixpkgs.config.allowUnfreePredicate = pkg: lib.getName pkg == "baud";
```

## Build the app

Clone the repository with git and build from the clone, with
[Nix](https://nixos.org) and [devenv](https://devenv.sh):

```sh
git clone https://github.com/baud-terminal/baud
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

`devenv test` runs the test suites and the gates, and the pre-push hook
runs it too. Every check runs locally: baud has no hosted CI.
`devenv tasks run test:x86-avx2` runs the checks of the AVX2 scanner,
which need any x86_64 Linux whose `/proc/cpuinfo` lists `avx2` and
refuse to run anywhere else. [MEASUREMENTS.md](../../MEASUREMENTS.md)
says how each benchmark is run.
