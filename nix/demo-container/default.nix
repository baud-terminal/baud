# **The container the README session ssh's into**
# (`baud-app/examples/demo/container.rs`): busybox's shell and tools,
# OpenSSH's sshd and fastfetch, from aarch64-linux nixpkgs, in an image
# made by nixpkgs' own `dockerTools.streamLayeredImage`.
#
# `pkgs` is the build host's nixpkgs and `linux` the same revision for
# aarch64-linux, the system colima's VM runs on an Apple silicon Mac. The
# image takes only the three packages from `linux`, which cache.nixos.org
# holds, and makes everything else -- its files, its customisation layer,
# the script that streams it -- with `pkgs`, so a Mac with no Linux
# builder makes it. Its files are written whole, not linked into the
# store, so each has its own mode: the customisation layer is a tar made
# under fakeroot, every entry owned by 0.
#
# The text of each file is beside this one, and the demo's tests read the
# same files (`include_str!`).
{ pkgs, linux }:
let
  inherit (linux) busybox openssh fastfetchMinimal;
in
pkgs.dockerTools.streamLayeredImage {
  # The demo removes the image by this name when the session ends.
  name = "baud-demo";
  tag = "readme";
  # The image's processor, whatever the build host's.
  architecture = "arm64";
  extraCommands = ''
    install -d -m 0755 etc etc/ssh home home/baud home/baud/.config \
      home/baud/.config/fastfetch home/baud/.ssh run usr usr/bin usr/sbin \
      var var/empty
    install -d -m 0700 root
    install -d -m 1777 tmp
    install -m 0644 ${./passwd} etc/passwd
    install -m 0644 ${./group} etc/group
    install -m 0600 ${./shadow} etc/shadow
    install -m 0644 ${./profile} etc/profile
    install -m 0644 ${./sshd_config} etc/ssh/sshd_config
    install -m 0644 ${./fastfetch.jsonc} home/baud/.config/fastfetch/config.jsonc
    ln -s ${busybox}/bin bin
    ln -s ${fastfetchMinimal}/bin/fastfetch usr/bin/fastfetch
    ln -s ${openssh}/bin/sshd usr/sbin/sshd
  '';
  config.Entrypoint = [
    "/usr/sbin/sshd"
    "-D"
    "-e"
  ];
  # For the provenance file: each package's store path.
  passthru.packages = { inherit busybox openssh fastfetchMinimal; };
}
