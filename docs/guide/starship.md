# A starship prompt for baud

baud ships a prompt for [starship](https://starship.rs):
[assets/starship/baud.toml](../../assets/starship/baud.toml). It shows the
folder you are in and baud's amber ❯, on one line:

```
~ ❯
```

In a git repository the branch and a count of what changed join the same
line, and the rest appears only while it is true: a merge or rebase left
open, how long a slow command took, background jobs, a battery about to
run out and the last command's exit code.

```
~/code/app main !1 ?1 ⇡1 ❯
```

Over ssh, as root or as another user, one line above the prompt names who
and where, `jordan@devbox`.

The folder is drawn in your terminal's own text colour, and the ❯ in
baud's amber. baud's cursor is amber too, so the end of the line reads as
baud's mark, ❯ and the block. Everything else takes one of the terminal's
sixteen colours, which baud already sets for each theme.

## What it needs

- **starship 1.27.0 or later.** 1.26.0 and earlier are affected by two
  advisories:
  [GHSA-679r-jwc5-r5m3](https://github.com/starship/starship/security/advisories/GHSA-679r-jwc5-r5m3)
  (inside a repository, git's check of who owns it is skipped) and
  [GHSA-x486-6jjc-phfp](https://github.com/starship/starship/security/advisories/GHSA-x486-6jjc-phfp)
  (a link in a folder's metadata can make starship use memory without
  bound). Both are fixed in 1.27.0. Check yours with `starship --version`.
- **No Nerd Font.** Every character outside ASCII in the prompt is one of
  ❯ ❮ ✗ ⇡ ⇣ » ⋯ ∙, which baud draws from the fonts it ships. Each is one
  column wide under every `ambiguous_width` setting, so the shell and the
  terminal always agree on where the cursor is.

## Use it

Copy the file to where starship looks for its config:

```sh
cp assets/starship/baud.toml ~/.config/starship.toml
```

Or leave it where it is and point starship at it:

```sh
export STARSHIP_CONFIG=/path/to/baud.toml
```

Then start starship from your shell's startup file:

- zsh, in `~/.zshrc`: `eval "$(starship init zsh)"`
- bash, in `~/.bashrc`: `eval "$(starship init bash)"`
- fish, in `~/.config/fish/config.fish`: `starship init fish | source`

In zsh and in bash 4.4 or later, baud's shell integration marks starship's
prompt like any other, and needs nothing from you
([`[shell] integration`](configuration.md)). fish 4 marks its own prompts.

## The light palette

starship cannot see whether the terminal is dark or light, so the file
carries two palettes, `baud-dark` and `baud-light`, and starts with the
dark one. Two colours change between them: the red of a failure, and the
colour of a git operation in progress, a slow command's time and background
jobs (yellow on dark, violet on light). The amber ❯ is the same on both, the
logo's `#f4a32c`, beside a cursor in the same colour. It reads 1.80:1 on the
light page, under the 4.5:1 the file's other colours meet; for the contrast,
set `amber = "#986100"` in `baud-light`, and in baud's config `[palette]
cursor = "#986100"` and `cursor_text = "#EFEFEF"`, which keeps ❯█ one colour.

**Two files, chosen with `STARSHIP_CONFIG`.** Keep a second copy with the
palette line changed:

```sh
mkdir -p ~/.config/starship
cp assets/starship/baud.toml ~/.config/starship/baud.toml
sed 's/^palette = "baud-dark"$/palette = "baud-light"/' \
  assets/starship/baud.toml > ~/.config/starship/baud-light.toml
```

and choose one in your startup file before `starship init`. baud follows
your Mac's light or dark appearance unless its config sets a `theme`, so
this picks the file the same way when the shell starts:

```sh
if defaults read -g AppleInterfaceStyle >/dev/null 2>&1; then
  export STARSHIP_CONFIG=~/.config/starship/baud.toml
else
  export STARSHIP_CONFIG=~/.config/starship/baud-light.toml
fi
```

**Or one file, switched in place.** `starship config palette baud-light`
rewrites the file with the light palette, and `starship config palette
baud-dark` switches back. If your config is a link, as home-manager makes
one, that command replaces the link with a plain file, so use the two
files instead.

## Add more

These are left out of the file because each costs time on every prompt.
Add the one you want to the file, and its name to `format` where the
comment says.

A blocked `.envrc`, for [direnv](https://direnv.net). It reads the folder's
list of files on every prompt: in a folder of 20,000 files that took a
prompt from about 15 ms to about 63 ms. Add `$direnv` after `$nix_shell`
in `format`.

```toml
[direnv]
disabled = false
format = "([direnv $allowed]($style) )"
allowed_msg = ""
not_allowed_msg = "blocked"
denied_msg = "denied"
style = "note"
```

The Python virtualenv you are in. Add `$python` after `$nix_shell`.

```toml
[python]
format = "[$virtualenv]($style) "
style = "quiet"
detect_extensions = []
detect_files = []
detect_folders = []
```

A language's name, added after `$nix_shell` in the same way, reads the
same list of files:

```toml
[rust]
format = "[rust](quiet) "
```

Keep `$version` out of a language's format: it makes starship run that
language's own program on every prompt, and in the Node.js module `$style`
does too.
