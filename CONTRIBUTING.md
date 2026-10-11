# Contributing

## Setup

This project uses [devenv](https://devenv.sh) to pin the exact Rust toolchain
and tool versions CI uses:

```bash
direnv allow   # or: devenv shell
```

Everything below assumes you are inside that shell (or run each command as
`devenv shell -- <command>`).

## Before opening a PR

Run the full local pipeline:

```bash
dev-ci
```

This is not an approximation of CI — `.github/workflows/ci.yml` runs
`devenv test`, which runs the same `tasks."test:*"` set defined in
`devenv.nix`. A green `dev-ci` locally means a green PR check, because both
execute the same nix-defined suite. If you want to run one piece while
iterating, `enterShell` lists the individual scripts (`dev-fmt`, `dev-lint`,
`dev-check`, `dev-test`).

**Committing from a git worktree.** `treefmt` finds its tree root by walking
up from the working directory to the first `devenv.nix`, so it formats the
checkout you are in, worktree or not -- once the shell you are using has been
evaluated from a `devenv.nix` that says so. Passing `--tree-root` to it is an
error, because the wrapper already passes `--tree-root-file`. The pre-commit
hook still does not run in a worktree: `.pre-commit-config.yaml` is generated
by entering the shell and is absent there, and `prek` refuses to run without
it. In a worktree, run `treefmt --fail-on-change` from inside it (0 changed)
and commit with `PREK_ALLOW_NO_CONFIG=1`.

**Add a check by adding a task in `devenv.nix`, never by adding a step to the
workflow.** A step that exists only in CI is a step you cannot run locally, and
that is exactly the divergence this setup exists to prevent.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/). Nothing is
released (below), so no prefix decides a version or triggers a release.

## Releases

**baud is private and publishes nothing.** Every crate's `Cargo.toml` carries
`publish = false`, so `cargo publish` refuses each one, and a new crate must
carry it too. The release-plz jobs in `.github/workflows/ci.yml` still run on
master, and `release-plz.toml` keeps them to a no-op: `release` skips every
push that is not a Release PR's merge and finds nothing to release on one,
`release-pr` opens no Release PR, and neither tags nor makes a GitHub
release. Do not publish by hand. `cargo metadata --no-deps
--format-version 1` shows `"publish": []` for every package.

## Code of Conduct

This project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
