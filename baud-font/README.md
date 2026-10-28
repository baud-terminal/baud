# baud-font

**baud's font stack: font files read, shaped and rasterised with no font engine underneath.**

It reads TrueType, OpenType, CFF and variable fonts, shapes text through GSUB, GPOS and morx, and draws the glyphs itself. It is `no_std`.

It is part of [baud](https://github.com/i-am-logger/baud), a terminal emulator, and is published so that
`cargo install baud` can build it. Its API follows baud's needs and may
change in any release.

Licensed under CC BY-NC-SA 4.0 (`LICENSE`); `NOTICE` names the third-party material it carries, each part's licence and where its notice is.
