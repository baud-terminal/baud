#!/usr/bin/env bash
# Keystroke to pixel, six terminals, read from outside each: baud-bench's
# `key-pixel` posts a key through the compositor's virtual keyboard and times
# the first frame whose pixels changed (its module says how). Runs on the
# desktop it is started from, which must be Hyprland: each terminal opens
# there, is made full screen, takes the keys, and is closed.
#
#   scripts/linux-kp1.sh <outdir> <rounds> <keys> [arm...]
#
# Arms: `launch`'s cases below, all but baud-slow by default; baud-aa
# is baud again, the A/A floor, and baud-slow is
# baud with a child that echoes each byte 20 ms late, the probe's check.
# Each terminal runs `clear; exec cat` with its cursor's blink off, in
# DejaVu Sans Mono 13 under a scratch HOME; the first cell's 96x64 pixels
# are watched, and BackSpace resets each key. The arms' order turns each
# round. The other terminals are nixpkgs's (`linux-terminals.sh`); baud and
# key-pixel are this tree's, built here. Timed: nothing else running.
set -u
root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:?usage: scripts/linux-kp1.sh <outdir> <rounds> <keys> [arm...]}
rounds=${2:?rounds}
keys=${3:?keys}
shift 3
arms=("$@")
[ ${#arms[@]} -gt 0 ] || arms=(baud baud-aa foot alacritty kitty ghostty wezterm)
[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] || {
  echo "run it from a Hyprland session" >&2
  exit 1
}
eval "$("$root/scripts/linux-terminals.sh")" || exit 1
cargo build -q --release --manifest-path "$root/Cargo.toml" -p baud --bin baud || exit 1
cargo build -q --release --manifest-path "$root/Cargo.toml" -p baud-bench --example key-pixel || exit 1
target=${CARGO_TARGET_DIR:-$root/target}
BAUD=$target/release/baud
KP=$target/release/examples/key-pixel
KEYMAP=$root/baud-xkb/tests/fixtures/keymap-us.xkb
mkdir -p "$out"
out=$(cd "$out" && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/kp1.XXXXXX") || exit 1
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/scratch/.config"
cat >"$work/fonts.conf" <<EOF
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <dir>$FONT_DIR</dir>
  <cachedir>$work/fc-cache</cachedir>
</fontconfig>
EOF
printf '[cursor]\nblink = false\n' >"$work/baud.toml"
# The slow arm's child: no echo from the tty; each byte written back 20 ms
# later, BackSpace as the erase a cooked tty would echo.
cat >"$work/slow.sh" <<'EOF'
clear
stty raw -echo
while IFS= read -r -s -n1 -d '' c; do
  sleep 0.02
  if [ "$c" = $'\x7f' ]; then printf '\b \b'; else printf '%s' "$c"; fi
done
EOF
font="DejaVu Sans Mono"
size=13
child=(sh -c 'clear; exec cat')
# The desktop's own session, with a scratch home and the one font.
tenv() {
  env -i PATH="$PATH" HOME="$work/scratch" XDG_CONFIG_HOME="$work/scratch/.config" \
    XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" WAYLAND_DISPLAY="$WAYLAND_DISPLAY" \
    HYPRLAND_INSTANCE_SIGNATURE="$HYPRLAND_INSTANCE_SIGNATURE" \
    FONTCONFIG_FILE="$work/fonts.conf" LANG=C.UTF-8 TERM=xterm-256color "$@"
}
launch() {
  case "$1" in
  baud | baud-aa)
    tenv BAUD_CONFIG="$work/baud.toml" "$BAUD" -e "${child[@]}"
    ;;
  baud-slow)
    tenv BAUD_CONFIG="$work/baud.toml" "$BAUD" -e bash "$work/slow.sh"
    ;;
  foot)
    tenv "$FOOT" --font="$font:size=$size" -o cursor.blink=no "${child[@]}"
    ;;
  alacritty)
    tenv "$ALACRITTY" -o "font.normal.family=\"$font\"" -o font.size=$size \
      -o cursor.blinking=\"Never\" -e "${child[@]}"
    ;;
  kitty)
    tenv "$KITTY" -o "font_family=$font" -o font_size=$size -o cursor_blink_interval=0 \
      -o linux_display_server=wayland "${child[@]}"
    ;;
  ghostty)
    tenv "$GHOSTTY" --font-family="$font" --font-size=$size --cursor-style-blink=false \
      --confirm-close-surface=false --quit-after-last-window-closed=true \
      --initial-command="sh -c 'clear; exec cat'"
    ;;
  wezterm)
    tenv "$WEZTERM" --config "font=wezterm.font(\"$font\")" --config font_size=$size \
      --config cursor_blink_rate=0 start --always-new-process -- "${child[@]}"
    ;;
  esac
}
{
  echo "baud $(git -C "$root" rev-parse HEAD)$(git -C "$root" diff --quiet HEAD || echo ' (dirty)')"
  echo "baud binary $(sha256sum "$BAUD" | cut -c1-16); key-pixel $(sha256sum "$KP" | cut -c1-16)"
  echo "nixpkgs $NIXPKGS_REV: the other five are its builds"
  hyprctl version | head -1
  hyprctl monitors -j | jq -c '.[] | {name,width,height,refreshRate,scale}'
  echo "start $(date +%T) load1 $(cut -d' ' -f1 /proc/loadavg)"
} >"$out/meta.txt"
n=${#arms[@]}
r=0
while [ "$r" -lt "$rounds" ]; do
  i=0
  while [ "$i" -lt "$n" ]; do
    arm=${arms[$(((i + r) % n))]}
    before=$(hyprctl activewindow -j | jq -r '.address // ""')
    launch "$arm" >"$out/r$r-$arm.term.log" 2>&1 &
    lpid=$!
    addr=""
    for _ in $(seq 100); do
      a=$(hyprctl activewindow -j | jq -r '.address // ""')
      [ -n "$a" ] && [ "$a" != "$before" ] && {
        addr=$a
        break
      }
      sleep 0.1
    done
    if [ -z "$addr" ]; then
      echo "round $r $arm: no window" >>"$out/rounds.log"
      kill "$lpid" 2>/dev/null
      i=$((i + 1))
      continue
    fi
    hyprctl dispatch fullscreen 0 >/dev/null
    sleep 1.5
    shape=$(hyprctl activewindow -j | jq -c '{class,fullscreen,size}')
    wpid=$(hyprctl activewindow -j | jq -r '.pid')
    echo "round $r $arm $(date +%T) load1 $(cut -d' ' -f1 /proc/loadavg) $shape" >>"$out/rounds.log"
    timeout 120 "$KP" "$KEYMAP" 0 0 96 64 "$keys" 150 $((1000 + r * 10 + i)) 300 \
      >"$out/r$r-$arm.txt" 2>"$out/r$r-$arm.err"
    echo "  exit $?" >>"$out/rounds.log"
    # Closed by the window's own process: the launcher's pid is only its
    # env wrapper.
    kill "$wpid" 2>/dev/null
    for _ in $(seq 50); do
      hyprctl clients -j | jq -e --arg a "$addr" 'any(.[]; .address == $a)' >/dev/null || break
      sleep 0.1
    done
    kill "$lpid" 2>/dev/null
    wait "$lpid" 2>/dev/null
    sleep 0.5
    i=$((i + 1))
  done
  r=$((r + 1))
done
echo "end $(date +%T) load1 $(cut -d' ' -f1 /proc/loadavg)" >>"$out/meta.txt"
echo "$out"
