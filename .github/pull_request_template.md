## What does this change and why?

<!-- The "why" matters more than the "what" — the diff already shows what changed. -->

## Checklist

- [ ] `devenv test` passes locally — it is the whole nix-defined suite, and the pre-push hook runs it; nothing runs it elsewhere
- [ ] Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
- [ ] Any new check was added as a `tasks."test:*"` entry in `devenv.nix`
- [ ] No version numbers changed: release-plz's Release PR raises them
