## What does this change and why?

<!-- The "why" matters more than the "what" — the diff already shows what changed. -->

## Checklist

- [ ] `devenv test` passes locally (see [CONTRIBUTING.md](../CONTRIBUTING.md)) — it runs the same nix-defined suite as CI
- [ ] Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
- [ ] Any new check was added as a `tasks."test:*"` entry in `devenv.nix`, not as a workflow-only step
- [ ] No version numbers changed: the Release PR raises them (see Releases in [CONTRIBUTING.md](../CONTRIBUTING.md))
