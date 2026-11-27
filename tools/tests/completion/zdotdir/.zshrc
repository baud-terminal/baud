# The zsh the completion recordings in tests/completion/ were made with:
# compinit and zsh's own completion for git, a neutral prompt, and the
# switches the README's table names.
autoload -Uz compinit && compinit -u -D
PS1='demo%# '

# BAUD_MENU: menu selection, as many configurations turn it on.
[[ -n ${BAUD_MENU-} ]] && zstyle ':completion:*' menu select

# BAUD_FIVE names tests/resize/zdotdir/.zshrc: the five-line prompt the
# resize recordings use, with WezTerm's marks. Otherwise WEZTERM_SH, when
# set, is WezTerm's shell integration, sourced unmodified, for the OSC 133
# marks; unset, the shell sends none.
if [[ -n ${BAUD_FIVE-} ]]; then
  source "$BAUD_FIVE"
elif [[ -n ${WEZTERM_SH-} ]]; then
  source "$WEZTERM_SH"
fi
