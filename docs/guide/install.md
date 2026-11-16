# Installing baud

There is no release of baud yet, so Nix is the one way to install it for
now. Nix builds baud from the repository: the `baud` command and, on a
Mac, the app, `baud.app`, which you can put in Applications and the Dock.
A clone of the repository builds the same app with devenv.

## What you need

- **A Mac running macOS 26 or later, or Linux under a Wayland
  compositor.** On an older macOS baud says what it needs and quits.
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

The package carries baud's licence, the third-party notices and the
licences of the fonts baud draws with in `share/doc/baud/`, as the app
does. On a Mac it carries the app too, in
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

This leaves `baud.app` in `target/`. Open it from Finder, or drag it to
Applications and launch it from there or from the Dock.

Clone rather than download a ZIP from GitHub. A ZIP has no `.git` folder:
the dev shell starts and `cargo build --release` works, but the build cannot
name its commit, so `baud --version` ends in `+unknown`, and `devenv test`
stops at once, as its hooks need a git repository.

To try baud without making the app, run it from the terminal you are in:

```sh
devenv shell -- cargo run --release
```

## What a program sees

baud tells the programs it runs who it is the usual way: `TERM` is
`xterm-256color`, `COLORTERM` is `truecolor`, `TERM_PROGRAM` is `baud`, and
`TERM_PROGRAM_VERSION` is baud's version with its build, `<version>+<commit>` (the one XTVERSION reports and `baud --version` prints).
Programs read these to decide which keys, colours and features to use.

## Credits

`baud --credits` prints the notices of every third-party work the `baud`
program carries -- its fonts and their icon sets, the Unicode data and
other tables built into it, and the code ported into it from other
projects -- each with its copyright, what was changed and its licence, then
the licence texts, and exits, as `baud --version` does, without opening a
window or touching your configuration.
[THIRD_PARTY.md](../../THIRD_PARTY.md) opens with the same notices, and on
macOS, baud > About baud > Acknowledgements opens that page on GitHub, at
the build's own commit.

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
- **A window that says it could not open a terminal** met the system's limit
  on terminals, which every app's windows and tabs share: 511 on a Mac unless
  it was raised. The window says how many are open, and stays until you close
  it; your other windows go on as they were. Close terminals you no longer
  need, or raise the limit, up to 999, with
  `sudo sysctl kern.tty.ptmx_max=999`. That lasts until the Mac restarts; add
  `kern.tty.ptmx_max=999` to `/etc/sysctl.conf` to keep it.
- **baud can crash at launch, rarely, when a second copy starts at the same
  moment.** Opening another copy with `open -n`, or running the app's binary
  while baud is already open, starts a second baud beside the first, and the
  two can collide in macOS's Metal shader cache while they start. Quit the
  extra copy and open baud again.

## Linux

baud runs as an everyday terminal on Linux under a Wayland compositor.
Given the same fonts and colours it draws the same pixels as the Mac
version, copies and pastes with the compositor's clipboard, and the mouse
selects, scrolls back, pastes with the middle button, passes links on to
be opened and works in programs that use the mouse; characters drawn from
each system's own fonts, and some programs' dim colours, can still look
different. A point is the desktop's there, 96 dots to the inch times its
text scaling, so text is larger than the same `font_size` on the Mac.
[Status](status.md) says what is in progress on Linux and what is open.

## For contributors

`devenv test` runs the test suites and the gates, and the pre-push hook
runs it too. Every check runs locally: baud has no hosted CI.
`devenv tasks run test:x86-avx2` runs the checks of the AVX2 scanner. They
need an x86_64 Linux whose `/proc/cpuinfo` lists `avx2`, and they refuse to
run on any other host. `scripts/check-fresh-shell.sh` starts the dev shell
the way a first run does, in a new clone and in a tree with no `.git`; it
needs the network, so `devenv test` does not run it.
[MEASUREMENTS.md](../../MEASUREMENTS.md) says how each benchmark is run.
