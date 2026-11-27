# shellcheck shell=sh
# What the README's demo children share (tools/scripts/readme-demo-*.sh source it;
# tools/scripts/readme-capture.sh runs them as BAUD_CMD). Sourced, not run.
#
# A demo types its commands itself, a character at a time, and runs them
# with arguments it chooses, so what is filmed is the same on every run and
# no shell prompt of the machine's -- user, host, directory -- can reach it.

e=$(printf '\033')

# The terminal's echo off and canonical mode off: the window is frontmost
# while it is filmed, and a key pressed then would otherwise be echoed onto
# the screen. The first capture taken without this showed stray
# characters under the picture.
quiet() { stty -echo -icanon 2>/dev/null || true; }

# The neutral prompt every clip shows: `baud ❯ `.
prompt() { printf '%s[1;36mbaud%s[0m %s[35m\342\235\257%s[0m ' "$e" "$e" "$e" "$e"; }

# type_cmd TEXT: types TEXT, pauses as a person does before Enter, and
# ends the line. A long command goes four characters a step, so the clip
# is not spent on typing; each step is a `sleep` process, about 30 ms.
type_cmd() {
  s=$1
  while [ -n "$s" ]; do
    rest=${s#?}
    [ ${#s} -le 24 ] || rest=${s#????}
    printf '%s' "${s%"$rest"}"
    s=$rest
    sleep 0.02
  done
  sleep 0.25
  printf '\r\n'
}

# csi FINAL: one control sequence, ESC [ and FINAL.
csi() { printf '%s[%s' "$e" "$1"; }

# nerd_prompt: the text still's prompt, in Nerd Font icons -- U+F120 (a
# terminal) and U+E0A0 (a branch) -- before the same U+276F.
nerd_prompt() {
  printf '%s[1;36m\357\204\240 baud%s[0m %s[33m\356\202\240 main%s[0m %s[35m\342\235\257%s[0m ' \
    "$e" "$e" "$e" "$e" "$e" "$e"
}

# wait_for FILE: the recorder's GO, so the first beat is on film.
wait_for() { while [ ! -e "$1" ]; do sleep 0.02; done; }

# clear_all: every kitty image deleted, then the screen, then home.
clear_all() { printf '%s_Ga=d%s\\%s[H%s[2J' "$e" "$e" "$e" "$e"; }
