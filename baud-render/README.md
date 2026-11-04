# baud-render

**baud's renderer: the screen's cells turned into pixels.**

It shapes and lays out each row, keeps the glyph atlas, draws the cursor and its trails, and redraws only the rows that changed, on the CPU or as instances for the GPU.

It is part of [baud](https://github.com/baud-terminal/baud), a terminal emulator, and is one of
the crates baud is built from. Its API follows baud's needs and may change
in any release.

Licensed under CC BY-NC-SA 4.0 (`LICENSE`); `NOTICE` names the third-party material it carries, each part's licence and where its notice is.
