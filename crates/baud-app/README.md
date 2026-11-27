# baud

**The terminal with character.**

baud is a terminal emulator for macOS 26 and later. It ships with its own
font, so text looks right on the first launch with nothing to install;
the cursor can leave a plasma, fire or lightning trail when it jumps; and
it takes output in as fast as programs send it. Pictures show in the
terminal through the kitty graphics protocol and Sixel, and the history
scrolls, selects and rewraps when the window is resized.

## Install

baud installs with [Nix](https://nixos.org), with flakes turned on:

```sh
nix run github:baud-terminal/baud
```

This builds the `baud` binary, for macOS 26 and later or for Linux under
Wayland, with the settings baud is
measured with, and opens a window running your login shell;
`nix profile add github:baud-terminal/baud` keeps it on your `PATH`. The
app bundle for Applications and the Dock is built from the repository
([Installing](https://github.com/baud-terminal/baud/blob/master/docs/guide/install.md)).

On Linux baud runs under Wayland, as an everyday terminal;
[Status](https://github.com/baud-terminal/baud/blob/master/docs/guide/status.md)
says what is in progress there.

## More

[The README](https://github.com/baud-terminal/baud#readme) has pictures of all of it,
[Features](https://github.com/baud-terminal/baud/blob/master/docs/guide/features.md) lists every one, and
[Configuration](https://github.com/baud-terminal/baud/blob/master/docs/guide/configuration.md) every setting.

baud is licensed under CC BY-NC-SA 4.0 (`LICENSE`); the third-party files
it carries, such as its fonts, keep their own licences beside them, and
`NOTICE` names each with its licence and where its notice is. This software is
based in part on the work of the Independent JPEG Group.
