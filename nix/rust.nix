_:

{
  # Rust language configuration
  languages.rust = {
    enable = true;
    # https://devenv.sh/reference/options/#languagesrustchannel
    #
    # Nightly, deliberately. This project is a measurement harness for an ingest
    # hot path, and the compiler is part of what is being measured: codegen
    # changes, new SIMD intrinsics and portable-simd all land on nightly first,
    # and a regression that appears there is a finding rather than an accident.
    channel = "nightly";

    components = [
      "rustc"
      "cargo"
      "clippy"
      "rustfmt"
      "rust-analyzer"
      # `cargo miri test -p baud-pipeline` checks the handoff's `unsafe`
      # for undefined behaviour; Miri builds its sysroot from `rust-src`.
      "miri"
      "rust-src"
    ];

    # Warnings are errors for every cargo invocation in the shell, `devenv test`
    # included.
    # devenv folds this into the RUSTFLAGS env var (languages.rust.rustflags).
    # Cargo passes `--cap-lints allow` to registry/git dependencies, so only
    # this project's own crates are held to it — a warning in a third-party
    # dependency never breaks the build.
    rustflags = "-D warnings";

    # Fetched by rust-overlay alongside the toolchain, so the bare-metal gate in
    # `tasks."test:bare"` needs no extra setup step. x86_64-unknown-none
    # has no OS and no allocator, which is what makes it a real test of
    # baud-core's #![no_std] and its default-off `alloc` feature.
    # The two Linux targets are for `cargo check` of the Wayland wiring in
    # baud-app on this Mac: type-checked here, run on a Linux box. Nothing
    # links for them here, and nothing needs to.
    targets = [
      "x86_64-unknown-none"
      "aarch64-unknown-linux-gnu"
      "x86_64-unknown-linux-gnu"
    ];
  };
}
