# baud-wayland

**The Wayland protocol, spoken directly over the socket, with no libwayland.**

The interface tables are generated at build time from the protocol XMLs it carries in `protocols/`, which keep their own notices.

It is part of [baud](https://github.com/baud-terminal/baud), a terminal emulator, and is one of
the crates baud is built from. Its API follows baud's needs and may change
in any release.

Licensed under CC BY-NC-SA 4.0 (`LICENSE`); `NOTICE` names the third-party material it carries, each part's licence and where its notice is.
