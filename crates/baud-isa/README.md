# baud-isa

**The scan baud reads terminal output with, one block of bytes at a time.**

The portable implementation, and the interface the instruction-set crates implement. It is `no_std` and allocates nothing.

It is part of [baud](https://github.com/baud-terminal/baud), a terminal emulator, and is one of
the crates baud is built from. Its API follows baud's needs and may change
in any release.

Licensed under CC BY-NC-SA 4.0 (`LICENSE`).
