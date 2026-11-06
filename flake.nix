{
  # `nix run github:baud-terminal/baud` opens baud; `nix build` leaves it in
  # ./result/bin, and the app beside it: on macOS ./result/Applications/
  # baud.app, on Linux a desktop entry and its icons under ./result/share.
  # The dev shell is devenv's (`devenv.nix`); this file only builds the
  # terminal.
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
    # **The README session's prompt** (`nix/demo-container/`): starship
    # 1.27.0 and the bash it runs under, from a nixpkgs-unstable revision,
    # pinned here by its commit. The revision above carries starship 1.24.2,
    # inside two advisories. The demo takes its own starship from the same
    # commit (`baud-app/examples/demo/container.rs`), and only the container's
    # image reads this input.
    nixpkgs-demo.url = "github:NixOS/nixpkgs/13e53762c5afaf384d4614e94527a9ccfafe41d0";
  };

  outputs =
    { nixpkgs
    , rust-overlay
    , nixpkgs-demo
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
          # release builds them. docs/embedding.md names the same date.
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
          # The suite runs in the dev shell (`devenv test`). Here it would open
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

          # The Linux desktop entry is checked against the specification as
          # it is installed; the macOS bundle is signed ad hoc, as a clone's
          # build signs it, by a signer nixpkgs builds (no Xcode tool is
          # reachable from a build).
          nativeBuildInputs =
            lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.desktop-file-utils ]
            ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.rcodesign ];

          # **The app**, assembled by the script a clone's build uses, so the
          # two bundles carry the same Info.plist, icon, fonts' licences and
          # binary (the shaders' library is inside it), after the fixup has
          # stripped and signed `bin/baud`, which it does to `bin` and not to
          # `Applications`. `bin/baud` is then the bundle's executable, so
          # `nix run` starts the binary the Dock does, and the bundle is
          # signed last, ad hoc, as `scripts/bundle-macos.sh` signs one.
          postFixup = lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
            app=$out/Applications/baud.app
            bash scripts/bundle-macos.sh --assemble "$out/bin/baud" "$app"
            rm "$out/bin/baud"
            ln -s ../Applications/baud.app/Contents/MacOS/baud "$out/bin/baud"
            rcodesign sign "$app"
          '';

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
          ''
          # **A desktop entry**, named for the window's Wayland app_id
          # (`org.baud.terminal`, `baud-app/src/linux.rs`), which is how a
          # desktop finds a window's entry, and the icon at the sizes drawn.
          + lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
            install -Dm644 packaging/linux/org.baud.terminal.desktop \
              "$out/share/applications/org.baud.terminal.desktop"
            desktop-file-validate "$out/share/applications/org.baud.terminal.desktop"
            for n in 16 32 64 128 256 512; do
              install -Dm644 "assets/logo/baud-icon-dock-$n.png" \
                "$out/share/icons/hicolor/''${n}x''${n}/apps/org.baud.terminal.png"
            done
            install -Dm644 assets/logo/baud-icon-dock.svg \
              "$out/share/icons/hicolor/scalable/apps/org.baud.terminal.svg"
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
      packages = lib.recursiveUpdate
        (forAll (pkgs: {
          default = baud pkgs;
          baud = baud pkgs;
        }))
        {
          # **The README session's container** (`nix/demo-container/`): `nix
          # build .#demo-container` leaves the script that streams its
          # image, which the demo writes to a file, checks against its pin
          # and loads. aarch64-darwin only, the Mac the demo runs on; its
          # packages are aarch64-linux ones cache.nixos.org holds.
          aarch64-darwin.demo-container = import ./nix/demo-container {
            pkgs = pkgsFor "aarch64-darwin";
            linux = pkgsFor "aarch64-linux";
            prompt = nixpkgs-demo.legacyPackages.aarch64-linux;
          };
        };

      # For a configuration that builds baud with its own nixpkgs.
      overlays.default = lib.composeExtensions rust-overlay.overlays.default (final: _: { baud = baud final; });
    };
}
