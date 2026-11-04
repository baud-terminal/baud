{
  # `nix run github:baud-terminal/baud` opens baud; `nix build` leaves it in
  # ./result/bin. The dev shell is devenv's (`devenv.nix`); this file only
  # builds the terminal.
  description = "baud, a fast terminal emulator";

  # The same nixpkgs and rust-overlay revisions as `devenv.lock`, so the
  # package builds with the compiler the repository is built and tested
  # with. `flake.lock` holds them, and `baud-app/tests/install.rs` fails
  # when it and `devenv.lock` disagree.
  inputs = {
    nixpkgs.url = "github:cachix/devenv-nixpkgs/rolling";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs
    , rust-overlay
    , ...
    }:
    let
      inherit (nixpkgs) lib;
      # Where baud runs: macOS on Apple silicon, and Linux.
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAll = f: lib.genAttrs systems (system: f (pkgsFor system));
      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
          # devenv-nixpkgs allows unfree packages already; this names the
          # one the flake means, and keeps holding on NixOS's nixpkgs.
          config.allowUnfreePredicate = pkg: lib.getName pkg == "baud";
        };

      cargoToml = lib.importTOML ./Cargo.toml;

      baud =
        pkgs:
        let
          # The nightly `nix/rust.nix` takes from that rust-overlay revision:
          # rustc 1.96.0-nightly 02c7f9bec. baud-domain and baud-font use
          # `#![feature]`s, and the manifests' `cargo-features`, so no stable
          # release builds them. CONTRIBUTING.md and docs/embedding.md name the same date.
          toolchain = pkgs.rust-bin.nightly."2026-04-11".minimal;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
        in
        rustPlatform.buildRustPackage {
          pname = "baud";
          inherit (cargoToml.workspace.package) version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # The one binary; the other members are the tests and benchmarks.
          cargoBuildFlags = [
            "-p"
            "baud"
          ];
          # The suite runs in the dev shell and in CI. Here it would open
          # ptys and wait on a display it does not have.
          doCheck = false;
          # **The binary the repository measures.** nixpkgs' cargo setup
          # hook appends `-Cforce-frame-pointers=yes` to the target's
          # `rustflags` in `.cargo/config.toml`, which keeps a frame pointer
          # in every function: 6,080 more bytes of `__text` than the
          # workspace build, on this nightly. A set `RUSTFLAGS` replaces a
          # config's `rustflags`, as devenv's does, and the profile's
          # `rustflags`, the function alignment, still apply.
          env.RUSTFLAGS = "";

          # The fonts baud draws with are inside the binary, and their
          # licences travel with it, as the app bundle carries them.
          postInstall = ''
            fonts=$out/share/doc/baud/fonts
            for d in built glyphs/languages/hebrew glyphs/languages/devanagari jetbrains-mono symbols-nerd-font stix-two-math; do
              mkdir -p "$fonts/$d"
            done
            cp baud-app/fonts/built/{OFL.txt,FONTLOG.txt,AUTHORS.txt,MANIFEST} "$fonts/built/"
            cp baud-app/fonts/glyphs/languages/hebrew/{OFL.txt,SOURCE} "$fonts/glyphs/languages/hebrew/"
            cp baud-app/fonts/glyphs/languages/devanagari/SOURCE "$fonts/glyphs/languages/devanagari/"
            cp baud-app/fonts/jetbrains-mono/{OFL.txt,AUTHORS.txt,SOURCE} "$fonts/jetbrains-mono/"
            cp baud-app/fonts/symbols-nerd-font/{LICENSE,README.md,SOURCE} "$fonts/symbols-nerd-font/"
            cp -R baud-app/fonts/symbols-nerd-font/icon-licences "$fonts/symbols-nerd-font/"
            cp baud-app/fonts/stix-two-math/{OFL.txt,SOURCE} "$fonts/stix-two-math/"
            cp LICENSE THIRD_PARTY.md "$out/share/doc/baud/"
          '';

          meta = {
            inherit (cargoToml.workspace.package) description;
            homepage = cargoToml.workspace.package.repository;
            # Free for anything that is not commercial; nixpkgs counts it as
            # unfree, so a nixpkgs that takes this package through the
            # overlay has to allow it.
            license = lib.licenses.cc-by-nc-sa-40;
            mainProgram = "baud";
            platforms = systems;
          };
        };
    in
    {
      packages = forAll (pkgs: {
        default = baud pkgs;
        baud = baud pkgs;
      });

      # For a configuration that builds baud with its own nixpkgs.
      overlays.default = lib.composeExtensions rust-overlay.overlays.default (final: _: { baud = baud final; });
    };
}
