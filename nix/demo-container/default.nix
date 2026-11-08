# **The container the README session ssh's into**
# (`baud-app/examples/demo/container.rs`): busybox's tools, bash as the
# login shell with starship drawing baud's prompt, OpenSSH's sshd,
# fastfetch, btop and helix, from aarch64-linux nixpkgs, in an image made by
# nixpkgs' own `dockerTools.streamLayeredImage`, with baud's mark as a
# picture for fastfetch to draw.
#
# `pkgs` is the build host's nixpkgs and `linux` the nixpkgs-unstable
# revision the flake pins for aarch64-linux, the system colima's VM runs
# on an Apple silicon Mac, with starship 1.27.0, the first release outside
# advisories GHSA-679r-jwc5-r5m3 and GHSA-x486-6jjc-phfp. The image takes
# only seven packages from it, which cache.nixos.org holds, and makes
# everything else -- its files,
# its customisation layer, the mark's picture, the prompt's config, the
# script that streams it -- with `pkgs`, so a Mac with no Linux builder
# makes it. Its files are written whole, not linked into the store, so
# each has its own mode: the customisation layer is a tar made under
# fakeroot, every entry owned by 0 but the user's home, which is the
# user's, as a home is: starship marks a folder its user cannot write.
#
# The text of each file is beside this one, and the demo's tests read the
# same files (`include_str!`). The mark is the repository's own
# `assets/logo/baud-mark-amber-tight.svg`, the ❯ and the cursor tight to
# their height, drawn to a PNG 600 pixels wide by librsvg: fastfetch sends
# a picture's file as it is, in iTerm2's inline protocol, and this
# fastfetch decodes none itself.
#
# The prompt's config is the repository's `assets/starship/baud.toml` with
# three modules off -- the user and the host it shows over ssh on a line
# of their own, and the container's name it shows in docker -- so the
# prompt there reads `~ ❯`, as baud's own shell does. The session's badge
# names the server while it is in.
#
# helix reads `helix.toml` beside this file, the config the demo's own
# helix reads too: its defaults but soft wrap. The files the session
# shows in the home -- the README at the take's commit and `hello.txt` --
# are copied in as it starts (`container::home_files`), not built in, so
# the image's pin does not follow every edit of the README.
{ pkgs, linux }:
let
  inherit (linux) busybox openssh fastfetch-unwrapped btop starship bashInteractive helix;
  starshipConfig = (pkgs.formats.toml { }).generate "starship.toml" (
    pkgs.lib.recursiveUpdate (pkgs.lib.importTOML ../../assets/starship/baud.toml) {
      username.disabled = true;
      hostname.disabled = true;
      container.disabled = true;
    }
  );
in
pkgs.dockerTools.streamLayeredImage {
  # The demo removes the image by this name when the session ends.
  name = "baud-demo";
  tag = "readme";
  # The image's processor, whatever the build host's.
  architecture = "arm64";
  extraCommands = ''
    install -d -m 0755 etc etc/ssh home home/jordan home/jordan/.config \
      home/jordan/.config/btop home/jordan/.config/fastfetch home/jordan/.config/helix home/jordan/.ssh run \
      usr usr/bin usr/sbin usr/share usr/share/baud var var/empty
    install -d -m 0700 root
    install -d -m 1777 tmp
    install -m 0644 ${./passwd} etc/passwd
    install -m 0644 ${./group} etc/group
    install -m 0600 ${./shadow} etc/shadow
    install -m 0644 ${./profile} etc/profile
    install -m 0644 ${./sshd_config} etc/ssh/sshd_config
    install -m 0644 ${starshipConfig} etc/starship.toml
    install -m 0644 ${./fastfetch.jsonc} home/jordan/.config/fastfetch/config.jsonc
    install -m 0644 ${./btop.conf} home/jordan/.config/btop/btop.conf
    install -m 0644 ${./helix.toml} home/jordan/.config/helix/config.toml
    ${pkgs.librsvg}/bin/rsvg-convert --width 600 \
      --output usr/share/baud/mark.png ${../../assets/logo/baud-mark-amber-tight.svg}
    chmod 0644 usr/share/baud/mark.png
    ln -s ${busybox}/bin bin
    ln -s ${bashInteractive}/bin/bash usr/bin/bash
    ln -s ${starship}/bin/starship usr/bin/starship
    ln -s ${fastfetch-unwrapped}/bin/fastfetch usr/bin/fastfetch
    ln -s ${btop}/bin/btop usr/bin/btop
    ln -s ${helix}/bin/hx usr/bin/hx
    ln -s ${openssh}/bin/sshd usr/sbin/sshd
  '';
  # The user's home, the user's: uid and gid 1000, as `passwd` names them.
  fakeRootCommands = ''
    chown 1000:1000 home/jordan
  '';
  config.Entrypoint = [
    "/usr/sbin/sshd"
    "-D"
    "-e"
  ];
  # For the provenance file: each package's store path.
  passthru.packages = { inherit busybox bashInteractive starship openssh fastfetch-unwrapped btop helix; };
}
