{ pkgs
, config
, lib
, ...
}:
let
  cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
  # The root manifest is a virtual workspace, so there is no `package` table to
  # read: the name is the repo, the shared metadata lives in workspace.package.
  packageName = "baud";
  packageVersion = cargoToml.workspace.package.version;
  packageDescription = cargoToml.workspace.package.description or "";

  # baud-bench is Linux-only (F_SETPIPE_SZ), so it is a workspace member but not
  # a default one. On Linux every cargo invocation covers it; elsewhere the
  # default members are the portable crates. CI runs on ubuntu-latest, so the
  # harness is never unchecked in the place that gates a merge.
  scope = lib.optionalString pkgs.stdenv.isLinux "--workspace";

  # The bare-metal gate. Building baud-core for a target with no OS and no
  # allocator is what actually proves #![no_std] and the default-off `alloc`
  # feature; a green build on the host proves neither.
  bareTarget = "x86_64-unknown-none";
in
{
  # Set root explicitly for flake compatibility
  devenv.root = lib.mkDefault (builtins.toString ./.);

  dotenv.enable = true;
  imports = [
    ./nix/rust.nix
  ];

  # Additional packages for development
  packages = [
    pkgs.git
    pkgs.pkg-config
    # audit_all.sh shells out to impl_audit.py; the gate must not depend on
    # whatever python happens to be on the host.
    pkgs.python3
  ];

  # Development scripts
  scripts.dev-test.exec = ''
    echo "Running tests..."
    RUSTFLAGS="-D warnings" cargo test --release ${scope} --all-features
  '';

  scripts.dev-fmt.exec = ''
    echo "Checking formatting..."
    treefmt --fail-on-change
  '';

  scripts.dev-lint.exec = ''
    echo "Running clippy..."
    cargo clippy --quiet ${scope} -- -D warnings
  '';

  scripts.dev-check.exec = ''
    echo "Checking compilation..."
    cargo check --quiet ${scope}
  '';

  scripts.dev-bare.exec = ''
    echo "Building baud-core for ${bareTarget} (no_std, no alloc)..."
    cargo build --release -p baud-core --target ${bareTarget}
  '';

  scripts.dev-audit.exec = ''
    echo "Running the type-system audit..."
    sh ./audit_all.sh
  '';

  scripts.dev-ci.exec = ''
    echo "Running full CI pipeline locally..."
    echo "=== fmt ==="
    treefmt --fail-on-change || { echo "FAILED: fmt"; exit 1; }
    echo "=== clippy ==="
    cargo clippy --quiet ${scope} -- -D warnings || { echo "FAILED: clippy"; exit 1; }
    echo "=== check ==="
    cargo check --quiet ${scope} || { echo "FAILED: check"; exit 1; }
    echo "=== test ==="
    RUSTFLAGS="-D warnings" cargo test --release --quiet ${scope} || { echo "FAILED: test"; exit 1; }
    echo "=== bare metal ==="
    cargo build --release --quiet -p baud-core --target ${bareTarget} || { echo "FAILED: bare"; exit 1; }
    echo "=== audit ==="
    sh ./audit_all.sh || { echo "FAILED: audit"; exit 1; }
    echo "=== ALL PASSED ==="
  '';

  scripts.dev-run.exec = ''
    echo "Running the PTY benchmark (Linux only)..."
    cargo run --release -p baud-bench
  '';

  scripts.dev-build.exec = ''
    echo "Building ${packageName}..."
    cargo build --release ${scope}
  '';

  # Environment variables
  env = {
    CARGO_TARGET_DIR = "./target";
  };

  # Development shell setup.
  #
  # The whole banner goes to stderr. `devenv shell -- <cmd>` runs this first, so
  # anything on stdout is prepended to that command's output - and CI runs
  # `devenv shell -- cargo metadata` as Swatinem/rust-cache's cmd-format probe,
  # where a banner in front of the JSON is unparseable. The action swallows that
  # error, keeps the step green, and caches nothing.
  enterShell = ''
    {
      clear
      ${pkgs.figlet}/bin/figlet "${packageName}"
      echo
      {
        ${pkgs.lib.optionalString (packageDescription != "") ''echo "• ${packageDescription}"''}
        echo -e "• \033[1mv${packageVersion}\033[0m"
        echo -e " \033[0;32m✓\033[0m Development environment ready"
      } | ${pkgs.boxes}/bin/boxes -d stone -a l -i none
      echo
      echo "Available scripts:"
      echo "  dev-ci        - Full pipeline (fmt + clippy + check + test + bare + audit)"
      echo "  dev-test      - Run tests"
      echo "  dev-fmt       - Check formatting"
      echo "  dev-lint      - Run clippy"
      echo "  dev-check     - Check compilation"
      echo "  dev-bare      - Build baud-core for ${bareTarget} (no_std gate)"
      echo "  dev-audit     - Run the type-system audit gate"
      echo "  dev-run       - Run the PTY benchmark (Linux only)"
      echo "  dev-build     - Build the workspace"
      echo ""
    } >&2
  '';

  # https://devenv.sh/integrations/treefmt/
  #
  # One formatter for the whole tree, not just Rust — a Nix or shell file that
  # CI reformats is as much a diff-noise source as an unformatted .rs. `treefmt`
  # is the single entry point for `dev-fmt`, `tasks."test:fmt"`, and the
  # pre-commit hook, so all three can only ever agree.
  treefmt = {
    enable = true;
    config = {
      settings.global.excludes = [
        ".devenv.flake.nix"
        ".devenv/"
      ];

      programs = {
        # Nix
        nixpkgs-fmt.enable = true;
        deadnix = {
          enable = true;
          no-underscore = true;
        };
        statix.enable = true;

        # Rust — use the devenv toolchain so the formatter understands the
        # same edition the compiler does.
        rustfmt = {
          enable = true;
          package = config.languages.rust.toolchainPackage;
        };

        # Shell
        shellcheck.enable = true;
        shfmt.enable = true;
      };
    };
  };

  # https://devenv.sh/git-hooks/
  git-hooks.settings.rust.cargoManifestPath = "./Cargo.toml";

  # Use the same Rust toolchain for git-hooks as for development
  git-hooks.tools = {
    cargo = lib.mkForce config.languages.rust.toolchainPackage;
    clippy = lib.mkForce config.languages.rust.toolchainPackage;
    rustfmt = lib.mkForce config.languages.rust.toolchainPackage;
  };

  # treefmt only. A clippy hook would run with its own default flags - no
  # --all-targets, no --all-features - which is a different cargo fingerprint
  # from the test:clippy task, so it compiles the crate under clippy a second
  # time and cannot fail anything that task does not. Formatting is the part
  # worth catching before the commit rather than after.
  git-hooks.hooks = {
    treefmt.enable = true;
  };

  # https://devenv.sh/tasks/
  #
  # These tasks ARE the check suite. `enterTest` runs them, so `devenv test`
  # locally and `devenv test` in .github/workflows/ci.yml execute the same
  # thing by construction — the workflow is a thin wrapper, and the two cannot
  # drift apart.
  #
  # The `after` edges are load-bearing. devenv runs tasks with no edge between
  # them concurrently, so without these, fmt, clippy, check and test all invoke
  # cargo at once and block on the single lock over ./target. Serialised, they
  # run cheapest-first — a formatting failure costs no compilation at all — and
  # each later task reuses what the previous one compiled.
  tasks = {
    "test:fmt" = {
      exec = "treefmt --fail-on-change";
    };

    "test:clippy" = {
      exec = "cargo clippy --quiet ${scope} -- -D warnings";
      after = [ "test:fmt" ];
    };

    "test:check" = {
      exec = "cargo check --quiet ${scope}";
      after = [ "test:clippy" ];
    };

    # --release is load-bearing, not an optimisation. baud-core/tests/perf_floor.rs
    # asserts throughput floors (100 MB/s spans, 70 MB/s cells); an unoptimised
    # build misses them by an order of magnitude, so the suite would fail on the
    # profile rather than on the code. BUILD.md runs the same tests in release.
    "test:unit" = {
      exec = "RUSTFLAGS='-D warnings' cargo test --release --quiet ${scope}";
      after = [ "test:check" ];
    };

    # The no_std proof. A host build links std through the harness crates and
    # so cannot fail on an accidental std dependency in baud-core; a target
    # with no OS and no allocator can and does.
    "test:bare" = {
      exec = "cargo build --release --quiet -p baud-core --target ${bareTarget}";
      after = [ "test:unit" ];
    };

    # The type-system gate: raw integers outside the newtype internals are a
    # bug waiting for two quantities to be confused. Per-file limits and the
    # reason for each live in audit_all.sh.
    "test:audit" = {
      exec = "sh ./audit_all.sh";
      after = [ "test:bare" ];
    };
  };

  # https://devenv.sh/tests/
  enterTest = lib.mkForce "devenv tasks run test:fmt test:clippy test:check test:unit test:bare test:audit";
}
