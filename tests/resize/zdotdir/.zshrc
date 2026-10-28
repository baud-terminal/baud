# The shell the resize recordings in tests/resize/ were made with: zsh
# with no configuration but this file, a prompt of the shape those
# recordings exercise, and WezTerm's shell integration for the OSC 7 and
# OSC 133 marks.
#
# The prompt is five rows: an empty one, three padded to the full width,
# so that each wraps when the window narrows, and the input line. The
# padding is expanded from $COLUMNS each time zsh draws the prompt, so a
# resize redraws it at the new width. The glyphs are Nerd Font
# private-use code points.
#
# WEZTERM_SH names upstream's assets/shell-integration/wezterm.sh, fetched
# at record time and not committed (tests/resize/README.md).

setopt prompt_subst
_e=''
_baud_git() {
  _b=$(git symbolic-ref --short HEAD 2>/dev/null)
  _h=$(git rev-parse --short=8 HEAD 2>/dev/null)
  _p=${(%):-%~}
}
precmd_functions+=(_baud_git)

PS1=$'\n'
PS1+='zsh %B%F{yellow}'$'\uf313'' shell%f%b'
PS1+='%F{black}${(l:COLUMNS-13:: :)_e}%f'$'\ue68b'' '$'\n'
PS1+='%B%F{cyan}'$'\ue0a0\uf1d3'' ${_b}%f%b %F{blue}(${_h})%f'
PS1+='%F{black}${(l:COLUMNS-${#_b}-${#_h}-9:: :)_e}%f'
PS1+=' %F{magenta}'$'\U000f0083''%f '$'\n'
PS1+='%F{white}'$'\ueb06'' %~'
PS1+='%F{black}${(l:COLUMNS-${#_p}-2:: :)_e}%f'$'\n'
PS1+='%B%F{green}'$'\uf155''%f%b '

# Without the user variables, OSC 1337 SetUserVar, which carry the
# user and host names; the OSC 7 and OSC 133 marks stay.
export WEZTERM_SHELL_SKIP_USER_VARS=1
[[ -n $WEZTERM_SH ]] && source "$WEZTERM_SH"

# What the shell prints as it starts: a full screen for the recordings
# that begin with output, one line for the ones that begin fresh.
[[ -r ${BAUD_BANNER-} ]] && cat -- "$BAUD_BANNER"
