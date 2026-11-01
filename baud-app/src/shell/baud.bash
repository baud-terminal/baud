# baud's shell integration for bash: marks for each prompt, where the
# input begins and where a command's output begins and ends (OSC 133), so
# baud can tell finished output from the prompt you are at.
#
# baud starts bash with --posix and ENV naming this file, the one way
# into a login shell's startup: in POSIX mode bash reads this file and no
# other. So this file puts back what that changed -- ENV, POSIX mode and
# what it set -- and then reads the startup files bash reads without it,
# in bash's order, before it adds the marks. [shell] integration = "off"
# in baud's config starts bash without this file.

if [ -n "${BAUD_BASH_ENV+set}" ]; then
  ENV=$BAUD_BASH_ENV
  unset BAUD_BASH_ENV
else
  unset ENV
fi
set +o posix
# POSIX mode turned these on; bash outside it starts with them off.
shopt -u inherit_errexit 2>/dev/null
case $(declare -p HISTFILE 2>/dev/null) in
"declare -- HISTFILE=\"$HOME/.sh_history\"") HISTFILE=~/.bash_history ;;
esac

# The marks want PS0, which bash has from 4.4. Set for the user's own
# startup files to read, and never exported; one that unsets it keeps
# baud's marks out of this shell.
if ((BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 4))); then
  BAUD_SHELL_INTEGRATION=1
fi

# bash.1, INVOCATION: a login shell reads /etc/profile, then the first of
# ~/.bash_profile, ~/.bash_login and ~/.profile that exists; any other
# interactive shell reads ~/.bashrc. They are the user's, and unknown here.
# shellcheck source=/dev/null
if shopt -q login_shell; then
  [ -e /etc/profile ] && . /etc/profile
  if [ -e ~/.bash_profile ]; then
    . ~/.bash_profile
  elif [ -e ~/.bash_login ]; then
    . ~/.bash_login
  elif [ -e ~/.profile ]; then
    . ~/.profile
  fi
elif [ -e ~/.bashrc ]; then
  . ~/.bashrc
fi

