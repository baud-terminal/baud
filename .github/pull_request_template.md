## What does this change and why?

<!-- The "why" matters more than the "what" — the diff already shows what changed. -->

## Checklist

- [ ] `dev-ci` passes locally (see [CONTRIBUTING.md](../CONTRIBUTING.md)) — it runs the same nix-defined suite as CI
- [ ] Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
- [ ] Any new check was added as a `tasks."test:*"` entry in `devenv.nix`, not as a workflow-only step
- [ ] No version numbers changed — baud publishes nothing, and a release raises them in a commit of its own (see Releases in [CONTRIBUTING.md](../CONTRIBUTING.md))
