# The engines tools/baud-oracle links, from one nixpkgs revision: HarfBuzz
# 12.3.0 and FreeType 2.14.3, the versions every committed oracle file in
# baud-font/tests/var/ names. This revision is the commit that took
# FreeType to 2.14.3 while HarfBuzz was still 12.3.0; none of nixos-25.11,
# nixos-26.05 or nixpkgs-unstable carries the pair. Its HarfBuzz check
# phase runs a CoreText shaping test that fails against this host's
# CoreText, so the check is skipped; the library is built from the same
# source and configuration either way.
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
in
pkgs.mkShell {
  packages = [ pkgs.pkg-config ];
  buildInputs = [
    harfbuzz
    pkgs.freetype
  ];
}
