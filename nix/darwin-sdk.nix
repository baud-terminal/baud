# **The SDK baud links on macOS, 26 or later**, for the flake's package and
# the dev shell alike, so the installed app, a clone's build in the shell
# and a `cargo install` against Xcode all link one generation of SDK.
# AppKit lays a window out by the SDK a binary was linked against: against
# nixpkgs' default, 14.4, the title bar is the one from before macOS 26.
# The newest the nixpkgs has, 27 then 26; one with neither is refused
# rather than built against an older SDK. `crates/baud-app/tests/macos_floor.rs`
# holds the names to 26 or later and reads the linked SDK back.
pkgs:
pkgs.apple-sdk_27 or pkgs.apple-sdk_26
  or (throw "baud links an apple-sdk of 26 or later, and this nixpkgs has none")
