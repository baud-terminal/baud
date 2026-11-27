# baud-domain

**The types baud is built from: cells, colours, modes, widths and the Unicode tables behind them.**

Every quantity is its own type, so mixing up a column and a row, or a byte count and a cell count, does not compile. It also carries the generated Unicode, colour and character-set tables, and the image decoders for pictures in the terminal. It is `no_std`.

It is part of [baud](https://github.com/baud-terminal/baud), a terminal emulator, and is one of
the crates baud is built from. Its API follows baud's needs and may change
in any release.

Licensed under CC BY-NC-SA 4.0 (`LICENSE`); `NOTICE` names the third-party material it carries, each part's licence and where its notice is.
