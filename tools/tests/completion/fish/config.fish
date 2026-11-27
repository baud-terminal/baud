# The fish the completion recording in tests/completion/ was made with:
# no greeting, a neutral prompt and title, no OSC 7. fish sends its own OSC 133
# marks.
set -g fish_greeting
function fish_prompt
    printf 'demo> '
end
function fish_title
    echo demo
end
# fish defines no OSC 7 of its own when one is defined: the recording
# carries no host name or directory.
function __fish_update_cwd_osc
end
