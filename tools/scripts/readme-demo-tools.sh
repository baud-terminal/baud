#!/bin/sh
# The child the README's tools clip is filmed on (tools/scripts/readme-capture.sh
# runs it as BAUD_CMD and drives the wheel, the selection and the resize
# from outside): a git log long enough to leave a scrollback, its lines
# wider than the narrowed window so the resize rewraps them, and a prompt.
# It draws everything before the capture starts and then only waits.
#
# usage: readme-demo-tools.sh [<go-file> <done-file>]
# (both unused: the capture script drives this clip and ends it)
# The capture script exports README_REPO and README_REV.
# shellcheck disable=SC1091 # read beside this file, not from the input
. "$(dirname "$0")/readme-demo-lib.sh"
quiet
clear_all
prompt
printf 'git log --format="%%h  %%s" -n 60\n'
git --no-pager -C "$README_REPO" log --format='%h  %s' --no-decorate -n 60 "$README_REV"
prompt
exec sleep 600
