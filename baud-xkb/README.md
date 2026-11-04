# baud-xkb

**An XKB keymap compiler for the keymap a Wayland compositor sends, with no libxkbcommon.**

It reads the keymap text, resolves its modifiers and key types, and answers what a key produces under the current modifier state. Its keysym table is generated at build time from xkbcommon's header, which it carries with its notice.

It is part of [baud](https://github.com/baud-terminal/baud), a terminal emulator, and is one of
the crates baud is built from. Its API follows baud's needs and may change
in any release.

Licensed under CC BY-NC-SA 4.0 (`LICENSE`); `NOTICE` names the third-party material it carries, each part's licence and where its notice is.
