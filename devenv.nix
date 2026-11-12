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

  # `test:unit`'s test build and run, and with `--no-run` the warm process's
  # build: one string, so the two cannot build with different RUSTFLAGS or a
  # different profile, and the gate finds the watcher's artifacts current.
  # `baud-bench/tests/warm.rs` holds both sites to it. Not `--no-fail-fast`,
  # which nextest refuses beside `--no-run`; `test:unit` adds it.
  unitBuild = "RUSTFLAGS='-D warnings' cargo nextest run --release --cargo-quiet ${scope}";

  # **The event rig, macOS's** (`baud-app/tests/event_rig.rs`): every effect
  # baud asks of the OS runs in a turn of its own. It needs `baud` built
  # with the test-only `effect-probe` feature, so it is a build of its own,
  # under the gate's RUSTFLAGS, in `release-probe`'s directory (the
  # workspace Cargo.toml says why). `test:unit` runs it after the suite, and
  # the warm process builds it after the suite's build.
  rigBuild = "RUSTFLAGS='-D warnings' cargo nextest run --cargo-profile release-probe --cargo-quiet -p baud --features effect-probe --test event_rig";
  isDarwin = pkgs.stdenv.hostPlatform.isDarwin;
  rigRun = if isDarwin then " ${rigBuild}; c=$?;" else " c=0;";
  warmBuild = if isDarwin then "${unitBuild} --no-run && ${rigBuild} --no-run" else "${unitBuild} --no-run";
  # The watcher, for `devenv up` and `warm-run` alike. No no-LTO arm: a
  # re-run after a small fix builds `--cargo-profile fast` in its own slot's
  # target directory, which cargo keys on that path, so a build here would
  # warm nothing it reads.
  watch = "exec bash \"$DEVENV_ROOT/scripts/warm.sh\" watch ${lib.escapeShellArg warmBuild}";
  baconConfig = ''
    default_job = "watch"
    [jobs.watch]
    command = ["bash", "-c", "exec bash \"$BAUD_WATCH_JOB\" clippy"]
    need_stdout = true
    allow_warnings = true
    on_change_strategy = "wait_then_restart"
  '';
  noJob = name: ''
    if [ -z "''${BAUD_WATCH_JOB:-}" ]; then
      echo "${name}: BAUD_WATCH_JOB is unset (devenv.local.nix names the job script); not watching"
      exec sleep 2147483647
    fi
  '';

  # The bare-metal gate. Building baud-core for a target with no OS and no
  # allocator is what actually proves #![no_std] and the default-off `alloc`
  # feature; a green build on the host proves neither.
  bareTarget = "x86_64-unknown-none";
  # The Linux gate. `baud-app`'s Wayland driver is `cfg(target_os = "linux")`,
  # so a host build on this Mac never compiles it; the Linux standard library
  # `nix/rust.nix` fetches lets clippy type-check it here. A module nothing
  # builds does not go red, it goes absent.
  linuxTarget = "aarch64-unknown-linux-gnu";
  # The disassembler `test:x86-avx2` reads the AVX2 arm with. GNU objdump,
  # whose AT&T output is what `scripts/x86-avx2.sh` matches; only Linux
  # builds it, since the script refuses every other host first.
  objdump =
    if pkgs.stdenv.hostPlatform.isLinux
    then "${pkgs.binutils-unwrapped}/bin/objdump"
    else "objdump";
