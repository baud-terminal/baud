# hb-shape, the oracle scripts/gen-shape-fixtures.sh asks, at the version
# each baud-font/tests/shape/*.txt header names. Two pins, because the
# fixtures were made under two: 14.5.1 (the Devanagari, Hebrew, Arabic,
# script and order files) and 13.2.1 (gsub, gpos, fallback, morx and
# brackets). Each fixture is kept only from the run under the version its
# header names: a full run of the script rewrites every file under the
# one hb-shape on PATH. The shell refuses a version it has no pin for, and
# asserts the pinned revision's harfbuzz is that version.
#
#   14.5.1: nixpkgs 13e53762, the revision flake.lock pins; its harfbuzz
#           is the derivation the script's Devanagari note names.
#   13.2.1: nixos-26.05 at 7dcdd873, the same tarball, patch and meson
#           flags as the build that made the 13.2.1 files.
#
#   nix-shell tools/hb-shape.nix --argstr harfbuzz 14.5.1 --run \
#     'BAUD_ORACLE=<bin> scripts/gen-shape-fixtures.sh devanagari'
#
# BAUD_ORACLE is tools/baud-oracle's baud-oracle bin, built in
# tools/shell.nix.
{ harfbuzz ? "14.5.1"
,
}:
let
  pins = {
    "14.5.1" = {
      rev = "13e53762c5afaf384d4614e94527a9ccfafe41d0";
      sha256 = "0dihr83y60hkvizqlz53sqbvfqgv1hxyicqdn3vdgiddxw4by0d6";
    };
    "13.2.1" = {
      rev = "7dcdd87373d72e7a2173e4db02e20d3d7f495cfd";
      sha256 = "09f5ys7xly8gaip09yxpp1xawqm4dlzrjq0ib8x80y9wsdpnp2xl";
    };
  };
  pin = pins.${harfbuzz} or (throw "no hb-shape pin for HarfBuzz ${harfbuzz}");
  pkgs =
    import
      (fetchTarball {
        url = "https://github.com/NixOS/nixpkgs/archive/${pin.rev}.tar.gz";
        inherit (pin) sha256;
      })
      { };
in
assert pkgs.harfbuzz.version == harfbuzz;
pkgs.mkShell {
  packages = [ pkgs.harfbuzz.dev ];
}
