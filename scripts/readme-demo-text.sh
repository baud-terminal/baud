#!/bin/sh
# The child the README's text still is taken on (scripts/readme-capture.sh
# runs it as BAUD_CMD): a prompt drawn with Nerd Font icons, and
# scripts/readme-text.txt -- emoji sequences, flags, a line of Chinese and of
# Japanese, more icons, and code in JetBrains Mono's ligatures -- in a table
# whose right border shows whether every width came out right. It touches
# DONE once the screen is drawn.
#
# usage: readme-demo-text.sh <done-file>
# shellcheck disable=SC1091 # read beside this file, not from the input
. "$(dirname "$0")/readme-demo-lib.sh"
done=$1
quiet
clear_all
nerd_prompt
printf 'cat readme-text.txt\n'
cat "$(dirname "$0")/readme-text.txt"
nerd_prompt
sleep 0.5
touch "$done"
exec sleep 600
