# baud's shell integration for fish: marks for each prompt and where a
# command's output begins and ends (OSC 133), so baud can tell finished
# output from the prompt you are at; and the folder the shell is in
# (OSC 7), which a `./` path is read against.
#
# baud starts fish with `-C 'source <this file>'`, which fish runs once
# its own startup and the user's configuration are done, before the
# first prompt; nothing in fish's environment changes. fish 4 and later
# mark their prompts themselves, and this file adds no mark to them.
# [shell] integration = "off" in baud's config starts fish without it.
#
# fish 3 draws its prompt from the function fish_prompt, so the mark
# where the input begins (B) could come only from a fish_prompt of
# baud's in place of the user's -- the one funcsave, funced and
# fish_config would then save over the user's own. No function of the
# user's is replaced: the marks come from fish's events, and fish 3's
# prompt goes without B, which the text says a prompt ends with. An
# open departure: baud does not know where fish 3's input begins, and
# [screen] prompt_at_bottom does not move a fish 3 prompt.

if status is-interactive; and test (string split -m 1 . -- $version)[1] -lt 4 2>/dev/null
    # Never exported. A prompt function, or a handler of the prompt's
    # events, that already marks keeps its own marks.
    set -g BAUD_SHELL_INTEGRATION 1
    if functions -q fish_prompt; and functions fish_prompt | string match -q -- '*]133;*'
        set -e BAUD_SHELL_INTEGRATION
    end
    for __baud_handler in (functions --handlers-type generic 2>/dev/null)
        set -l __baud_pair (string split ' ' -- $__baud_handler)
        if set -q __baud_pair[2]; and contains -- $__baud_pair[1] fish_prompt fish_preexec fish_postexec
            functions $__baud_pair[2] | string match -q -- '*]133;*'
            and set -e BAUD_SHELL_INTEGRATION
        end
    end
    set -e __baud_handler
end

if set -q BAUD_SHELL_INTEGRATION
    # 0 before the first prompt, 1 at a prompt no command has run from,
    # 2 once a command has run.
    set -g __baud_prompted 0
    # A token of this shell's own, on its C and D marks: baud ends a
    # remote command only at a D that carries it, which no program on the
    # other end of an ssh can know. Never exported.
    set -g __baud_token (od -An -N8 -tx1 /dev/urandom 2>/dev/null | string replace -ra '[^0-9a-f]' '')

    function __baud_prompt --on-event fish_prompt
        if test $__baud_prompted = 1
            printf '\e]133;D;aid=%s;baud_token=%s\a' $fish_pid "$__baud_token"
        end
        printf '\e]133;A;aid=%s\a' $fish_pid
        set -g __baud_prompted 1
    end

    # The command line, as kitty and fish 4 send it: `cmdline_url=`.
    function __baud_preexec --on-event fish_preexec
        printf '\e]133;C;baud_token=%s;cmdline_url=%s\a' "$__baud_token" (string escape --style=url -- "$argv")
    end

    function __baud_postexec --on-event fish_postexec
        printf '\e]133;D;%s;aid=%s;baud_token=%s\a' $status $fish_pid "$__baud_token"
        set -g __baud_prompted 2
    end
end

# The folder the shell is in, for baud to read a `./` path against (OSC
# 7), at the first prompt and at every change: a file: URI naming this
# machine as $hostname names it (RFC 8089), the path escaped as fish's own
# report escapes it. Where baud's marks are, and in fish 4, which marks
# itself; not from a command substitution, whose output is captured.
if status is-interactive; and begin
        set -q BAUD_SHELL_INTEGRATION
        or test (string split -m 1 . -- $version)[1] -ge 4 2>/dev/null
    end
    function __baud_cwd --on-variable PWD
        status is-command-substitution; and return
        printf '\e]7;file://%s%s\a' $hostname (string escape --style=url -- $PWD)
    end
    __baud_cwd
end

# [shell] ssh_env = "on" in baud's config, set by baud's -C before this
# file: ssh asks to send COLORTERM and TERM_PROGRAM (SendEnv,
# ssh_config(5)), which a server keeps only where its sshd_config(5) names
# them in AcceptEnv. In fish 4 too; an ssh function of the user's is kept.
if set -q __baud_ssh_env
    set -e __baud_ssh_env
    if not functions -q ssh
        function ssh --wraps ssh --description 'ssh, asking to send COLORTERM and TERM_PROGRAM'
            command ssh -o SendEnv=COLORTERM -o SendEnv=TERM_PROGRAM $argv
        end
    end
end
