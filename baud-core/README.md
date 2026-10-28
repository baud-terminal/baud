# baud-core

**baud's terminal core: the bytes a program writes, parsed and drawn into a grid of cells.**

It implements the control functions of ECMA-48, the DEC terminal manuals and xterm's control sequences, with scrollback, reflow on resize, selection and search. It is `no_std` and allocates nothing unless its `alloc` feature is on.

It is part of [baud](https://github.com/i-am-logger/baud), a terminal emulator, and is published so that
`cargo install baud` can build it. Its API follows baud's needs and may
change in any release.

Licensed under CC BY-NC-SA 4.0 (`LICENSE`).
