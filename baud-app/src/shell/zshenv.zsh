# baud's shell integration for zsh: marks for each prompt, where the
# input begins and where a command's output begins and ends (OSC 133), so
# baud can tell finished output from the prompt you are at.
#
# baud starts zsh with ZDOTDIR naming the directory this file is written
# to, so zsh reads it where it would read $ZDOTDIR/.zshenv. Everything
# else is the user's own: ZDOTDIR is put back first, zsh's new-user check
# runs where zsh runs it, then the user's .zshenv, and zsh goes on to
# .zprofile, .zshrc and .zlogin in the user's directory. [shell]
# integration = "off" in baud's config starts zsh without this file.

if [[ -n ${BAUD_ZSH_ZDOTDIR+set} ]]; then
  builtin export ZDOTDIR=$BAUD_ZSH_ZDOTDIR
  builtin unset BAUD_ZSH_ZDOTDIR
else
  builtin unset ZDOTDIR
fi
# [shell] ssh_env = "on" in baud's config, out of what the user's startup
# files and commands see.
builtin typeset -g __baud_ssh_env=${BAUD_SSH_ENV-}
builtin unset BAUD_SSH_ENV

if [[ -o interactive ]]; then
  # zsh loads this module just before $ZDOTDIR/.zshenv, and it looks for
  # the user's startup files in ZDOTDIR, which named this file then: so
  # it is loaded again here, where ZDOTDIR is the user's.
  builtin zmodload zsh/newuser 2>/dev/null && builtin zmodload -u zsh/newuser 2>/dev/null

  # Set for the user's own startup files to read, and never exported;
  # one that unsets it keeps baud's marks out of this shell.
  BAUD_SHELL_INTEGRATION=1

  # 0 before the first prompt, 1 after it.
  builtin typeset -gi __baud_prompted=0
  # The marks, spelled here alone, so that no function of baud's holds
  # one and a function that does is the user's.
  builtin typeset -g __baud_mark=']133;'
  builtin typeset -g __baud_osc=$'\e'$__baud_mark __baud_bel=$'\a'
  builtin typeset -g __baud_p="%{${__baud_osc}P;k=i${__baud_bel}%}"
  builtin typeset -g __baud_s="%{${__baud_osc}P;k=s${__baud_bel}%}"
  builtin typeset -g __baud_b="%{${__baud_osc}B${__baud_bel}%}"
  # A token of this shell's own, on its C and D marks: baud ends a remote
  # command only at a D that carries it, which no program on the other
  # end of an ssh can know. Never exported.
  builtin typeset -g __baud_token="${$(od -An -N8 -tx1 /dev/urandom 2>/dev/null)//[^0-9a-f]/}"

  # The prompts as the user set them: baud's marks taken out.
  __baud_unmark() {
    PS1=${PS1//$__baud_p/}
    PS1=${PS1//$__baud_b/}
    PS2=${PS2//$__baud_s/}
    PS2=${PS2//$__baud_b/}
  }

  __baud_precmd() {
    builtin local -i st=$?
    builtin local percent=off
    [[ -o prompt_percent ]] && percent=on
    builtin emulate -L zsh
    # Last of the hooks, so what it marks is the prompt the others made.
    if [[ ${precmd_functions[-1]} != __baud_precmd ]]; then
      precmd_functions=(${precmd_functions:#__baud_precmd} __baud_precmd)
    fi
    # Before the first prompt is marked, every mark in the prompts, and in
    # any function, is the user's: their startup files mark the prompt, by
    # the prompts or by hooks of their own, or want no marks.
    if (( ! __baud_prompted )) && [[ -z ${BAUD_SHELL_INTEGRATION-} ||
      $PS1$PS2$RPS1 == *$__baud_mark* || ${(j: :)${(v)functions}} == *$__baud_mark* ]]; then
      precmd_functions=(${precmd_functions:#__baud_precmd})
      preexec_functions=(${preexec_functions:#__baud_preexec})
      builtin unfunction __baud_precmd __baud_preexec __baud_unmark
      builtin unset __baud_prompted __baud_mark __baud_osc __baud_bel __baud_p __baud_s \
        __baud_b __baud_token BAUD_SHELL_INTEGRATION
      return st
    fi
    __baud_unmark
    # Not when a widget runs the hooks to redraw the prompt it is at.
    if ! builtin zle; then
      (( __baud_prompted )) &&
        builtin print -rn -- "${__baud_osc}D;$st;aid=$$;baud_token=$__baud_token${__baud_bel}"
      builtin print -rn -- "${__baud_osc}A;aid=$$${__baud_bel}"
      __baud_prompted=1
    fi
    if [[ $percent == on ]]; then
      PS1=$__baud_p$PS1$__baud_b
      PS2=$__baud_s$PS2$__baud_b
    fi
    return st
  }

  # The command line, as kitty sends it: `cmdline=` quoted as `%q` quotes
  # it, so nothing in it can end the mark. zsh passes an empty `$1` when
  # its history mechanism is not active, and then the C names none.
  __baud_preexec() {
    builtin emulate -L zsh
    __baud_unmark
    if [[ -n $1 ]]; then
      builtin print -rn -- "${__baud_osc}C;baud_token=$__baud_token;cmdline=${(q)1}${__baud_bel}"
    else
      builtin print -rn -- "${__baud_osc}C;baud_token=$__baud_token${__baud_bel}"
    fi
  }

  builtin typeset -ga precmd_functions preexec_functions
  precmd_functions+=(__baud_precmd)
  preexec_functions+=(__baud_preexec)

  # [shell] ssh_env = "on": ssh asks to send COLORTERM and TERM_PROGRAM
  # (SendEnv, ssh_config(5)), which a server keeps only where its
  # sshd_config(5) names them in AcceptEnv. Defined before the user's
  # startup files, so an ssh function or alias of theirs takes its place.
  if [[ -n $__baud_ssh_env ]]; then
    ssh() { command ssh -o SendEnv=COLORTERM -o SendEnv=TERM_PROGRAM "$@"; }
  fi
fi
builtin unset __baud_ssh_env

# zsh reads a startup file that exists and is readable; so does this.
[[ -r ${ZDOTDIR-$HOME}/.zshenv ]] && builtin source -- ${ZDOTDIR-$HOME}/.zshenv
