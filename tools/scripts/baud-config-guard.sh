# shellcheck shell=sh
# Sourced by a harness that starts baud: **a run never writes a config.**
#
# baud writes its commented default config when it finds none and
# BAUD_CONFIG is unset (crates/baud-app/src/default_config.rs). A harness started
# with the caller's HOME would leave that file in their home, so this sets
# BAUD_CONFIG, when it is unset, to the file baud's lookup would read --
# $XDG_CONFIG_HOME/baud/config.toml for an absolute XDG_CONFIG_HOME, then
# ~/.config/baud/config.toml -- and, with neither there, to an empty file of
# the run's own, which is the defaults and is removed when the run ends. What baud reads is unchanged either
# way; BAUD_CONFIG set makes it write nothing.
if [ -z "${BAUD_CONFIG:-}" ]; then
  case "${XDG_CONFIG_HOME:-}" in
  /*) [ -e "$XDG_CONFIG_HOME/baud/config.toml" ] && BAUD_CONFIG=$XDG_CONFIG_HOME/baud/config.toml ;;
  esac
fi
if [ -z "${BAUD_CONFIG:-}" ]; then
  case "${HOME:-}" in
  /*) [ -e "$HOME/.config/baud/config.toml" ] && BAUD_CONFIG=$HOME/.config/baud/config.toml ;;
  esac
fi
if [ -z "${BAUD_CONFIG:-}" ]; then
  BAUD_CONFIG=$(mktemp "${TMPDIR:-/tmp}/baud-config.XXXXXX") || exit 1
  # The run's own empty file goes when the run does. A harness that sets
  # an EXIT trap of its own after sourcing this replaces this one.
  baud_config_made=$BAUD_CONFIG
  trap 'rm -f "$baud_config_made"' EXIT
fi
export BAUD_CONFIG
