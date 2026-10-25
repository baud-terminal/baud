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

  # Explicit rather than implied by default-members, so adding a crate that is
  # not a default member cannot quietly drop out of the check suite.
  scope = "--workspace";

  # The bare-metal gate. Building baud-core for a target with no OS and no
  # allocator is what actually proves #![no_std] and the default-off `alloc`
  # feature; a green build on the host proves neither.
  bareTarget = "x86_64-unknown-none";
  # The Linux gate. `baud-app`'s Wayland driver is `cfg(target_os = "linux")`,
  # so a host build on this Mac never compiles it; the Linux standard library
  # `nix/rust.nix` fetches lets clippy type-check it here. A module nothing
  # builds does not go red, it goes absent.
  linuxTarget = "aarch64-unknown-linux-gnu";
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
    # `test:unit` runs the tests through nextest (the owner's call,
    # 2026-10-07): `cargo test` runs the ~120 test binaries one after
    # another, so a binary with one long test left the other cores idle until
    # it finished. nextest runs every test of every binary across all cores,
    # each in its own process. It does not run doctests, so `test:unit` runs
    # those after it with `cargo test --doc`.
    pkgs.cargo-nextest
  ];

  # Development scripts
  #
  # **No `--all-features`.** It turned on every ceiling and probe arm at once,
  # and the arms are wrong by construction and stub one another out: on
  # 5f1d5f0 baud-core's lib did not compile that way under `-D warnings`
  # (an unreachable statement, an unused `mut`, an unused parameter, two
  # methods never called), so the script was red on a green tree.
  # `test:features` checks each arm alone, which is how an arm is used.
  scripts.dev-test.exec = ''
    echo "Running tests..."
    RUSTFLAGS="-D warnings" cargo nextest run --release --no-fail-fast ${scope}; a=$?
    RUSTFLAGS="-D warnings" cargo test --release --doc ${scope}; b=$?
    [ "$a" -eq 0 ] && [ "$b" -eq 0 ]
  '';

  scripts.dev-fmt.exec = ''
    echo "Checking formatting..."
    treefmt --fail-on-change
  '';

  # **`--all-targets`, because without it clippy sees neither `tests/` nor a
  # `#[cfg(test)] mod`.** The gate was green for as long as it has existed
  # while `cargo clippy --workspace --all-targets` carried 13 errors -- five
  # `format!`s with nothing to interpolate, two hand-rolled range tests, three
  # loops indexing what they could iterate, two tuple types past the
  # complexity bar, and a test module with six functions hidden behind it.
  # That is this repo's gate-scoped-to-less-than-it-appears failure, in the
  # one gate whose whole job is to read every line that compiles.
  scripts.dev-lint.exec = ''
    echo "Running clippy..."
    cargo clippy --quiet ${scope} --all-targets -- -D warnings
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
    cargo clippy --quiet ${scope} --all-targets -- -D warnings || { echo "FAILED: clippy"; exit 1; }
    echo "=== check ==="
    # --all-targets, because a plain check builds neither benches nor
    # examples and `cargo test` does not build a bench target either. Three
    # commits landed green over a `baud-bench` bench that did not compile
    # before this was noticed; `scripts/check-features.sh` had already been
    # caught by the same gap and fixed only for itself.
    cargo check --quiet --all-targets ${scope} || { echo "FAILED: check"; exit 1; }
    echo "=== test ==="
    RUSTFLAGS="-D warnings" cargo nextest run --release --cargo-quiet --no-fail-fast ${scope}; a=$?
    RUSTFLAGS="-D warnings" cargo test --release --quiet --doc ${scope}; b=$?
    [ "$a" -eq 0 ] && [ "$b" -eq 0 ] || { echo "FAILED: test"; exit 1; }
    echo "=== bare metal ==="
    cargo build --release --quiet -p baud-core --target ${bareTarget} || { echo "FAILED: bare"; exit 1; }
    echo "=== linux ==="
    cargo clippy --quiet -p baud-app --target ${linuxTarget} --all-targets -- -D warnings || { echo "FAILED: linux"; exit 1; }
    echo "=== sources ==="
    bash ./scripts/check-sources.sh || { echo "FAILED: sources"; exit 1; }
    echo "=== audit ==="
    sh ./audit_all.sh || { echo "FAILED: audit"; exit 1; }
    echo "=== ALL PASSED ==="
  '';

  # `cargo run -p baud-bench` named no target from the commit that deleted
  # `baud-core2` (its default bin was the span-model PTY harness), and the
  # measurement programs are `[[bench]]` targets now, so this names one.
  scripts.dev-bench.exec = ''
    echo "Ingest throughput, in memory, on the recording named or cat-cjk..."
    cargo bench -p baud-bench --bench ingest2 -- "$@"
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
      echo "  dev-bench     - Ingest throughput in memory (cargo bench, ingest2)"
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
      # **The tree root is found from the working directory, not baked in.**
      # Left empty, devenv's module writes `--tree-root <the checkout that
      # evaluated the shell>` into the wrapper, after "$@" so no caller can
      # override it, so from an agent worktree the main checkout's shell
      # would format the main checkout's copies. Non-empty, the module passes
      # `--tree-root-file` instead and treefmt walks up from the working
      # directory to the first directory holding this file: the checkout you
      # are in, worktree or not. Not treefmt-nix's `.git/config`, which a
      # worktree does not have (its `.git` is a file), so the walk would pass
      # it and reach the main checkout. The generated treefmt.toml is unchanged.
      projectRootFile = "devenv.nix";

      settings.global.excludes = [
        ".devenv.flake.nix"
        ".devenv/"
        # Pinned copies of other projects' sources, hash-locked by
        # `docs/research/sources/MANIFEST.toml`. rustfmt rewrote six of the
        # group J files on its first pass and every one stopped matching its
        # sha256 -- a formatted pin is no longer the document it claims to be.
        # A glob, not a directory: treefmt matches excludes against file
        # paths, and the directory form left all 544 files traversed.
        "docs/research/sources/*"
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

    # The same task set CI runs, on pre-push rather than pre-commit.
    #
    # This exists because local `dev-ci` green is not evidence CI is green, and
    # relying on remembering to run it did not work: master sat red while the
    # local gate passed, because the failure was in a job the local gate does
    # not have. Pre-push is the right stage -- pre-commit would run the whole
    # suite on every commit of a stack, and the thing being protected is the
    # remote, not the reflog.
    #
    # Cost: seconds when the dev shell and ./target are warm, a full dev-shell
    # build when cold. `git push --no-verify` skips it, deliberately -- there
    # are legitimate reasons to push a branch that does not build yet, and a
    # gate with no escape hatch gets disabled wholesale instead.
    dev-ci = {
      enable = true;
      name = "dev-ci (full check suite)";
      entry = "devenv test";
      stages = [ "pre-push" ];
      pass_filenames = false;
    };
  };

  # https://devenv.sh/tasks/
  #
  # These tasks ARE the check suite. `enterTest` runs them, so `devenv test`
  # locally and `devenv test` in .github/workflows/ci.yml execute the same
  # thing by construction — the workflow is a thin wrapper, and the two cannot
  # drift apart.
  #
  # The `after` edges are load-bearing, and they set two things that are
  # separate. Order: devenv runs tasks with no edge between them
  # concurrently, so without these, fmt, clippy, check and test all invoke
  # cargo at once and block on the single lock over ./target. Serialised,
  # they run cheapest-first — a formatting failure skips clippy, check and
  # the test build — and each later task reuses what the previous one
  # compiled.
  # Failure: a plain `after` runs a task only if the one before it
  # succeeded, and `@completed` runs it once that one has finished or been
  # skipped, whatever its exit code. fmt, clippy, check and unit are chained
  # by plain edges, so a formatting error skips clippy, check and unit, and
  # a compile error skips unit, with no test build. The five gates after
  # unit are chained by `@completed` (note V, V21 B), so they run after a
  # red or a skipped unit and one push reports every independent failure;
  # after a compile error, bare and linux can fail again on it. The run still
  # exits non-zero: see the comment above `enterTest`.
  tasks = {
    "test:fmt" = {
      exec = "treefmt --fail-on-change";
    };

    "test:clippy" = {
      exec = "cargo clippy --quiet ${scope} --all-targets -- -D warnings";
      after = [ "test:fmt" ];
    };

    "test:check" = {
      exec = "cargo check --quiet --all-targets ${scope}";
      after = [ "test:clippy" ];
    };

    # --release is for speed now, not correctness. The throughput floors this
    # comment once cited (baud-core/tests/perf_floor.rs) left with baud-core2
    # (5562be5), and the last wall-clock bound became a bench on 2026-09-22
    # (docs/record.md, "A timing bound in `cargo test` measured the load, and it
    # is a bench now"), so nothing in the suite fails on the profile. A debug run
    # takes minutes where release takes seconds (docs/record.md, "The tests run
    # on every core, the owner's call of 2026-10-07"). docs/contributing/build.md
    # runs the same tests in release.
    "test:unit" = {
      # `--no-fail-fast`, the owner's call 2026-09-22: cargo stops at the
      # first failing test binary, and a wall-clock flake once stopped this
      # run at 81 of 119 -- which read as one red test rather than a partial
      # run. One red binary must not hide the rest. nextest takes the same
      # flag, and the doctests run whether or not it passed. The task exits
      # non-zero if either half failed; `test:bare` and the gates after it
      # run anyway, through their `@completed` edges.
      #
      # nextest rather than `cargo test`, the owner's call 2026-10-07; why is
      # at `pkgs.cargo-nextest` above. It builds the same test binaries,
      # with the same profile and flags, through `cargo test --no-run`.
      exec = "RUSTFLAGS='-D warnings' cargo nextest run --release --cargo-quiet --no-fail-fast ${scope}; a=$?; RUSTFLAGS='-D warnings' cargo test --release --quiet --doc ${scope}; b=$?; [ \"$a\" -eq 0 ] && [ \"$b\" -eq 0 ]";
      after = [ "test:check" ];
    };

    # The no_std proof. A host build links std through the harness crates and
    # so cannot fail on an accidental std dependency in baud-core; a target
    # with no OS and no allocator can and does.
    # `baud-font` makes the same `no_std` claim and gets the same gate; a claim
    # in a crate header that nothing builds for a bare target is prose.
    "test:bare" = {
      exec = "cargo build --release --quiet -p baud-core --target ${bareTarget} && cargo build --release --quiet -p baud-font --target ${bareTarget}";
      after = [ "test:unit@completed" ];
    };

    # The Linux wiring, type-checked under clippy for a target this Mac
    # cannot run, so the driver cannot rot unbuilt between Linux sessions.
    "test:linux" = {
      exec = "cargo clippy --quiet -p baud-app --target ${linuxTarget} --all-targets -- -D warnings";
      after = [ "test:bare@completed" ];
    };

    # The source-provenance gate. A design claim that cites a paper nobody
    # fetched is worse than one with no citation, because it looks checked.
    "test:sources" = {
      exec = "bash ./scripts/check-sources.sh";
      after = [ "test:fmt" ];
    };

    # The type-system gate: raw integers outside the newtype internals are a
    # bug waiting for two quantities to be confused. Per-file limits and the
    # reason for each live in audit_all.sh.
    "test:audit" = {
      exec = "sh ./audit_all.sh";
      after = [ "test:linux@completed" ];
    };
    # Every feature arm must build on its own. `--all-features` builds them all
    # at once, which masks per-arm rot and, on 5f1d5f0, did not compile under
    # `-D warnings` (dev-test ran it until 2026-10-11); this checks each
    # individually, the way the arms are actually used for measurement. It
    # checks; `test:feature-suites` runs the tests.
    "test:features" = {
      exec = "bash ./scripts/check-features.sh";
      after = [ "test:audit@completed" ];
    };

    # baud-core's suite under its arms, in five builds: the arms that
    # replace a default path, in three builds that cannot hide one another
    # (`probe-double-final-scan` rides with the `two-pass-csi` it implies);
    # `diag`, which adds counters and the tests that read them, alone; and
    # the additive probes in one build. Off aarch64, `route-stride` is a
    # sixth. A compiled arm can still be wrong: 7d21de1 fixed
    # `two-pass-csi`, under which a test already in the suite was red on
    # 9f392ce while no gate ran the suite under it. Every baud-core feature
    # is in one of the script's classes -- run, run by host, or excluded
    # with the test it turns red -- so a new arm cannot go unclassified.
    # `cell-table` is excluded as wrong, not by construction: its read
    # writes a cell for a character the computing path attaches, such as
    # a mark or ZWJ, and
    # `the_table_read_writes_what_the_computing_path_writes` is red under it.
    # Run by hand (`devenv tasks run test:feature-suites`) and not in
    # `enterTest`: in `devenv test` it would run on every push and in CI's
    # Lint/UT job, whose timeout is sized without it. Where it runs is the
    # owner's call (record, "baud-core's suite runs under its arms").
    "test:feature-suites" = {
      exec = "bash ./scripts/test-features.sh";
      after = [ "test:features" ];
    };

    # A `path` dependency that leaves the repository builds on the machine it
    # was written on and nowhere else. Two have already shipped here and both
    # were green -- one by never being built, one by not being a workspace
    # member -- so this is the gate that says which machine a tree can be
    # built on rather than leaving it to a failure somewhere else.
    "test:paths" = {
      exec = "bash ./scripts/check-paths.sh";
      after = [ "test:features@completed" ];
    };

    # RustSec advisories, which SECURITY.md asks reporters for. Run by hand
    # (`devenv tasks run test:advisories`) and not in `enterTest`: it fetches
    # the advisory database over the network, and an unchanged tree can pass
    # today and fail once an advisory is published, where every check in the
    # push hook is offline and gives the same answer every run. Four
    # lockfiles: the workspace's, and the three harnesses under bench-others,
    # each its own workspace. A vulnerability fails it; an unmaintained or
    # unsound crate is printed as a warning.
    "test:advisories" = {
      exec = ''
        status=0
        for lock in Cargo.lock bench-others/*/Cargo.lock; do
          echo "=== $lock ==="
          ${pkgs.cargo-audit}/bin/cargo-audit audit --file "$lock" || status=1
        done
        exit $status
      '';
    };
  };

  # https://devenv.sh/tests/
  #
  # **Every gate must stay named here.** devenv (2.4.0, is_soft_failure()
  # in devenv-tasks/src/tasks.rs) counts a failed task as soft, and leaves
  # it out of the exit code, when it was not named on the command line, some
  # task scheduled in the run comes after it, and every such edge is
  # `@completed`. Named, each gate is a root and its failure is hard, so a
  # red unit run fails `devenv test` after the later gates have run. Drop
  # unit, bare, linux, audit or features from this list, or run one later
  # gate alone (`devenv tasks run test:features`, or the by-hand
  # `test:feature-suites`, schedules the chain before it), and that gate's
  # failure exits 0.
  enterTest = lib.mkForce "devenv tasks run test:fmt test:sources test:clippy test:check test:unit test:bare test:linux test:audit test:features test:paths";
}
