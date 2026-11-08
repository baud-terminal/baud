# **The demo's mpv** (`baud-app/examples/demo/run.rs`): nixpkgs' own mpv,
# wrapped as nixpkgs wraps it, built from mpv's master at a commit that
# holds two fixes to its terminal video:
#
# - 12d630d819 "terminal: serialize terminal output between the status line
#   and the VOs": the status line, written from the main thread, could land
#   inside a frame vo_kitty wrote from its own thread, and cut it;
# - 8e566099cb "vo_kitty: present frames through the terminal swapchain",
#   whose uninit ends its `ESC _ G a=d ;` with `ESC \`, the ST 0.41.0's
#   uninit left out, so the delete was a control string left open.
#
# The commit is 8e566099cb itself, eight commits after 12d630d819 on master
# (GitHub's compare of the two: "ahead", behind by 0). `postPatch` fails the
# build if the source lacks either: the lock the status line and the VOs
# take, and the ST after the delete in vo_kitty's uninit.
#
# `pkgs` is the nixpkgs the demo's other programs on the Mac come from,
# whose mpv-unwrapped is 0.41.0: this one is that recipe with its source
# replaced, and its dependencies are the ones cache.nixos.org holds there.
{ pkgs }:
let
  rev = "8e566099cbbbfbd9177c457ed26ac51223ba7303";
  unwrapped = pkgs.mpv-unwrapped.overrideAttrs (old: {
    version = "0.41.0-unstable-2026-10-10";
    src = pkgs.fetchFromGitHub {
      owner = "mpv-player";
      repo = "mpv";
      inherit rev;
      hash = "sha256-ON+dry5AF/KzhU4LexYFOSmNaWCT/PmhEfGn2Tnjz/k=";
    };
    # nixpkgs' own, but for the line its first substitution names, which
    # master spells with a strip and an escape: the build options left out
    # of the binary, so it names no build-time path.
    postPatch = ''
      substituteInPlace meson.build \
        --replace-fail "conf_data.set_quoted('CONFIGURATION', meson.build_options().strip().replace('\\\\', '\\\\\\\\'))" \
                       "conf_data.set_quoted('CONFIGURATION', '<omitted>')"
      pushd TOOLS
      mv mpv_identify.sh mpv_identify
      patchShebangs *.py *.sh
      mv mpv_identify mpv_identify.sh
      popd
      grep -q '^void terminal_lock_output(void);$' osdep/terminal.h
      grep -A1 'append_passthrough(p, out, KITTY_ESC_DELETE_ALL);' video/out/vo_kitty.c \
        | grep -q 'append_passthrough(p, out, KITTY_ESC_END);'
    '';
    passthru = old.passthru // {
      inherit rev;
    };
  });
in
(pkgs.mpv.override { mpv-unwrapped = unwrapped; }).overrideAttrs (old: {
  passthru = (old.passthru or { }) // {
    inherit rev;
    packages.mpv-unwrapped = unwrapped;
  };
})
