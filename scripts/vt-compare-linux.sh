#!/usr/bin/env bash
# Replays the recordings through six terminals on this Linux host at one
# geometry and one font, and records what each took: vt-compare.sh's method
# on a compositor of its own.
#
# A private headless compositor on the GPU's render node holds the windows,
# so nothing opens on the user's screen; every window floats, so each
# terminal keeps the 200x50 it asks for, and `vt-child.sh` records the size
# it got. One font file is the only face (a private FONTCONFIG_FILE), every
# terminal is started with a scratch HOME, and each runs `vt-child.sh`, which
# replays a recording `reps` times, waits for a DSR 6 reply and appends wall
# time, the terminal's CPU time and its geometry. Terminals are interleaved,
# their order turned by one each round; the reader discards the first round.
#
# usage: vt-compare-linux.sh <out-file> <rounds> <reps> <recording>[:reps]...
# env:   the executables, one variable each, as the check below names them;
#        FONT_DIR -- a directory holding the font (DejaVu Sans Mono);
#        VT_REPLY -- baud-bench's `vt-reply` example;
#        TERMINALS -- which to run; RENDER_NODE -- default /dev/dri/renderD128.
#        BAUD_STDERR -- where baud's stderr goes (default: discarded with the run).
set -u
out="$1"
rounds="$2"
reps="$3"
shift 3
here=$(cd "$(dirname "$0")/.." && pwd)
child="$here/scripts/vt-child.sh"
: "${TERMINALS:=baud foot alacritty kitty ghostty wezterm}"
: "${RENDER_NODE:=/dev/dri/renderD128}"
for v in FOOT ALACRITTY KITTY GHOSTTY WEZTERM BAUD SWAY FONT_DIR VT_REPLY; do
  [ -n "${!v:-}" ] || {
    echo "set $v" >&2
    exit 1
  }
done
work=$(mktemp -d "${TMPDIR:-/tmp}/vt-compare-linux.XXXXXX") || exit 1
run="$work/run"
mkdir -p "$work/scratch/.config" "$run" && chmod 700 "$run"
# No run writes a config into the caller's home: baud gets an empty config,
# and every other terminal finds none under the scratch HOME.
# baud's grid is the window's: no padding and no lights row, so 200x50 asked
# is 200x50 laid out.
printf 'padding = 0\n\n[window]\nactivity_lights = "off"\n' >"$work/baud.toml"
cat >"$work/fonts.conf" <<EOF
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <dir>$FONT_DIR</dir>
  <cachedir>$work/fc-cache</cachedir>
</fontconfig>
EOF
cat >"$work/sway.conf" <<EOF
output HEADLESS-1 mode 3840x2160 scale 1
for_window [app_id=".*"] floating enable, border none
for_window [class=".*"] floating enable, border none
EOF
sway_pid=
stop() {
  [ -n "$sway_pid" ] && kill "$sway_pid" 2>/dev/null
  rm -rf "$work"
}
trap stop EXIT
env -i PATH="$PATH" HOME="$work/scratch" XDG_RUNTIME_DIR="$run" \
  WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=gles2 \
  WLR_RENDER_DRM_DEVICE="$RENDER_NODE" \
  "$SWAY" -c "$work/sway.conf" >"$work/sway.log" 2>&1 &
sway_pid=$!
sock=
for _ in $(seq 100); do
  for s in "$run"/wayland-[0-9]*; do
    case "${s##*/}" in *.lock) ;; *) [ -S "$s" ] && sock=${s##*/} ;; esac
  done
  [ -n "$sock" ] && break
  sleep 0.1
done
[ -n "${sock:-}" ] || {
  echo "sway did not start:" >&2
  cat "$work/sway.log" >&2
  exit 1
}
# The environment every terminal starts with: the compositor, the scratch
# home, the one font, and nothing of the caller's session.
tenv() {
  env -i PATH="$PATH" HOME="$work/scratch" XDG_CONFIG_HOME="$work/scratch/.config" \
    XDG_RUNTIME_DIR="$run" WAYLAND_DISPLAY="$sock" FONTCONFIG_FILE="$work/fonts.conf" \
    LANG=C.UTF-8 TERM=xterm-256color "$@"
}
font="DejaVu Sans Mono"
size=13
run_one() {
  term="$1"
  rec="$2"
  cmd=(bash "$child" "$term" "$rec" "$reps" "$out" "$VT_REPLY")
  case "$term" in
  baud | baud-*)
    # A second build is an arm of its own: `baud-pt1` runs $BAUD_PT1. Each
    # binary's file is named `baud`, the name vt-child.sh looks for.
    bin=$BAUD
    if [ "$term" != baud ]; then
      v=BAUD_${term#baud-}
      v=${v^^}
      bin=${!v:?set $v for the $term arm}
    fi
    # `--grid` before `-e`: a floating window opens at that grid.
    tenv BAUD_CONFIG="$work/baud.toml" "$bin" --grid 200x50 -e "${cmd[@]}" \
      2>"${BAUD_STDERR:-$work/baud-stderr.log}" >/dev/null
    ;;
  foot)
    tenv "$FOOT" --window-size-chars=200x50 --font="$font:size=$size" \
      -o scrollback.lines=2000 "${cmd[@]}" >/dev/null 2>&1
    ;;
  alacritty)
    tenv "$ALACRITTY" -o window.dimensions.columns=200 -o window.dimensions.lines=50 \
      -o "font.normal.family=\"$font\"" -o font.size=$size -o scrolling.history=2000 \
      -e "${cmd[@]}" >/dev/null 2>&1
    ;;
  kitty)
    tenv "$KITTY" -o initial_window_width=200c -o initial_window_height=50c \
      -o remember_window_size=no -o "font_family=$font" -o font_size=$size \
      -o scrollback_lines=2000 -o linux_display_server=wayland \
      "${cmd[@]}" >/dev/null 2>&1
    ;;
  ghostty)
    tenv "$GHOSTTY" --window-width=200 --window-height=50 --font-family="$font" \
      --font-size=$size --scrollback-limit=2000000 --quit-after-last-window-closed=true \
      --confirm-close-surface=false --initial-command="${cmd[*]}" >/dev/null 2>&1
    ;;
  wezterm)
    face="font=wezterm.font(\"$font\")"
    tenv "$WEZTERM" --config initial_cols=200 --config initial_rows=50 \
      --config "$face" --config font_size=$size \
      --config scrollback_lines=2000 --config quit_when_all_windows_are_closed=true \
      start --always-new-process -- "${cmd[@]}" >/dev/null 2>&1
    ;;
  esac
}
# A recording may carry its own count, `file:reps`, so each run replays
# about the same bytes. The terminals' order turns by one each round, so no
# terminal always runs first or after the same one.
default_reps=$reps
read -r -a terms <<<"$TERMINALS"
n=${#terms[@]}
r=0
while [ "$r" -lt "$rounds" ]; do
  for item in "$@"; do
    rec=${item%%:*}
    reps=$default_reps
    [ "$item" != "$rec" ] && reps=${item#*:}
    # Whole: a terminal may start its child in another directory.
    rec=$(realpath "$rec")
    i=0
    while [ "$i" -lt "$n" ]; do
      term=${terms[$(((i + r) % n))]}
      echo "round $r $term $(basename "$rec") reps $reps $(date +%T) load1 $(cut -d' ' -f1 /proc/loadavg)"
      run_one "$term" "$rec"
      sleep 1
      i=$((i + 1))
    done
  done
  r=$((r + 1))
done