in
{
  # Set root explicitly for flake compatibility
  devenv.root = lib.mkDefault (builtins.toString ./.);

  dotenv.enable = true;
  imports = [
    ./nix/rust.nix
  ];

  # **The SDK the shell links against, 26 or later**: the flake's package's
  # (`nix/darwin-sdk.nix`), in place of nixpkgs' default 14.4, so a build
  # here draws the title bar the installed app and a `cargo install` draw.
  # devenv sets SDKROOT and DEVELOPER_DIR from it.
  apple.sdk = if isDarwin then import ./nix/darwin-sdk.nix pkgs else null;

  # Additional packages for development
  packages = [
    pkgs.git
    pkgs.pkg-config
    # `test:unit` runs the tests through nextest (since 2026-10-07):
    # `cargo test` runs the ~120 test binaries one after another, so a binary
    # with one long test left the other cores idle until it finished. nextest
    # runs every test of every binary across all cores, each in its own
    # process. It does not run doctests, so `test:unit` runs those after it
    # with `cargo test --doc`.
    pkgs.cargo-nextest
    # `test:changelog` reads the history through git-cliff
    # (`scripts/check-changelog.sh`).
    pkgs.git-cliff
    # The `warm` process watches the tree through watchexec
    # (`scripts/warm.sh watch`).
    pkgs.watchexec
    # `kitten icat` for the live check that it draws under baud's default
    # config (`baud-app/src/programs.rs`), which a machine without kitten
    # skips. kitty's package carries `kitten`.
    pkgs.kitty
    # The two watchers `devenv up` runs (`processes` below).
    pkgs.bacon
    pkgs.cargo-watch
  ];

  # **The release test build, kept warm while you work.** `warm-run` runs a
  # watcher in the foreground that runs
  # `test:unit`'s builds, `--no-run`, through `scripts/bounded` at nice 10,
  # 2 s after the last save, so the gate finds nothing left to compile in this
  # checkout's `./target`. `warm-pause`
  # stops the build in flight and starts no other until `warm-resume`,
  # `warm-status` prints one line on the watcher, the pause and the build,
  # and every bench and timing example refuses to measure beside an unpaused
  # watcher (`baud-bench/src/warm.rs`). `scripts/warm.sh` has the rest.
  #
  # **Not under `devenv test`**, which starts every process before it runs
  # `enterTest` and stops them all after (devenv 2.4.0, `test` in
  # devenv/src/devenv/mod.rs): the gate would start a build beside its own,
  # and stop the `devenv up` already running.
  # **The two watchers, bacon (clippy) and cargo-watch (core and domain
  # tests), replace `warm` under `devenv up`.** `warm` kept this checkout's
  # release test build warm for a gate run here; the gate now builds in its
  # own clone and target, so a warm build here only took CPU from the work.
  # `warm-run`, `warm-pause` and `warm-status` stay for a foreground use and
  # for the benches' refusal beside a watcher. Each watcher runs ONE job
  # script, named by `BAUD_WATCH_JOB` (set in the untracked
  # `devenv.local.nix`): it builds in its own target outside this tree,
  # runs under `scripts/bounded`, honours a pause, never runs a Metal test,
  # and writes its result outside the tree. Unset, a watcher says so and
  # idles. bacon 3.26: headless, no `.bacon-locations` export, and a change
  # during a run waits for it to end (`wait_then_restart`), so a run is never
  # killed half way. cargo-watch 8.5: `.gitignore` keeps `target/` and
  # `.devenv*` out, `--watch-when-idle` drops the events a run makes, and a
  # 2 s debounce as `warm` had.
  processes = lib.mkIf (!config.devenv.isTesting) {
    bacon.exec = noJob "bacon" + ''
      exec bacon --headless --no-export-locations --config-toml ${lib.escapeShellArg baconConfig}
    '';
    cargo-watch.exec = noJob "cargo-watch" + ''
      exec cargo watch --watch-when-idle -d 2 -s 'exec bash "$BAUD_WATCH_JOB" test'
    '';
  };

  scripts.warm-run.exec = watch;

  scripts.warm-status.exec = ''
    bash "$DEVENV_ROOT/scripts/warm.sh" status
  '';

  scripts.warm-pause.exec = ''
    bash "$DEVENV_ROOT/scripts/warm.sh" pause
  '';

  scripts.warm-resume.exec = ''
    bash "$DEVENV_ROOT/scripts/warm.sh" resume
  '';

  # Development scripts
  #
  # **No `--all-features`.** It turned on every ceiling and probe arm at once,
  # and the arms are wrong by construction and stub one another out: on
  # e2dc386 baud-core's lib did not compile that way under `-D warnings`
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
    echo "Running a subset of the checks (devenv test runs them all)..."
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
    cargo clippy --quiet -p baud --target ${linuxTarget} --all-targets -- -D warnings || { echo "FAILED: linux"; exit 1; }
    echo "=== sources ==="
    bash ./scripts/check-sources.sh || { echo "FAILED: sources"; exit 1; }
    echo "=== no-python ==="
    cargo xtask no-python || { echo "FAILED: no-python"; exit 1; }
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
  # anything on stdout is prepended to that command's output, and a caller
  # that parses it -- `devenv shell -- cargo metadata`, say -- finds a banner
  # in front of the JSON and cannot read it.
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
      echo "  devenv test   - The check suite the pre-push hook runs"
      echo "  dev-ci        - A quicker subset (fmt, clippy, check, test, bare, linux, sources, no-python, audit)"
      echo "  dev-test      - Run tests"
      echo "  dev-fmt       - Check formatting"
      echo "  dev-lint      - Run clippy"
      echo "  dev-check     - Check compilation"
      echo "  dev-bare      - Build baud-core for ${bareTarget} (no_std gate)"
      echo "  dev-audit     - Run the type-system audit gate"
      echo "  dev-bench     - Ingest throughput in memory (cargo bench, ingest2)"
      echo "  dev-build     - Build the workspace"
      echo "  warm-pause    - Stop the warm build (devenv up) before a measurement"
      echo "  warm-resume   - Let the warm build start again at the next save"
      echo "  warm-run      - Run the warm build's watcher in the foreground"
      echo "  warm-status   - One line: the watcher, the pause, the build"
      echo ""
    } >&2
  '';

  # https://devenv.sh/integrations/treefmt/
  #
  # One formatter for the whole tree, not just Rust — a Nix or shell file
  # that the next run reformats is as much a diff-noise source as an
  # unformatted .rs. `treefmt` is the single entry point for `dev-fmt`,
  # `tasks."test:fmt"`, and the pre-commit hook, so all three can only ever
  # agree.
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
        # Globs, where `.devenv/` was seen to keep nothing out. In a tree with
        # no `.git`, with that entry, entering `devenv shell` changed the CLI's
        # own `.devenv/bootstrap/bootstrapLib.nix`: `deadnix --edit`, run alone
        # on a copy, deletes the unused `devenv_lock`, and the shell then failed
        # on "function 'mkDevenvForSystem' called with unexpected argument
        # 'devenv_lock'". A clone of the same commit changed nothing. With
        # `.devenv/*` alone, a build then had 17 generated `.rs` files under
        # `target/` changed. MEASUREMENTS, "`devenv shell` in a new tree".
        ".devenv/*"
        ".direnv/*"
        "target/*"
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

    # The whole check suite, `devenv test`, on pre-push rather than
    # pre-commit. Every check is local: no hosted service runs any of them.
    #
    # This exists because `dev-ci` green is not evidence the suite is green,
    # and relying on remembering to run the suite did not work: master once
    # sat red while `dev-ci` passed, because the failure was in a check
    # `dev-ci` does not have. Pre-push is the right stage -- pre-commit would
    # run the whole suite on every commit of a stack, and the thing being
    # protected is the remote, not the reflog.
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
  # These tasks ARE the check suite, and every one runs on a developer's
  # machine: baud has no hosted CI, and publishing runs locally as well
  # (`release-plz.toml`). `enterTest` runs the push gates, so `devenv test`
  # and the pre-push hook execute the same thing by construction. Three are
  # run by hand and not in `enterTest`: `test:feature-suites`,
  # `test:advisories` and, on an x86-64 host with AVX2, `test:x86-avx2`.
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
  # paths needs no build and runs right after fmt, beside sources and
  # changelog (note V, V21 add-on 1);
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
    # (108f7ef), and the last wall-clock bound became a bench on 2026-09-22, so
    # nothing in the suite fails on the profile. A debug run takes minutes where
    # release takes seconds.
    "test:unit" = {
      # `--no-fail-fast`, since 2026-09-22: cargo stops at the first failing
      # test binary, and a wall-clock flake once stopped this run at 81 of
      # 119 -- which read as one red test rather than a partial run. One red
      # binary must not hide the rest. nextest takes the same flag, and the
      # doctests run whether or not it passed. The task exits non-zero if
      # either half failed; `test:bare` and the gates after it run anyway,
      # through their `@completed` edges.
      #
      # nextest rather than `cargo test`, since 2026-10-07; why is at
      # `pkgs.cargo-nextest` above. It builds the same test binaries, with
      # the same profile and flags, through `cargo test --no-run`.
      exec = "${unitBuild} --no-fail-fast; a=$?; RUSTFLAGS='-D warnings' cargo test --release --quiet --doc ${scope}; b=$?;${rigRun} [ \"$a\" -eq 0 ] && [ \"$b\" -eq 0 ] && [ \"$c\" -eq 0 ]";
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
      exec = "cargo clippy --quiet -p baud --target ${linuxTarget} --all-targets -- -D warnings";
      after = [ "test:bare@completed" ];
    };

    # The source-provenance gate. A design claim that cites a paper nobody
    # fetched is worse than one with no citation, because it looks checked.
    "test:sources" = {
      exec = "bash ./scripts/check-sources.sh";
      after = [ "test:fmt" ];
    };

    # No tracked file is Python or runs it: nothing here declares an
    # interpreter any more, so a script that calls one again would work on
    # the machine it was written on and nowhere else. `xtask/src/no_python.rs`
    # says what it reads. A plain edge, as sources has: it needs no other gate.
    "test:no-python" = {
      exec = "cargo xtask no-python";
      after = [ "test:fmt" ];
    };

    # The type-system gate: raw integers outside the newtype internals are a
    # bug waiting for two quantities to be confused. Per-file limits and the
    # reason for each live in `xtask/src/audit.rs`.
    "test:audit" = {
      exec = "sh ./audit_all.sh";
      after = [ "test:linux@completed" ];
    };
    # Every feature arm must build on its own. `--all-features` builds them all
    # at once, which masks per-arm rot and, on e2dc386, did not compile under
    # `-D warnings` (dev-test ran it until 2026-10-11); this checks each
    # individually, the way the arms are actually used for measurement. It
    # checks; `test:feature-suites` runs the tests.
    "test:features" = {
      exec = "bash ./scripts/check-features.sh";
      after = [ "test:audit@completed" ];
    };

    # baud-core's suite under its arms, in five builds: the arms that replace
    # a default path, in three builds that cannot hide one another
    # (`probe-double-final-scan` rides with the `two-pass-csi` it implies);
    # `diag`, which adds counters and the tests that read them, alone; and
    # the additive probes in one build. Off aarch64, `route-stride` is a
    # sixth. A compiled arm can still be wrong: 08bc0f9 fixed `two-pass-csi`,
    # under which a test already in the suite was red on c6a5494 while no
    # gate ran the suite under it. Every baud-core feature is in one of the
    # script's classes -- run, run by host, or excluded with the test it
    # turns red -- so a new arm cannot go unclassified. `cell-table` is
    # excluded as wrong, not by construction: its read writes a cell for a
    # character the computing path attaches, such as a mark or ZWJ, and
    # `the_table_read_writes_what_the_computing_path_writes` is red under it.
    # Run by hand (`devenv tasks run test:feature-suites`) and not in
    # `enterTest`: in `devenv test` it would add those five builds to every
    # push.
    "test:feature-suites" = {
      exec = "bash ./scripts/test-features.sh";
      after = [ "test:features" ];
    };

    # release-plz writes CHANGELOG.md through `release-plz.toml`'s commit
    # parsers and leaves out, with no error, a commit none of them matches.
    # This fails on any such commit in the history, and on a non-conventional
    # subject, before it reaches master.
    "test:changelog" = {
      exec = "bash ./scripts/check-changelog.sh";
      after = [ "test:fmt" ];
    };

    # A `path` dependency that leaves the repository builds on the machine it
    # was written on and nowhere else. Two have already shipped here and both
    # were green -- one by never being built, one by not being a workspace
    # member -- so this is the gate that says which machine a tree can be
    # built on rather than leaving it to a failure somewhere else.
    # It needs no build, so it runs right after formatting, as sources and
    # changelog do, and a path that leaves the repository is reported in the
    # first minutes of a run rather than at the end of the chain (note V,
    # V21 add-on 1). A plain edge: a formatting failure skips it.
    "test:paths" = {
      exec = "bash ./scripts/check-paths.sh";
      after = [ "test:fmt" ];
    };

    # What crates.io would receive: `baud` and the crates it is built from,
    # each with the fields crates.io shows, a version beside every path
    # dependency, its licence and notices, a .crate under the upload limit,
    # and each built from its own .crate as `cargo publish` builds it, so a
    # tarball that does not build fails at the push, not partway through a
    # release.
    "test:publish" = {
      exec = "cargo xtask publish --verify";
      # After the features gate, not after paths: the verify builds the
      # twelve crates and their dependencies from nothing, in a folder of its
      # own under ./target (xtask/src/publish.rs, package()), so it waits for the
      # cargo chain to finish rather than share the machine with it.
      after = [ "test:features@completed" ];
    };

    # **The AVX2 arm's checks, the ones no other host can make**
    # (`scripts/x86-avx2.sh` says what each proves): `Avx2` against
    # `Portable` (`baud-isa-x86-64/tests/agrees.rs`) and the crate's other
    # tests, the workspace suite under `baud-core/avx2-scan`, and the
    # disassembly showing the arm is live -- a `vpmovmskb` on a `%ymm` in the
    # avx2 build of the `ingest2` bench, and that build differing from the
    # portable one by `cmp -s`. Each is built with `-C target-feature=+avx2`,
    # which `avx2-scan` and the isa crate's tests are cfg'd on.
    #
    # **It needs any x86_64 Linux whose /proc/cpuinfo lists avx2.** On any
    # other host it refuses and exits non-zero,
    # rather than pass having built the portable path in the arm's place.
    # Run by hand there (`devenv tasks run test:x86-avx2`) and not in
    # `enterTest`, where it would refuse every push from an aarch64 Mac. No
    # `after` edge: run alone, it schedules no other gate and its failure is
    # hard (see the comment above `enterTest`).
    "test:x86-avx2" = {
      exec = "OBJDUMP=${objdump} bash ./scripts/x86-avx2.sh";
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
  enterTest = lib.mkForce "devenv tasks run test:fmt test:sources test:no-python test:changelog test:clippy test:check test:unit test:bare test:linux test:audit test:features test:paths test:publish";
}
