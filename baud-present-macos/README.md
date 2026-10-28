# baud-present-macos

**baud's window on macOS: the frame drawn into a glass window, in time with the display.**

An IOSurface-backed view inside a `macos-liquid-glass` window, drawn by Metal or the CPU, with frames paced on the display link and AppKit's keys, mouse and text input.

It is part of [baud](https://github.com/i-am-logger/baud), a terminal emulator, and is published so that
`cargo install baud` can build it. Its API follows baud's needs and may
change in any release.

Licensed under CC BY-NC-SA 4.0 (`LICENSE`).
