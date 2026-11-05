# The engines tools/baud-oracle links: HarfBuzz 12.3.0 and FreeType
# 2.14.3, the versions every committed oracle file in baud-font/tests/var/
# names, from one nixpkgs revision, and libxkbcommon 1.13.2, the version
# baud-xkb's fixtures were made with, from a second. The first is the
# commit that took FreeType to 2.14.3 while HarfBuzz was still 12.3.0;
# none of nixos-25.11,
# nixos-26.05 or nixpkgs-unstable carries the pair. Its HarfBuzz check
# phase runs a CoreText shaping test that fails against this host's
# CoreText, so the check is skipped; the library is built from the same
# source and configuration either way. libxkbcommon there is 1.13.1;
# 1.13.2 comes from nixpkgs 13e53762, the revision flake.lock pins.
#
#   nix-shell tools/shell.nix --run \
#     'cargo run --release --manifest-path tools/Cargo.toml --bin advance'
#
# cargo itself comes from the repository's dev shell.
let
  pkgs =
    import
      (fetchTarball {
        url = "https://github.com/NixOS/nixpkgs/archive/21622bac70c240e85777be65c5b65bac6c3f3553.tar.gz";
        sha256 = "0049mvk6qfwj9mgidfmrkpr4xz06jh2b1hkllkx397kp7hgq5sqh";
      })
      { };
  harfbuzz = pkgs.harfbuzz.overrideAttrs { doCheck = false; };
  xkb =
    import
      (fetchTarball {
        url = "https://github.com/NixOS/nixpkgs/archive/13e53762c5afaf384d4614e94527a9ccfafe41d0.tar.gz";
        sha256 = "0dihr83y60hkvizqlz53sqbvfqgv1hxyicqdn3vdgiddxw4by0d6";
      })
      { };
in
assert xkb.libxkbcommon.version == "1.13.2";
pkgs.mkShell {
  packages = [ pkgs.pkg-config ];
  buildInputs = [
    harfbuzz
    pkgs.freetype
    xkb.libxkbcommon
  ];
}
