#!/usr/bin/env bash
# Prints, as `export` lines, the other terminals and the compositor the Linux
# comparisons run (`vt-compare-linux.sh`, `linux-pty.sh`, `linux-kp1.sh`):
# each from one nixpkgs revision, the build that distribution ships, so a
# second machine runs the same binaries. Needs nix with flakes.
#
#   eval "$(scripts/linux-terminals.sh)"
#
# The versions are in the store paths it prints.
# NIXPKGS overrides the revision.
set -euo pipefail
rev=${NIXPKGS:-d5cc6833999746cffdfe0598f7bc801a57b887a9}
attrs=(sway-unwrapped foot alacritty kitty ghostty wezterm dejavu_fonts)
mapfile -t paths < <(nix build --no-link --print-out-paths \
  "${attrs[@]/#/github:NixOS/nixpkgs/$rev#}")
[ "${#paths[@]}" = "${#attrs[@]}" ] || {
  echo "nix built ${#paths[@]} of ${#attrs[@]}" >&2
  exit 1
}
echo "export NIXPKGS_REV=$rev"
echo "export SWAY=${paths[0]}/bin/sway"
echo "export FOOT=${paths[1]}/bin/foot"
echo "export ALACRITTY=${paths[2]}/bin/alacritty"
echo "export KITTY=${paths[3]}/bin/kitty"
echo "export GHOSTTY=${paths[4]}/bin/ghostty"
echo "export WEZTERM=${paths[5]}/bin/wezterm"
echo "export FONT_DIR=${paths[6]}/share/fonts/truetype"
