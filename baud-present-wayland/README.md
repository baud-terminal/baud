# baud-present-wayland

**baud's window on Linux, over Wayland.**

A shared-memory window with its xdg role, drawn sharp at the scale the compositor prefers -- a fraction such as 1.5 included -- frames paced on the frame callback and kept to the screen's refresh where the compositor offers the protocols for it, the pointer, the clipboard and primary selection, an input method, and the keyboard through `baud-xkb`.

It is part of [baud](https://github.com/i-am-logger/baud), a terminal emulator, and is published so that
`cargo install baud` can build it. Its API follows baud's needs and may
change in any release.

Licensed under CC BY-NC-SA 4.0 (`LICENSE`).
