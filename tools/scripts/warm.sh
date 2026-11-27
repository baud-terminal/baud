#!/usr/bin/env bash
# **The release test build, kept warm while you work.** `devenv up` runs
# `warm.sh watch BUILD`, and `warm-run` runs the same in the foreground,
# where BUILD is `test:unit`'s own builds with `--no-run` (`warmBuild` in
# devenv.nix, made of the strings it runs), so the fingerprints match and
# the gate finds nothing left to compile:
#
#   warm.sh watch BUILD   watch the tree; on each save, after 2 s quiet, build
#   warm.sh build BUILD   one build, under tools/scripts/bounded, at nice 10
#   warm.sh pause         start nothing, and stop the build in flight
#   warm.sh resume        start again on the next save
#   warm.sh status        one line: the watcher, the pause, the build
#
# **A pause is a file**, `.devenv/state/warm.pause`, so it outlives the shell
# that wrote it. `warm-pause`, `warm-resume` and `warm-status` in devenv.nix
# run the last three. `watch` writes its pid to `.devenv/state/warm.watch.pid`
# and `build` its bounded's to `.devenv/state/warm.pid`.
#
# **A measurement refuses to start beside an unpaused watcher**, idle or not,
# since an idle one starts a build at the next save: every bench and timing
# example calls `baud_bench::warm::refuse_while_warm` first, and multibench
# carries the same file. It finds a watcher and its builds by the words
# `<root>/scripts/warm.sh build` in their command lines, so `watch` and
# `build` run this file by its absolute path. A root with a space in it reads
# as no root, and the measurement refuses even when paused. On Linux it also
# refuses beside any process of a build, paused or not (below).
#
# **Not while a test run is live.** A build in the middle of a gate's test run
# would relink the binaries nextest is starting and the rlibs the doctests
# link, so a build waits for every `cargo test` and `nextest run` started in
# this checkout to end, `--no-run` aside. One started in another checkout,
# a worktree under `.claude/` included, builds in that checkout's target
# directory and is not waited for.
#
# **A pause stops the whole tree, or says it did not.** Every process of a
# build carries `BAUD_WARM_BUILD=<root>` in its environment. A signal to
# bounded's watcher on the `--timeout` path kills the tree it has seen
# (`End::Signalled`, tools/scripts/bounded.rs); that is the macOS watcher. On Linux
# the helper is systemd-run or `setsid timeout`, a child of the script, which
# the signal does not reach, so there every process whose `/proc` environment
# carries the variable is signalled as well, by pid. A pause exits 1, naming
# them, while any is left, and `build` does not end before they have.
# macOS shows no other process's environment, so there the tree is what
# bounded saw: a process whose parent exits within one of its polls is not.
set -u

here=$(cd "${BASH_SOURCE[0]%/*}" && pwd -P) || exit 1
root=${here%/tools/scripts}
state=$root/.devenv/state
flag=$state/warm.pause
pidfile=$state/warm.pid
watchfile=$state/warm.watch.pid
token="BAUD_WARM_BUILD=$root"

# The gate's time bound, nextest's 20 periods of 60 s (.config/nextest.toml);
# a cold build takes about 11 minutes. The memory bound is the gate's too:
# bounded reads tools/budgets.toml's when nothing replaces it.
timeout=1200

usage() {
  echo "usage: warm.sh watch BUILD | build BUILD | pause | resume | status" >&2
  exit 2
}

# A process's working directory: `/proc` on Linux, lsof elsewhere.
cwd_of() {
  if [[ -d /proc/self ]]; then
    readlink "/proc/$1/cwd" 2>/dev/null
  else
    lsof -a -p "$1" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p'
  fi
}