# The marks, set up in one function so that a DEBUG trap the user's files
# set sees one command of baud's here and not each line of it.
__baud_marks() {
  [ -n "${BAUD_SHELL_INTEGRATION-}" ] || return 0
  local -
  set +u
  __baud_prompted=0
  __baud_status=0
  # The marks, spelled here alone, so that no function of baud's holds
  # one and a function that does is the user's.
  __baud_mark=']133;'
  __baud_osc=$'\e'$__baud_mark
  __baud_bel=$'\a'
  __baud_p='\[\e]133;P;k=i\a\]'
  __baud_s='\[\e]133;P;k=s\a\]'
  __baud_b='\[\e]133;B\a\]'
  __baud_c='\e]133;C\a'
  # Whether PS0 was set when it was last marked; whether the DEBUG trap
  # is wanted; the DEBUG trap read at the prompt, and the user's own.
  __baud_ps0=
  __baud_arm=
  __baud_trap=
  __baud_user_debug=
  # bash 5.1 and later run each entry of an array PROMPT_COMMAND with the
  # command's $?, PIPESTATUS and $_; before 5.1 only its first is run.
  __baud_arrays=0
  if ((BASH_VERSINFO[0] > 5 || (BASH_VERSINFO[0] == 5 && BASH_VERSINFO[1] >= 1))); then
    __baud_arrays=1
  fi

  # The prompts as the user set them: baud's marks taken out, and PS0
  # unset again if it was unset. Under `local -`, a user's `set -u` stays
  # out of every function of baud's.
  __baud_unmark() {
    local -
    set +u
    PS1=${PS1//"$__baud_p"/}
    PS1=${PS1//"$__baud_b"/}
    PS2=${PS2//"$__baud_s"/}
    PS2=${PS2//"$__baud_b"/}
    if [[ -n ${PS0+set} ]]; then
      PS0=${PS0//"$__baud_c"/}
      if [[ -z $PS0 && -z $__baud_ps0 ]]; then
        unset PS0
      fi
    fi
  }

  # The command's D, once baud's entry has kept its status. The marks go
  # to stderr, where bash draws the prompt.
  __baud_before() {
    if ((__baud_prompted)); then
      builtin printf '%sD;%s;aid=%s%s' "$__baud_osc" "$__baud_status" "$$" "$__baud_bel" >&2
    fi
    return "$__baud_status"
  }

  # After the user's entries of PROMPT_COMMAND: the prompt they made, marked.
  __baud_after() {
    local -
    set +u
    # Before the first prompt is marked, every mark in the prompts, in
    # PROMPT_COMMAND and in any function is the user's: their startup files
    # mark the prompt, and theirs stand alone.
    if ((!__baud_prompted)) && {
      [[ $PS1$PS2$PS0${PROMPT_COMMAND[*]} == *"$__baud_mark"* ]] ||
        [[ $(declare -f) == *"$__baud_mark"* ]]
    }; then
      local status=$__baud_status
      __baud_stand_down
      return "$status"
    fi
    __baud_unmark
    builtin printf '%sA;aid=%s%s' "$__baud_osc" "$$" "$__baud_bel" >&2
    __baud_prompted=1
    __baud_ps0=${PS0+set}
    PS1=$__baud_p$PS1$__baud_b
    PS2=$__baud_s$PS2$__baud_b
    PS0=$__baud_c$PS0
    # An exported prompt reaches every command as it stands, so the marks
    # come out before the command runs: by a DEBUG trap the rest of this
    # entry sets and whose first firing clears it. Only while this entry
    # is the last, as a later one would fire it before the prompt is drawn.
    __baud_arm=
    if [[ ${PS1@a}${PS2@a}${PS0@a} == *x* ]] && __baud_last_entry; then
      __baud_arm=1
    fi
    return "$__baud_status"
  }

  # Whether the last entry of PROMPT_COMMAND that bash runs is baud's.
  __baud_last_entry() {
    if ((__baud_arrays)) && [[ ${PROMPT_COMMAND@a} == *a* ]]; then
      ((${#PROMPT_COMMAND[@]})) && [[ ${PROMPT_COMMAND[-1]} == "$__baud_entry" ||
        ${PROMPT_COMMAND[-1]} == *"$__baud_last" ]]
    else
      [[ $PROMPT_COMMAND == *"$__baud_last" ]]
    fi
  }

  # The DEBUG trap, from the one the last entry read where a function
  # cannot see it: its first firing, before a line's first command, takes
  # the marks out, puts the user's own trap back and runs it for that
  # command, `$?` and `$_` as they were. baud's, not fired since, is kept.
  __baud_set_trap() {
    local -
    set +u
    case $__baud_trap in
    *__baud_debug*) return ;;
    '') __baud_user_debug= ;;
    *)
      __baud_trap=${__baud_trap#trap -- }
      __baud_trap=${__baud_trap% DEBUG}
      eval "__baud_user_debug=$__baud_trap"
      ;;
    esac
    if [[ -n $__baud_user_debug ]]; then
      # shellcheck disable=SC2016
      builtin trap -- '__baud_st=$? __baud_u=$_; builtin trap -- "$__baud_user_debug" DEBUG; __baud_debug "$__baud_st" "$__baud_u"'$'\n'"$__baud_user_debug" DEBUG
    else
      # A DEBUG trap cleared inside a function is set again when it returns;
      # and one that fails skips its command under extdebug.
      # shellcheck disable=SC2016
      builtin trap -- '__baud_u=$_; builtin trap - DEBUG; __baud_debug 0 "$__baud_u"' DEBUG
    fi
  }

  # The trap's first firing: the prompts as the user set them, for the
  # command to be given; it returns the status the user's own trap is to
  # see, and its last argument is `$_` as it was.
  __baud_debug() {
    __baud_unmark
    return "$1"
  }

  # baud's entries out of PROMPT_COMMAND, in the form the user's startup
  # files left it, and every name of baud's gone. A string of baud's may
  # have become part of another entry: one that rewrites PROMPT_COMMAND at
  # the first prompt, as bash-preexec does, keeps what it found.
  # shellcheck disable=SC2128,SC2178
  __baud_stand_down() {
    local i e
    if ((__baud_arrays)) && [[ ${PROMPT_COMMAND@a} == *a* ]]; then
      for i in "${!PROMPT_COMMAND[@]}"; do
        e=${PROMPT_COMMAND[i]//"$__baud_first"$'\n'/}
        e=${e//$'\n'"$__baud_last"/}
        case $e in
        "$__baud_entry" | "$__baud_first" | "$__baud_last") unset 'PROMPT_COMMAND[i]' ;;
        *) PROMPT_COMMAND[i]=$e ;;
        esac
      done
      # Numbered from 0 again, as it would be had baud's entry not been
      # there; and a string of the user's that baud made an array is a
      # string again.
      PROMPT_COMMAND=("${PROMPT_COMMAND[@]}")
      if [[ $__baud_pc != *a* ]] && ((${#PROMPT_COMMAND[@]} <= 1)); then
        e=${PROMPT_COMMAND[*]}
        unset PROMPT_COMMAND
        if [[ -n $__baud_pc_set ]]; then
          PROMPT_COMMAND=$e
        fi
      fi
    else
      PROMPT_COMMAND=${PROMPT_COMMAND//"$__baud_first"$'\n'/}
      PROMPT_COMMAND=${PROMPT_COMMAND//$'\n'"$__baud_last"/}
      case $PROMPT_COMMAND in
      "$__baud_first" | "$__baud_last")
        if [[ -n $__baud_pc_set ]]; then
          PROMPT_COMMAND=
        else
          unset PROMPT_COMMAND
        fi
        ;;
      esac
    fi
    unset -f __baud_unmark __baud_before __baud_after __baud_last_entry __baud_set_trap \
      __baud_debug __baud_stand_down
    unset __baud_prompted __baud_status __baud_mark __baud_osc __baud_bel __baud_p \
      __baud_s __baud_b __baud_c __baud_ps0 __baud_arm __baud_trap __baud_user_debug \
      __baud_arrays __baud_pc __baud_pc_set __baud_entry __baud_first __baud_last \
      BAUD_SHELL_INTEGRATION
  }

  # baud's entries run with stdout on /dev/null. A DEBUG trap fires before
  # each of their commands as before any other entry's; bash-preexec's
  # takes the first command after its own last entry for the user's, and
  # passes over one whose stdout is not a terminal, so it still finds the
  # user's first command after it installs itself at the first prompt.
  #
  # From bash 5.1, baud's one entry, last of an array: bash gives it the
  # command's $?, as it gives every entry, so it marks D and then A.
  # shellcheck disable=SC2016
  __baud_entry='{ __baud_status=$?; if declare -F __baud_after >/dev/null; then __baud_before; __baud_after; if [[ -n ${__baud_arm-} ]]; then __baud_trap=$(builtin trap -p DEBUG); __baud_set_trap; fi; fi; } >/dev/null'
  # A string PROMPT_COMMAND is one entry, so baud's go first and last in
  # it. One a startup file exported reaches shells that do not have these
  # functions: there the first keeps `$?` for the rest, and the last does
  # nothing.
  # shellcheck disable=SC2016
  __baud_first='{ __baud_status=$?; if declare -F __baud_before >/dev/null; then __baud_before; else (exit "$__baud_status"); fi; } >/dev/null'
  # shellcheck disable=SC2016
  __baud_last='{ if declare -F __baud_after >/dev/null; then __baud_after; if [[ -n ${__baud_arm-} ]]; then __baud_trap=$(builtin trap -p DEBUG); __baud_set_trap; fi; fi; } >/dev/null'

  # PROMPT_COMMAND as the user's startup files left it, for a stand-down
  # to put back. From bash 5.1 baud's entry follows the user's: a string
  # stays as it was, the first entry of an array, where code that reads or
  # sets PROMPT_COMMAND as a string finds it and every entry of the user's
  # reads the command's $?, PIPESTATUS and `$_`; with none, the first
  # entry is left for one. An exported string stays a string, which an
  # array cannot be exported as, and so does any before 5.1.
  __baud_pc=${PROMPT_COMMAND@a}
  __baud_pc_set=${PROMPT_COMMAND+set}
  # shellcheck disable=SC2128,SC2178
  if ((__baud_arrays)) && [[ $__baud_pc == *a* ]]; then
    PROMPT_COMMAND+=("$__baud_entry")
  elif ((__baud_arrays)) && [[ $__baud_pc != *x* ]]; then
    if [[ -n $PROMPT_COMMAND ]]; then
      PROMPT_COMMAND=("$PROMPT_COMMAND" "$__baud_entry")
    else
      PROMPT_COMMAND=([1]="$__baud_entry")
    fi
  elif [[ -n $PROMPT_COMMAND ]]; then
    PROMPT_COMMAND=$__baud_first$'\n'$PROMPT_COMMAND$'\n'$__baud_last
  else
    PROMPT_COMMAND=$__baud_first$'\n'$__baud_last
  fi
}
__baud_marks
unset -f __baud_marks
