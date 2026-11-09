{
  # `nix run github:baud-terminal/baud` opens baud; `nix build` leaves it in
  # ./result/bin, and the app beside it: on macOS ./result/Applications/
  # baud.app, on Linux a desktop entry and its icons under ./result/share.
  # The dev shell is devenv's (`devenv.nix`); this file only builds the
  # terminal.
  description = "baud, a fast terminal emulator";

  # **The package takes nixpkgs itself, at the revision the dev shell's is
  # made from.** The shell reads `cachix/devenv-nixpkgs`, which patches this
  # same revision (`nixpkgs-src` in `devenv.lock`) in a derivation for the
  # system asked for. Importing that needs a builder for that system during
  # evaluation, which a configuration evaluated on another machine, or with
  # `allow-import-from-derivation = false`, does not have. On aarch64-darwin,
  # the one system both evaluate for here, the package's stdenv and rustc are
  # the same derivations without its patches. rust-overlay is at
  # `devenv.lock`'s revision too, so the package builds with the compiler the
  # repository is built and tested with. `flake.lock` holds them, and
  # `baud-app/tests/install.rs` fails when
  # it and `devenv.lock` disagree. `scripts/check-flake-eval.sh` evaluates
  # every package with import-from-derivation off.
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/674a2028661e550a97400c1cc992b9cca9a668a1";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # **The README session's container** (`nix/demo-container/`): its
    # aarch64-linux packages, starship 1.27.0 among them, from the
    # nixpkgs-unstable commit the demo takes its own starship from
    # (`baud-app/examples/demo/container.rs`). The image's pin holds their
    # store paths, and btop and fastfetch differ at the revision above; only
    # the container's image reads this input.
    nixpkgs-demo.url = "github:NixOS/nixpkgs/6dadaf4375be35033e1c84395adbd53ff5fded96";
    # **The demo's programs on the Mac** (`NIXPKGS` in
    # `baud-app/examples/demo/run.rs`): only the demo's mpv
    # (`nix/demo-mpv/`) reads this input, its recipe and its dependencies.
    nixpkgs-demo-mac.url = "github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a";
  };

  outputs =
    { self
    , nixpkgs
    , rust-overlay
    , nixpkgs-demo
    , nixpkgs-demo-mac
    , ...
    }:
    let
      inherit (nixpkgs) lib;
      # Where baud builds: macOS on Apple silicon, where it runs, and Linux,
      # where it has run only on headless Wayland compositors.
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
          # nixpkgs counts baud's licence as unfree and refuses the package
          # unless it is allowed; this allows baud and nothing else.
          config.allowUnfreePredicate = pkg: lib.getName pkg == "baud";
        };

      cargoToml = lib.importTOML ./Cargo.toml;

      # The SDK baud links on macOS, 26 or later (see `buildInputs` below);
      # a nixpkgs with none is refused rather than built against an older one.
      darwinSdk =
        pkgs:
          pkgs.apple-sdk_27 or pkgs.apple-sdk_26
            or (throw "baud links an apple-sdk of 26 or later, and this nixpkgs has none");

      baud =
        pkgs:
        let
          # The nightly `nix/rust.nix` takes from that rust-overlay revision:
          # rustc 1.101.0-nightly 249c1e4e3. baud-domain and baud-font use
          # `#![feature]`s, and the manifests' `cargo-features`, so no stable
          # release builds them. docs/embedding.md names the same date.
          toolchain = pkgs.rust-bin.nightly."2026-11-06".minimal;
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
          # **The build's name** (`baud-app/build.rs`). A nix build has no
          # `.git`, so the flake says which commit this is; `dirtyShortRev`
          # is its answer for a tree that differs from the commit.
          env.BAUD_GIT_REV = self.shortRev or self.dirtyShortRev or "unknown";
          # **And when that commit was made**, the About window's age line's
          # start: `lastModified`, the commit's time for a git flake; `0`,
          # which the build reads as no time, where nix has none.
          env.BAUD_GIT_TIME = toString (self.lastModified or 0);

          # The Linux desktop entry is checked against the specification as
          # it is installed; the macOS bundle is signed ad hoc, as a clone's
          # build signs it, by a signer nixpkgs builds (no Xcode tool is
          # reachable from a build).
          nativeBuildInputs =
            lib.optionals pkgs.stdenv.hostPlatform.isLinux [ pkgs.desktop-file-utils ]
            ++ lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ pkgs.rcodesign ];

          # **The SDK the app links, 26 or later**, as `cargo install` links
          # Xcode's. AppKit lays a window out by the SDK a binary was linked
          # against: linked against nixpkgs' default, 14.4, the title bar is
          # the one from before macOS 26, its traffic lights ending at 69
          # points; linked against Xcode's 27.2 it is 32 points tall and they
          # end at 78 (`baud-app/tests/titlebar_mark.rs` measures both). The
          # newest at the pin, 27.0, is nearest the 27.2 a clone's build
          # takes.
          # `checks.<darwin>.sdk` reads the linked SDK back.
          buildInputs = lib.optionals pkgs.stdenv.hostPlatform.isDarwin [ (darwinSdk pkgs) ];

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
            linux = nixpkgs-demo.legacyPackages.aarch64-linux;
          };
          # **The demo's mpv** (`nix/demo-mpv/`): nixpkgs' mpv built from
          # mpv's master at a commit with its two terminal video fixes.
          aarch64-darwin.demo-mpv = import ./nix/demo-mpv {
            pkgs = nixpkgs-demo-mac.legacyPackages.aarch64-darwin;
          };
        };

      # **The app's linked SDK, read back**: `nix flake check` (or `nix
      # build .#checks.aarch64-darwin.sdk`) fails if the binary's
      # LC_BUILD_VERSION names an SDK below 26, so the installed app and a
      # `cargo install` cannot drift back apart.
      checks = lib.genAttrs (lib.filter (lib.hasSuffix "darwin") systems) (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          sdk = pkgs.runCommand "baud-linked-sdk" { nativeBuildInputs = [ pkgs.cctools ]; } ''
            bin=${baud pkgs}/Applications/baud.app/Contents/MacOS/baud
            sdk=$(otool -l "$bin" | awk '/cmd LC_BUILD_VERSION/ { f = 1 } f && $1 == "sdk" { print $2; exit }')
            echo "baud links SDK $sdk"
            major=''${sdk%%.*}
            if [ -z "$major" ] || [ "$major" -lt 26 ]; then
              echo "the app links SDK '$sdk', below 26" >&2
              exit 1
            fi
            echo "$sdk" > $out
          '';
        }
      );

      # For a configuration that builds baud with its own nixpkgs.
      overlays.default = lib.composeExtensions rust-overlay.overlays.default (final: _: { baud = baud final; });
    };
}
