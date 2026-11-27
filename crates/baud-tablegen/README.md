# baud-tablegen

**The build step that turns the Unicode data files into baud's lookup tables.**

A crate's build script reads the pinned data files with it -- the Unicode Character Database and other tables -- and writes typed tables into the build directory. A row it cannot read, a value the data's own alias file does not list, a code point set twice or a range past U+10FFFF stops the build with the file, line and column. It uses the standard library alone.

It is part of [baud](https://github.com/baud-terminal/baud), a terminal emulator, and is one of
the crates baud is built from. Its API follows baud's needs and may change
in any release.

Licensed under CC BY-NC-SA 4.0 (`LICENSE`).
