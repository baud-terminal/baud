# baud

**The terminal with character: its own font, its own cursor effects, and
the speed to keep up.**

baud is a terminal emulator for macOS 26 and later. It ships with its own
font, so text looks right on the first launch with nothing to install;
the cursor can leave a plasma, fire or lightning trail when it jumps; and
it takes output in as fast as programs send it. Pictures show in the
terminal through the kitty graphics protocol and Sixel, and the history
scrolls, selects and rewraps when the window is resized.

## Install

baud is built with the nightly Rust toolchain of 2026-04-11, and with the
dependency versions it was released with:

```sh
cargo +nightly-2026-04-11 install --locked baud
baud
```

This installs the `baud` binary, which opens a window running your login
shell. The app bundle for Applications and the Dock is built from the
repository ([Installing](https://github.com/i-am-logger/baud/blob/master/docs/guide/install.md)).

The Linux version builds and has run in headless sessions; it is not ready
to use yet ([Status](https://github.com/i-am-logger/baud/blob/master/docs/guide/status.md)).

## More

[The README](https://github.com/i-am-logger/baud#readme) has pictures of all of it,
[Features](https://github.com/i-am-logger/baud/blob/master/docs/guide/features.md) lists every one, and
[Configuration](https://github.com/i-am-logger/baud/blob/master/docs/guide/configuration.md) every setting.

baud is licensed under CC BY-NC-SA 4.0 (`LICENSE`); the third-party files
it carries, such as its fonts, keep their own licences beside them.
