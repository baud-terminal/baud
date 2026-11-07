#!/usr/bin/env bash
# **Every package of the flake evaluates here, for every system, with
# import-from-derivation off.** A NixOS configuration evaluated on a Mac for a
# Linux host reads `packages.${system}.baud`, and a nixpkgs that builds during
# evaluation asks that Mac to build for Linux. Nothing is built here: a drvPath
# is the whole of what is asked for, so a machine with no builder for a system
# answers for it.
#
# `baud-app/tests/install.rs` holds the shape of `flake.nix` without nix; this
# is the check by nix. Fetches nixpkgs on a cold store, then takes seconds.
set -euo pipefail
cd "$(dirname "$0")/.." || exit 1

off=(--option allow-import-from-derivation false --no-update-lock-file)

# The control: the option refuses a build during evaluation, so a pass below
# means nothing asked for one.
probe='builtins.readFile (derivation { name = "ifd"; system = "x"; builder = "x"; })'
if said=$(nix eval "${off[@]}" --expr "$probe" 2>&1) ||
  [[ $said != *allow-import-from-derivation* ]]; then
  echo "the control: nix did not refuse a build during evaluation: $said" >&2
  exit 1
fi

each='builtins.mapAttrs (_: builtins.mapAttrs (_: p: p.drvPath))'
paths=$(nix eval "${off[@]}" --json .#packages --apply "$each")
for system in aarch64-darwin aarch64-linux x86_64-linux; do
  if [[ $paths != *"\"$system\":{"* ]]; then
    echo "the flake has no packages for $system: $paths" >&2
    exit 1
  fi
done
echo "$paths"
