# baud-isa-x86-64

**baud's scan with AVX2, on x86-64.**

The AVX2 implementation of `baud-isa`'s operations, held against the portable one by its tests. It is `no_std`.

It is part of [baud](https://github.com/i-am-logger/baud), a terminal emulator, and is published so that
`cargo install baud` can build it. Its API follows baud's needs and may
change in any release.

Licensed under CC BY-NC-SA 4.0 (`LICENSE`).
