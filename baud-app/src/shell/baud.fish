# baud's shell integration for fish: marks for each prompt and where a
# command's output begins and ends (OSC 133), so baud can tell finished
# output from the prompt you are at.
#
# baud starts fish with `-C 'source <this file>'`, which fish runs once
# its own startup and the user's configuration are done, before the
# first prompt; nothing in fish's environment changes. fish 4 and later
# mark their prompts themselves, and this file adds nothing to them.
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

    function __baud_prompt --on-event fish_prompt
        if test $__baud_prompted = 1
            printf '\e]133;D;aid=%s\a' $fish_pid
        end
        printf '\e]133;A;aid=%s\a' $fish_pid
        set -g __baud_prompted 1
    end

    function __baud_preexec --on-event fish_preexec
        printf '\e]133;C\a'
    end

    function __baud_postexec --on-event fish_postexec
        printf '\e]133;D;%s;aid=%s\a' $status $fish_pid
        set -g __baud_prompted 2
    end
end
