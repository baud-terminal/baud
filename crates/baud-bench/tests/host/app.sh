#!/bin/sh
# A stand-in for the app, for the programs that measure it from outside.
# Copied to <dir>/<name> beside <dir>/<name>.txt, which holds its launches
# separated by `---` lines; each run takes the next one, round and round.
# A launch's lines go to stderr, then it waits to be stopped, as the app does.
#
#   @exit N      exits at once with N
#   @wall MS     moves <dir>/clock on, the clock the report's wall is read from
#   @env NAME    prints `startup: env: NAME=<its value>`
#   @locale L    with no LANG, sets it to L and prints the app's locale note
#   @locale-unset L  the same, with LANG unset rather than empty
#   @quiet-locale L  with no LANG, sets it to L and prints no note
#   @note-env NAME  prints the locale note naming NAME's value, or `unset`
#   @require N=V exits with 9 unless the environment has N=V
#   @run         runs BAUD_CMD, less its closing sleep
#   ?metal TEXT  prints TEXT if <dir>/root/com.apple.metal is there; !metal if
#                not; ?metalfe and !metalfe the same for com.apple.metalfe
#
# Anything else is printed through printf's %b.
dir=$(dirname "$0")
n=$(cat "$0.count" 2>/dev/null || echo 0)
echo $((n + 1)) >"$0.count"
total=$(($(grep -c '^---$' "$0.txt") + 1))
awk -v want=$((n % total)) '/^---$/ { s++; next } s + 0 == want { print }' "$0.txt" >"$0.now"
while IFS= read -r line; do
  case $line in
  '@exit '*) exit "${line#@exit }" ;;
  '@wall '*)
    was=$(cat "$dir/clock" 2>/dev/null || echo 0)
    awk -v was="$was" -v ms="${line#@wall }" 'BEGIN { printf "%.6f\n", was + ms / 1000 }' >"$dir/clock"
    ;;
  '@env '*)
    name=${line#@env }
    printf 'startup: env: %s=%s\n' "$name" "$(eval "printf '%s' \"\${$name-unset}\"")" >&2
    ;;
  '@locale-unset '*)
    if [ -z "${LANG+set}" ]; then
      LANG=${line#@locale-unset }
      export LANG
      printf 'startup: locale: %s in 0.4 ms\n' "$LANG" >&2
    fi
    ;;
  '@quiet-locale '*)
    if [ -z "$LANG" ]; then
      LANG=${line#@quiet-locale }
      export LANG
    fi
    ;;
  '@note-env '*)
    name=${line#@note-env }
    printf 'startup: locale: %s in 0.1 ms\n' "$(eval "printf '%s' \"\${$name-unset}\"" | tr ' ' '_')" >&2
    ;;
  '@require '*)
    want=${line#@require }
    [ "$(eval "printf '%s' \"\${${want%%=*}-unset}\"")" = "${want#*=}" ] || exit 9
    ;;
  '@locale '*)
    if [ -z "$LANG" ]; then
      LANG=${line#@locale }
      export LANG
      printf 'startup: locale: %s in 0.4 ms\n' "$LANG" >&2
    fi
    ;;
  @run) /bin/sh -c "${BAUD_CMD%; sleep 1}" ;;
  '?metal '*) [ -d "$dir/root/com.apple.metal" ] && printf '%b\n' "${line#\?metal }" >&2 ;;
  '!metal '*) [ -d "$dir/root/com.apple.metal" ] || printf '%b\n' "${line#!metal }" >&2 ;;
  '?metalfe '*) [ -d "$dir/root/com.apple.metalfe" ] && printf '%b\n' "${line#\?metalfe }" >&2 ;;
  '!metalfe '*) [ -d "$dir/root/com.apple.metalfe" ] || printf '%b\n' "${line#!metalfe }" >&2 ;;
  *) printf '%b\n' "$line" >&2 ;;
  esac
done <"$0.now"
exec sleep 30