# Whether a live `cargo test` or `nextest run`, builds aside, was started in
# this checkout: its working directory is the root or under it, and not
# under `.claude/`, whose worktrees are checkouts of their own. The ones
# this build runs under are left out: a test of this file starts it from one.
test_run_live() {
  local above=" " pid=$$ cwd
  while [[ $pid =~ ^[0-9]+$ ]] && ((pid > 1)); do
    above+="$pid "
    pid=$(ps -o ppid= -p "$pid" 2>/dev/null) || break
    pid=${pid//[[:space:]]/}
  done
  for pid in $(ps -A -o pid= -o args= 2>/dev/null | awk -v above="$above" '
    index(above, " " $1 " ") == 0 && /(^|\/)cargo(-nextest)? (nextest run|test)( |$)/ && !/--no-run/ { print $1 }'); do
    cwd=$(cwd_of "$pid")
    if [[ ($cwd == "$root" || $cwd == "$root"/*) && $cwd != "$root"/.claude/* ]]; then
      return 0
    fi
  done
  return 1
}

# The processes of this root's builds, by the variable in their environment,
# one pid a line: Linux's `/proc` alone shows it, so elsewhere none.
tree() {
  local environ pid
  [[ -d /proc/self ]] || return 0
  for environ in /proc/[0-9]*/environ; do
    pid=${environ#/proc/}
    pid=${pid%/environ}
    if ((pid != $$)) && grep -qzxF -- "$token" "$environ" 2>/dev/null; then
      echo "$pid"
    fi
  done
}

# Whether a pid is alive and its command line holds a word.
alive_as() {
  [[ $1 =~ ^[1-9][0-9]*$ && $(ps -o args= -p "$1" 2>/dev/null) == *"$2"* ]]
}

# Stop a build: bounded's watcher, then whatever of the tree is left. Fails,
# naming them, while any process of the tree is still running.
stop() {
  local bounded=$1 i pid sig
  local -a left
  if alive_as "$bounded" bounded; then
    kill -TERM "$bounded" 2>/dev/null
    # bounded kills the tree and then waits up to 5 s for it.
    for ((i = 0; i < 100; i++)); do
      kill -0 "$bounded" 2>/dev/null || break
      sleep 0.1
    done
  fi
  for sig in TERM KILL; do
    mapfile -t left < <(tree)
    ((${#left[@]} == 0)) && break
    for pid in "${left[@]}"; do
      kill "-$sig" "$pid" 2>/dev/null
    done
    for ((i = 0; i < 50; i++)); do
      [[ -z $(tree) ]] && break
      sleep 0.1
    done
  done
  mapfile -t left < <(tree)
  if alive_as "$bounded" bounded; then
    left+=("$bounded")
  fi
  if ((${#left[@]} > 0)); then
    echo "warm: the build is still running: pid ${left[*]}" >&2
    return 1
  fi
}

case ${1-} in
watch)
  [[ $# -eq 2 ]] || usage
  cd "$root" || exit 1
  mkdir -p "$state" || exit 1
  # `exec` keeps the pid, so this is watchexec's.
  echo "$$" >"$watchfile"
  # `.gitignore` already keeps target/, .claude/ and .devenv/ out; these say
  # so here as well, with .git/ and .direnv/. A queued save waits for the
  # build in flight instead of killing it.
  exec watchexec --shell=none --debounce 2s --on-busy-update queue \
    --emit-events-to none \
    --ignore 'target/**' --ignore '.git/**' --ignore '.claude/**' \
    --ignore '.devenv/**' --ignore '.direnv/**' \
    -- bash "$root/tools/scripts/warm.sh" build "$2"
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
  BAUD_WARM_BUILD=$root nice -n 10 "$root/tools/scripts/bounded" --timeout "$timeout" -- bash -c "$2" &
  pid=$!
  echo "$pid" >"$pidfile"
  # `pause` writes the flag and then reads this file, and this wrote the file
  # and then reads the flag, so one of the two sees the other.
  if [[ -e $flag ]]; then
    kill -TERM "$pid" 2>/dev/null
  fi
  wait "$pid"
  status=$?
  # bounded has ended; on Linux what it did not reach may not have.
  if ! stop "$pid"; then
    status=1
  fi
  if [[ -f $pidfile && $(<"$pidfile") == "$pid" ]]; then
    rm -f "$pidfile"
  fi
  echo "warm: the build ended with status $status after $((SECONDS - started)) s"
  exit "$status"
  ;;
pause)
  [[ $# -eq 1 ]] || usage
  mkdir -p "$state" && : >"$flag" || exit 1
  pid=
  if [[ -f $pidfile ]]; then
    pid=$(<"$pidfile")
  fi
  # A pid file a killed build left behind can name a pid reused since, so
  # the pid is signalled only while it is still bounded.
  was=no
  if alive_as "$pid" bounded || [[ -n $(tree) ]]; then
    was=yes
  fi
  if ! stop "$pid"; then
    echo "warm: paused, but measurements may not start yet" >&2
    exit 1
  fi
  if [[ $was == yes ]]; then
    echo "warm: the build in flight was stopped"
  fi
  echo "warm: paused; measurements may start"
  ;;
resume)
  [[ $# -eq 1 ]] || usage
  rm -f "$flag" || exit 1
  echo "warm: resumed; the next save starts a build"
  ;;
status)
  [[ $# -eq 1 ]] || usage
  line="warm:"
  watching=no
  if [[ -f $watchfile ]] && alive_as "$(<"$watchfile")" watchexec; then
    watching=yes
    line+=" watching (pid $(<"$watchfile"))"
  else
    line+=" no watcher"
  fi
  if [[ -e $flag ]]; then line+=", paused"; else line+=", not paused"; fi
  if [[ -f $pidfile ]] && alive_as "$(<"$pidfile")" bounded; then
    line+=", building (pid $(<"$pidfile"))"
  else
    line+=", no build running"
  fi
  echo "$line"
  [[ $watching == yes ]]
  ;;
*) usage ;;
esac
