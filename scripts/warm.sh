#!/usr/bin/env bash
# **The release test build, kept warm while you work.** `devenv up` runs
# `warm.sh watch BUILD`, where BUILD is `test:unit`'s own build with
# `--no-run` (`unitBuild` in devenv.nix, the one string both use), so the
# fingerprints match and `devenv test` finds nothing left to compile:
#
#   warm.sh watch BUILD   watch the tree; on each save, after 2 s quiet, build
#   warm.sh build BUILD   one build, under scripts/bounded, at nice 10
#   warm.sh pause         start nothing, and stop the build in flight
#   warm.sh resume        start again on the next save
#
# **A pause is a file**, `.devenv/state/warm.pause`, so it outlives the shell
# that wrote it. `warm-pause` and `warm-resume` in devenv.nix run the last two.
#
# **A measurement refuses to start beside an unpaused watcher**, idle or not,
# since an idle one starts a build at the next save: every bench and timing
# example calls `baud_bench::warm::refuse_while_warm` first, and multibench
# carries the same file. It finds a watcher and its builds by the words
# `<root>/scripts/warm.sh build` in their command lines, so `watch` and
# `build` run this file by its absolute path. A root with a space in it reads
# as no root, and the measurement refuses even when paused.
#
# **Not while a test run is live.** A build in the middle of a gate's test run
# would relink the binaries nextest is starting and the rlibs the doctests
# link, so a build waits for every `cargo test` and `nextest run` on the
# machine to end, `--no-run` aside.
#
# **The pause's kill is bounded's**: a signal to its watcher on the
# `--timeout` path kills the whole tree (`End::Signalled`, scripts/bounded.rs).
# That is the macOS watcher. On Linux the helper is systemd-run or
# `setsid timeout`, a child of the script, and the signal reaches only the
# script, so there a pause starts nothing new and the build in flight runs on.
set -u

here=$(cd "${BASH_SOURCE[0]%/*}" && pwd -P) || exit 1
root=${here%/scripts}
state=$root/.devenv/state
flag=$state/warm.pause
pidfile=$state/warm.pid

# The gate's time bound, nextest's 20 periods of 60 s (.config/nextest.toml);
# a cold build takes about 11 minutes. The memory bound is the gate's too:
# bounded reads budgets.toml's when nothing replaces it.
timeout=1200

usage() {
  echo "usage: warm.sh watch BUILD | build BUILD | pause | resume" >&2
  exit 2
}

# Every live `cargo test` or `nextest run` on the machine, builds aside, but
# the ones this build runs under: a test of this file starts it from one.
test_run_live() {
  local above=" " pid=$$
  while [[ $pid =~ ^[0-9]+$ ]] && ((pid > 1)); do
    above+="$pid "
    pid=$(ps -o ppid= -p "$pid" 2>/dev/null) || break
    pid=${pid//[[:space:]]/}
  done
  ps -A -o pid= -o args= 2>/dev/null | awk -v above="$above" '
    index(above, " " $1 " ") == 0 && /(^|\/)cargo(-nextest)? (nextest run|test)( |$)/ && !/--no-run/ { found = 1 }
    END { exit !found }'
}

case ${1-} in
watch)
  [[ $# -eq 2 ]] || usage
  cd "$root" || exit 1
  # `.gitignore` already keeps target/, .claude/ and .devenv/ out; these say
  # so here as well, with .git/ and .direnv/. A queued save waits for the
  # build in flight instead of killing it.
  exec watchexec --shell=none --debounce 2s --on-busy-update queue \
    --emit-events-to none \
    --ignore 'target/**' --ignore '.git/**' --ignore '.claude/**' \
    --ignore '.devenv/**' --ignore '.direnv/**' \
    -- bash "$root/scripts/warm.sh" build "$2"
  ;;
build)
  [[ $# -eq 2 ]] || usage
  cd "$root" || exit 1
  if [[ -e $flag ]]; then
    echo "warm: paused ($flag), so nothing started"
    exit 0
  fi
  if test_run_live; then
    echo "warm: waiting for the test run to end"
    while test_run_live; do
      if [[ -e $flag ]]; then
        echo "warm: paused while waiting, so nothing started"
        exit 0
      fi
      sleep 5
    done
  fi
  mkdir -p "$state" || exit 1
  started=$SECONDS
  nice -n 10 "$root/scripts/bounded" --timeout "$timeout" -- bash -c "$2" &
  pid=$!
  echo "$pid" >"$pidfile"
  # `pause` writes the flag and then reads this file, and this wrote the file
  # and then reads the flag, so one of the two sees the other.
  if [[ -e $flag ]]; then
    kill -TERM "$pid" 2>/dev/null
  fi
  wait "$pid"
  status=$?
  if [[ -f $pidfile && $(<"$pidfile") == "$pid" ]]; then
    rm -f "$pidfile"
  fi
  echo "warm: the build ended with status $status after $((SECONDS - started)) s"
  exit "$status"
  ;;
pause)
  [[ $# -eq 1 ]] || usage
  mkdir -p "$state" && : >"$flag" || exit 1
  if [[ -f $pidfile ]]; then
    pid=$(<"$pidfile")
    # A pid file a killed build left behind can name a pid reused since, so
    # the pid is signalled only while it is still bounded.
    if [[ $pid =~ ^[1-9][0-9]*$ && $(ps -o args= -p "$pid" 2>/dev/null) == *bounded* ]]; then
      kill -TERM "$pid" 2>/dev/null
      # bounded kills the tree and then waits up to 5 s for it.
      for ((i = 0; i < 100; i++)); do
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.1
      done
      if kill -0 "$pid" 2>/dev/null; then
        echo "warm: paused, but the build in flight (pid $pid) is still running" >&2
        exit 1
      fi
      echo "warm: the build in flight (pid $pid) was stopped"
    fi
  fi
  echo "warm: paused; measurements may start"
  ;;
resume)
  [[ $# -eq 1 ]] || usage
  rm -f "$flag" || exit 1
  echo "warm: resumed; the next save starts a build"
  ;;
*) usage ;;
esac
