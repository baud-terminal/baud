# **The container the README session ssh's into**
# (`baud-app/examples/demo/container.rs`): busybox's shell and tools,
# OpenSSH's sshd, fastfetch and btop, from aarch64-linux nixpkgs, in an
# image made by nixpkgs' own `dockerTools.streamLayeredImage`, with baud's
# mark as a picture for fastfetch to draw.
#
# `pkgs` is the build host's nixpkgs and `linux` the same revision for
# aarch64-linux, the system colima's VM runs on an Apple silicon Mac. The
# image takes only the four packages from `linux`, which cache.nixos.org
# holds, and makes everything else -- its files, its customisation layer,
# the mark's picture, the script that streams it -- with `pkgs`, so a Mac
# with no Linux builder makes it. Its files are written whole, not linked
# into the store, so each has its own mode: the customisation layer is a
# tar made under fakeroot, every entry owned by 0.
#
# The text of each file is beside this one, and the demo's tests read the
# same files (`include_str!`). The mark is the repository's own
# `assets/logo/baud-mark-amber-tight.svg`, the ❯ and the cursor tight to
# their height, drawn to a PNG 600 pixels wide by librsvg: fastfetch sends
# a picture's file as it is, in iTerm2's inline protocol, and this
# fastfetch decodes none itself.
{ pkgs, linux }:
let
  inherit (linux) busybox openssh fastfetchMinimal btop;
in
pkgs.dockerTools.streamLayeredImage {
  # The demo removes the image by this name when the session ends.
  name = "baud-demo";
  tag = "readme";
  # The image's processor, whatever the build host's.
  architecture = "arm64";
  extraCommands = ''
    install -d -m 0755 etc etc/ssh home home/baud home/baud/.config \
      home/baud/.config/btop home/baud/.config/fastfetch home/baud/.ssh run \
      usr usr/bin usr/sbin usr/share usr/share/baud var var/empty
    install -d -m 0700 root
    install -d -m 1777 tmp
    install -m 0644 ${./passwd} etc/passwd
    install -m 0644 ${./group} etc/group
    install -m 0600 ${./shadow} etc/shadow
    install -m 0644 ${./profile} etc/profile
    install -m 0644 ${./sshd_config} etc/ssh/sshd_config
    install -m 0644 ${./fastfetch.jsonc} home/baud/.config/fastfetch/config.jsonc
    install -m 0644 ${./btop.conf} home/baud/.config/btop/btop.conf
    ${pkgs.librsvg}/bin/rsvg-convert --width 600 \
      --output usr/share/baud/mark.png ${../../assets/logo/baud-mark-amber-tight.svg}
    chmod 0644 usr/share/baud/mark.png
    ln -s ${busybox}/bin bin
    ln -s ${fastfetchMinimal}/bin/fastfetch usr/bin/fastfetch
    ln -s ${btop}/bin/btop usr/bin/btop
    ln -s ${openssh}/bin/sshd usr/sbin/sshd
  '';
  config.Entrypoint = [
    "/usr/sbin/sshd"
    "-D"
    "-e"
  ];
  # For the provenance file: each package's store path.
  passthru.packages = { inherit busybox openssh fastfetchMinimal btop; };
}
