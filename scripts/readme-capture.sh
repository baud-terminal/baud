#!/usr/bin/env bash
# Remakes the README's pictures in assets/readme/, every one of them filmed
# from baud itself. macOS only: it records baud's window through
# ScreenCaptureKit (scripts/trail-record.swift), so the terminal running it
# needs Screen Recording, and it raises the window and drives the pointer
# through System Events, which needs Accessibility.
#
#   cargo build --release -p baud && scripts/readme-capture.sh
#
# ONLY names a subset, e.g. ONLY="hero tools"; BAUD names another binary.
#
# What it makes, each at the same window -- BAUD_GRID=80x24 at 16 pt, the
# 776x548 pt window scripts/trail-clips.sh films -- in the system's
# appearance, which nothing here sets or checks: set System Settings to Dark
# before a run, as the committed set was, with the defaults otherwise:
# plasma trail, smooth scroll, glass. The one setting changed is
# `[cursor] blink = false`, so a clip's first and last frames can match.
#
#   text.png    scripts/readme-demo-text.sh: emoji, flags, Chinese,
#               Japanese, Nerd Font icons and ligatures, in straight columns,
#               drawn from open faces only: baud's embedded Regular and icon
#               face, Noto Color Emoji and Noto Sans CJK SC (OFL-1.1 all),
#               so no Apple glyph is published
#   hero.gif    scripts/readme-demo-hero.sh: btop, a git log scrolling,
#               a photo through kitten icat, the cursor trail
#   images.gif  scripts/readme-demo-images.sh: a photo, then a smaller one
#               beside text
#   tools.gif   scripts/readme-demo-tools.sh, driven from here by
#               scripts/readme-pointer.swift: the wheel into the scrollback
#               with the overlay scrollbar, a selection, a narrowing resize
#               that rewraps, and back
#   trail.gif   the last two moves of docs/cursor/plasma.mp4, which
#               scripts/trail-clips.sh records (at the 776x488 pt window it
#               was taken at, not this one)
#   social.png  1280x640: the lockup and the tagline over a darkened frame
#               of the hero's btop
#
# Everything outside the repository is pinned: kitten, btop and the
# converters by nixpkgs revision, the two photos by sha256 as well, and the
# two Noto faces by tag and sha256, fetched once into a cache directory
# outside the repository (NOTO_CACHE, default ~/.cache/baud-readme). The
# photos are elementary OS wallpapers, both Unsplash photographs published
# before June 2017 and so CC0, as elementary's debian/copyright records:
# "Canazei Granite Ridges" (unsplash.com/photos/yrwpJwDNSHE) and "Sunset by
# the Pier" (unsplash.com/photos/ces8_Bo7bhQ).
#
# kitten runs with --transfer-mode=stream. Its default, detect, timed out
# after its ten seconds in baud at e73843fa ("Timed out waiting for a
# response from the terminal"), while file, memory and stream each drew.
# Until the detect-mode timeout is settled, the README must not say plain
# `kitten icat` draws in baud; the demos show --transfer-mode=stream on film.
#
# It checks what it made rather than trusting it:
#   - baud read the generated config without a complaint;
#   - the recorder refuses a window that was not frontmost at the start and
#     the stop, a clip under its frame floor, or a writer that dropped
#     frames, and the take is retried -- the desktop is shared, and another
#     application coming forward is the usual failure;
#   - nothing baud was sent names this machine: the user, the host, a home
#     or temporary directory (BAUD_PTYLOG, grepped);
#   - every clip's first and last frames are the same screen, so it loops,
#     no sampled frame is blank, and every file is under 5 MB; the still is
#     taken with baud's window frontmost and is not blank;
#   - nothing reaches assets/readme until all of the above has passed;
#   - the tools clip rewrapped: the window narrowed on film -- a column a
#     fifth in from the right edge went black, outside the window -- and
#     baud's grid followed it below 60 columns (BAUD_DEBUG_RESIZE).
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
baud=${BAUD:-$root/target/release/baud}
dest=$root/assets/readme
only=${ONLY:-"text hero images tools trail social"}
# The social card's tagline, which must equal the README's. Each bar
# starts a new line on the card.
TAGLINE=${TAGLINE:-"The terminal with character:|its own font,|its own cursor effects,|and the speed to keep up."}
NIXPKGS=github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a
# The revision of this repository whose log the clips show.
REV=e73843fa
# Frames a second, and gifski's quality: the tools clip redraws the whole
# screen on every frame of its scroll and resize, and at 25 and 90 it
# came to 7.3 MB.
FPS=25
QUALITY=90
# The GIFs at the window's size in points; the still at its pixels.
WIDTH=776
X=/Applications/Xcode.app/Contents/Developer

die() {
  echo "readme-capture: $*" >&2
  exit 1
}
[ -x "$baud" ] || die "no $baud: cargo build --release -p baud"
git -C "$root" cat-file -e "$REV^{commit}" || die "this clone has no $REV"

tmp=$(mktemp -d)
pid=
keep=
cleanup() {
  if [ -n "$keep" ]; then kill "$keep" 2>/dev/null || true; fi
  if [ -n "$pid" ]; then kill "$pid" 2>/dev/null || true; fi
  # KEEP=1 leaves the takes and frames for a look.
  if [ -n "${KEEP-}" ]; then echo "kept $tmp" >&2; else rm -rf "$tmp"; fi
}
trap cleanup EXIT

nixout() { nix build --no-link --print-out-paths "$NIXPKGS#$1"; }
pinned() {
  [ "$(shasum -a 256 "$1" | cut -d' ' -f1)" = "$2" ] || die "$1 is not the file the pictures were made from"
}
kitten=$(nixout kitty)/bin/kitten
btop=$(nixout btop)/bin/btop
ff=$(nixout ffmpeg-headless.bin)/bin
gifski=$(nixout gifski)/bin/gifski
magick=$(nixout imagemagick)/bin/magick
rsvg=$(nixout librsvg)/bin/rsvg-convert
walls=$(nixout pantheon.elementary-wallpapers.src)/backgrounds
photo="$walls/Canazei Granite Ridges.jpg"
photo2="$walls/Sunset by the Pier.jpg"
pinned "$photo" 0eff9cd7fe92aabb8937644ca154436ba926310808dd99a2902cdde335b0868c
pinned "$photo2" b13f5a16af1cecb399a1c533b00a4fc935a3e8ce80b2b34ed435eadb3f1b9481

# The text still's emoji and CJK faces: open fonts, so no Apple glyph is in
# the picture. Noto Color Emoji is the CBDT build baud-font reads, not the
# COLRv1 one; both are OFL-1.1, the licence beside each in its tag's tree
# (fonts/LICENSE and LICENSE). 10.7 and 16.4 MB, so they are cached rather
# than committed.
NOTO_EMOJI=https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.051/fonts/NotoColorEmoji.ttf
NOTO_EMOJI_SHA=72a635cb3d2f3524c51620cdde406b217204e8a6a06c6a096ff8ed4b5fd6e27b
NOTO_CJK=https://raw.githubusercontent.com/notofonts/noto-cjk/Sans2.004/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf
NOTO_CJK_SHA=2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b
cache=${NOTO_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/baud-readme}
# fetch <url> <sha256>: the file's path in the cache, downloaded when it is
# missing, and refused unless it hashes to the pin.
fetch() {
  local f=$cache/${1##*/}
  if [ ! -e "$f" ]; then
    mkdir -p "$cache"
    curl -sSfL -o "$f.part" "$1" || die "could not fetch $1"
    mv "$f.part" "$f"
  fi
  pinned "$f" "$2"
  printf '%s' "$f"
}
emoji=$(fetch "$NOTO_EMOJI" "$NOTO_EMOJI_SHA")
cjk=$(fetch "$NOTO_CJK" "$NOTO_CJK_SHA")

for tool in trail-record winid front readme-pointer; do
  DEVELOPER_DIR=$X env -u SDKROOT $X/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc \
    -O -suppress-warnings -sdk $X/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk \
    -o "$tmp/$tool" "$root/scripts/$tool.swift"
done
# Everything is made here and reaches assets/readme only once every check
# has passed, so a refused run leaves the committed set as it was.
out=$tmp/out
mkdir -p "$out" "$tmp/btop"

printf 'font_size = 16\n\n[cursor]\nblink = false\n' >"$tmp/baud.toml"
# The text still's: the same, with a stack of open faces only. A named stack
# replaces the default and nothing of the system follows it -- only the
# embedded Regular, icon face and STIX Two Math, which font_fallback adds
# behind it -- so Menlo, Apple Color Emoji, Apple Symbols and Hiragino are
# out. The embedded Regular comes first, by its file, as in the default; a
# named stack has no styled faces, so the prompt's bold draws regular.
# TEXT_WITHOUT="emoji", "cjk" or both leaves a face out, to show what it
# drew: its characters then have no face, and nothing of Apple's takes them.
{
  printf 'font_size = 16\nfonts = [\n'
  printf '  "%s",\n' "$root/baud-app/fonts/built/Regular.ttf" \
    "$root/baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf"
  case " ${TEXT_WITHOUT-} " in *" emoji "*) ;; *) printf '  "%s",\n' "$emoji" ;; esac
  case " ${TEXT_WITHOUT-} " in *" cjk "*) ;; *) printf '  "%s",\n' "$cjk" ;; esac
  printf ']\n\n[cursor]\nblink = false\n'
} >"$tmp/text.toml"
# btop without its process list, which names the user and every command
# line, and without the network box, which names the address; on the
# terminal's background rather than its own.
cat >"$tmp/btop/btop.conf" <<'EOF'
color_theme = "Default"
theme_background = False
truecolor = True
shown_boxes = "cpu mem"
update_ms = 500
EOF
# The photos at about the size they are shown at, under the names the
# demos type: kitten decoding a 20-megapixel JPEG put the picture on screen
# a second after the prompt that follows it.
"$magick" "$photo" -resize 1100x -quality 92 "$tmp/canazei.jpg"
"$magick" "$photo2" -resize 640x -quality 92 "$tmp/pier.jpg"
export README_KITTEN=$kitten README_BTOP=$btop README_PHOTO=$tmp/canazei.jpg README_PHOTO2=$tmp/pier.jpg
export README_REPO=$root README_REV=$REV README_TMP=$tmp
# git as it ships: no pager, and none of the machine's configuration. The
# first tools take ran the user's pager, which drew a pager's screen and
# linked every hash to the user's remote.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1

raise() {
  osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true" \
    >/dev/null 2>&1 || true
}
wait_file() { while [ ! -e "$1" ]; do sleep 0.02; done; }

# launch <name> <grid or ""> <command> [<config>]: baud running <command>
# under <config> (the shared one by default), its pid in $pid. Not in a
# subshell, so the trap sees the pid.
launch() {
  local name=$1 grid=$2 cmd=$3 config=${4:-$tmp/baud.toml}
  env ${grid:+"BAUD_GRID=$grid"} BAUD_DEBUG_RESIZE=1 BAUD_LOG=info BAUD_CONFIG="$config" BAUD_MOTION=full \
    BAUD_SCROLLER=overlay BAUD_PTYLOG="$tmp/$name.pty" BAUD_CMD="$cmd" "$baud" >"$tmp/$name.log" 2>&1 &
  pid=$!
  sleep 1.5
  grep -qx "config: $config" "$tmp/$name.log" || die "$name: baud did not read its config"
  if grep '^config: ' "$tmp/$name.log" | grep -vx "config: $config" >&2; then
    die "$name: baud complained about its config"
  fi
}
stop() {
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  pid=
}

# film <name> <frame floor> <grid> <command> [<driver>]: records <command>
# into $tmp/<name>.mp4, retrying a take the recorder or the driver refused.
# The command is given the GO and DONE files as its last two arguments; a
# driver, when named, runs after GO and DONE is touched when it returns.
film() {
  local name=$1 floor=$2 grid=$3 cmd=$4 drive=${5-} go=$tmp/$1.go done=$tmp/$1.done
  local try rc drc
  for try in 1 2 3 4 5; do
    rm -f "$go" "$done" "$tmp/$name.pty"
    launch "$name" "$grid" "$cmd $go $done"
    raise
    sleep 0.5
    # Without a grid the window opens at its own size: set it to the one
    # BAUD_GRID=80x24 makes, which the grid follows to 80x24.
    [ -n "$grid" ] || resize 776 776 1
    (while [ ! -e "$done" ]; do
      raise
      sleep 0.4
    done) &
    keep=$!
    rc=0 drc=0
    "$tmp/trail-record" "$pid" "$tmp/$name.mp4" "$go" "$done" 0.4 "$floor" >"$tmp/$name.rec" 2>&1 &
    local rec=$!
    if [ -n "$drive" ]; then
      wait_file "$go"
      "$drive" || drc=$?
      touch "$done"
    fi
    wait "$rec" || rc=$?
    kill "$keep" 2>/dev/null || true
    keep=
    stop
    cat "$tmp/$name.rec"
    [ "$rc" -eq 0 ] && [ "$drc" -eq 0 ] && return 0
    echo "$name: take $try refused (recorder $rc, driver $drc)" >&2
  done
  die "$name: five takes refused"
}

# frames <mp4> <dir> [<first frame>]: the clip at $FPS and $WIDTH, from
# its first frame or the one named, counted at $FPS from the clip's start
# (a seek by time landed mid-trail: the clips' timestamps do not start at 0),
# the last frame held half a second so the loop rests before it restarts.
frames() {
  local mp4=$1 dir=$2
  mkdir -p "$dir"
  "$ff/ffmpeg" -v error -i "$mp4" \
    -vf "fps=$FPS,trim=start_frame=${3:-0},setpts=PTS-STARTPTS,scale=$WIDTH:-2:flags=lanczos,tpad=stop_mode=clone:stop_duration=0.5" \
    "$dir/%05d.png"
}

# check_loop <name> <dir>: the first and last frames are one screen, and
# no sampled frame is blank.
check_loop() {
  local name=$1 dir=$2 ae n i sd
  local fr=("$dir"/*.png)
  n=${#fr[@]}
  ae=$("$magick" compare -metric AE -fuzz 4% "${fr[0]}" "${fr[n-1]}" null: 2>&1 | cut -d' ' -f1) || true
  echo "$name: $n frames; first and last differ in $ae pixels"
  [ "${ae%.*}" -lt 200 ] || die "$name: the first and last frames differ, so it does not loop"
  for ((i = 0; i < n; i += 10)); do
    sd=$("$magick" "${fr[i]}" -colorspace gray -format '%[fx:standard_deviation*1000]' info:)
    [ "${sd%.*}" -ge 5 ] || die "$name: ${fr[i]} is blank (stddev $sd/1000)"
  done
  echo "$name: every tenth frame sampled, none blank"
}

# check_narrow: the tools clip narrowed the window and the grid followed.
check_narrow() {
  local dir=$1 n i m min=1000 cols
  local fr=("$dir"/*.png)
  n=${#fr[@]}
  for ((i = 0; i < n; i += 4)); do
    m=$("$magick" "${fr[i]}" -crop "1x400+$((WIDTH * 4 / 5))+60" -format '%[fx:int(mean*1000)]' info:)
    [ "$m" -lt "$min" ] && min=$m
  done
  [ "$min" -le 2 ] || die "tools: the window never narrowed on film (darkest column $min/1000)"
  cols=$(awk '/^resize asked/ { split($3, a, "x"); if (m == "" || a[1] + 0 < m) m = a[1] + 0 } END { print m }' "$tmp/tools.log")
  [ -n "$cols" ] && [ "$cols" -lt 60 ] || die "tools: the grid did not follow the window (narrowest ${cols:-no} columns), so nothing rewrapped"
  echo "tools: the window narrowed on film and the grid followed it to $cols columns"
}

gif() {
  local name=$1 dir=$2
  "$gifski" --quiet --fps "$FPS" --quality "$QUALITY" -o "$out/$name.gif" "$dir"/*.png
}

# The pointer, at a cell of the window: column and row from zero, the
# window's origin read from winid each time. The text area starts 8 pt in
# and 36 pt down, and a cell is 9.7 x 21.1 pt at 16 pt, measured from a
# still of this window.
at() {
  local b
  # The largest window on screen is the terminal's.
  b=$("$tmp/winid" "$pid" | awk '/onscreen=true/ { split(substr($3, 8), a, ","); if (a[3] * a[4] > m) { m = a[3] * a[4]; b = substr($3, 8) } } END { print b }')
  awk -v b="$b" -v c="$1" -v r="$2" 'BEGIN { split(b, a, ","); printf "%.1f %.1f", a[1] + 8 + 9.7 * (c + 0.5), a[2] + 36 + 21.1 * (r + 0.5) }'
}
resize() {
  osascript - "$pid" "$1" "$2" 548 "$3" <<'EOF'
on run argv
  set p to item 1 of argv as integer
  set w0 to item 2 of argv as integer
  set w1 to item 3 of argv as integer
  set h to item 4 of argv as integer
  set stp to item 5 of argv as integer
  tell application "System Events"
    tell (first process whose unix id is p)
      set frontmost to true
      set win to first window whose subrole is "AXStandardWindow"
      repeat with w from w0 to w1 by stp
        set size of win to {w, h}
        delay 0.02
      end repeat
      set size of win to {w1, h}
    end tell
  end tell
end run
EOF
}
# shellcheck disable=SC2046 # `at` prints the two coordinates to split
drive_tools() {
  local p
  sleep 0.5
  "$tmp/readme-pointer" "$pid" wheel $(at 40 12) 15 || return 3
  sleep 0.5
  "$tmp/readme-pointer" "$pid" drag $(at 9 9) $(at 50 12) || return 3
  sleep 0.5
  resize 776 520 -16 || return 3
  sleep 0.8
  resize 520 776 32 || return 3
  sleep 0.5
  p=$(at 60 2)
  # shellcheck disable=SC2086 # two coordinates
  "$tmp/readme-pointer" "$pid" click $p || return 3
  sleep 0.3
  "$tmp/readme-pointer" "$pid" wheel $(at 40 12) -15 || return 3
  # The overlay scrollbar fades out after the wheel stops.
  sleep 1.5
}

# social: the lockup, without its baked-in ground, and the tagline in
# JetBrains Mono over a frame of the hero's btop, darkened toward the
# lockup's ground (#15171B), a line for each bar-separated part, centred on
# the card's height. rsvg-convert finds the face through a fontconfig file
# naming the repository's fonts; the repository carries no 500 weight, so
# the wordmark is drawn at the nearest it has.
social() {
  local fonts part n=0 y step=56 args=()
  local IFS='|'
  # shellcheck disable=SC2206 # split on the bars, nothing else
  local lines=($TAGLINE)
  unset IFS
  local hero=$tmp/hero.mp4
  [ -e "$hero" ] || hero=$dest/hero.gif
  [ -e "$hero" ] || die "social: needs hero.mp4 in the run or the committed hero.gif"
  fonts=$root/baud-app/fonts/jetbrains-mono
  printf '<?xml version="1.0"?>\n<fontconfig><dir>%s</dir><cachedir>%s</cachedir></fontconfig>\n' \
    "$fonts" "$tmp/fc" >"$tmp/fonts.conf"
  "$ff/ffmpeg" -v error -ss 3.0 -i "$hero" -frames:v 1 "$tmp/btop.png"
  grep -v '<rect width="300" height="130"' "$root/assets/logo/baud-lockup-dark.svg" >"$tmp/lockup.svg"
  FONTCONFIG_FILE=$tmp/fonts.conf "$rsvg" -w 440 "$tmp/lockup.svg" -o "$tmp/lockup.png"
  y=$((-(${#lines[@]} - 1) * step / 2))
  for part in "${lines[@]}"; do
    args+=(-annotate "+560$(printf '%+d' $((y + n * step)))" "$part")
    n=$((n + 1))
  done
  "$magick" "$tmp/btop.png" -resize 1280x -gravity north -chop 0x96 -crop 1280x640+0+0 +repage \
    -blur 0x2 -fill '#15171B' -colorize 84% \
    "$tmp/lockup.png" -gravity west -geometry +60+0 -composite \
    -font "$fonts/JetBrainsMono-Regular.ttf" -fill '#F3F4F6' -pointsize 38 -gravity west \
    "${args[@]}" \
    "$out/social.png"
  "$magick" identify -format 'social.png: %wx%h\n' "$out/social.png"
}

leaks() {
  local t terms=()
  # Fixed strings, one -e each: a host name is not a pattern, and one
  # holding a bracket would make grep -E fail, which `if` reads as clean.
  for t in "$(id -un)" "$(scutil --get LocalHostName 2>/dev/null || true)" \
    "$(scutil --get ComputerName 2>/dev/null || true)" "$(hostname -s)" \
    /Users/ /private/ /var/folders claude-; do
    [ -n "$t" ] && terms+=(-e "$t")
  done
  for f in "$tmp"/*.pty; do
    [ -e "$f" ] || continue
    if LC_ALL=C grep -a -n -o -m 3 -F "${terms[@]}" "$f" >&2; then
      die "$(basename "$f"): the screen was sent the machine's name or a path on it"
    fi
  done
  echo "leaks: none in $(find "$tmp" -name '*.pty' | wc -l | tr -d ' ') recordings"
}

for what in $only; do
  case $what in
  text)
    rm -f "$tmp/text.done"
    launch text 80x24 "$root/scripts/readme-demo-text.sh $tmp/text.done" "$tmp/text.toml"
    wait_file "$tmp/text.done"
    raise
    sleep 0.8
    id=$("$tmp/winid" "$pid" | awk '/onscreen=true/ && $3 ~ /,776,548$/ && !f { print $1; f = 1 }')
    [ -n "$id" ] || die "text: no 776x548 window on screen"
    # An occluded window stops drawing, as the recorder knows: baud's
    # window has to be the frontmost when the still is taken.
    front=$("$tmp/front" | cut -d' ' -f1)
    [ "$front" = "$pid" ] || die "text: pid $pid is not frontmost (pid $front is)"
    screencapture -x -o -l "$id" "$out/text.png"
    stop
    sd=$("$magick" "$out/text.png" -colorspace gray -format '%[fx:int(standard_deviation*1000)]' info:)
    [ "$sd" -ge 5 ] || die "text: the still is blank (stddev $sd/1000)"
    "$magick" identify -format 'text.png: %wx%h\n' "$out/text.png"
    ;;
  hero)
    film hero 200 80x24 "$root/scripts/readme-demo-hero.sh"
    frames "$tmp/hero.mp4" "$tmp/hero"
    check_loop hero "$tmp/hero"
    gif hero "$tmp/hero"
    ;;
  images)
    film images 30 80x24 "$root/scripts/readme-demo-images.sh"
    frames "$tmp/images.mp4" "$tmp/images"
    check_loop images "$tmp/images"
    gif images "$tmp/images"
    ;;
  tools)
    # No BAUD_GRID: under it the grid does not follow the window, so the
    # resize would not rewrap. The window is set to the 776x548 the others
    # are filmed at before the take.
    film tools 30 "" "$root/scripts/readme-demo-tools.sh" drive_tools
    FPS=20 frames "$tmp/tools.mp4" "$tmp/tools"
    check_loop tools "$tmp/tools"
    check_narrow "$tmp/tools"
    FPS=20 QUALITY=75 gif tools "$tmp/tools"
    ;;
  trail)
    # The fifth and sixth moves: from rest on the top-left mark, the
    # diagonal down, the diagonal back, and rest on the same mark.
    # Counted from 0 at 25 a second, frames 120 to 142 match the clip's
    # first -- the fourth move's trail is out -- and the fifth move starts
    # at 143; the clip ends at rest after the sixth.
    frames "$root/docs/cursor/plasma.mp4" "$tmp/trail" 125
    check_loop trail "$tmp/trail"
    gif trail "$tmp/trail"
    ;;
  social)
    social
    ;;
  *) die "unknown asset $what" ;;
  esac
done
leaks
for f in "$out"/*; do
  size=$(wc -c <"$f" | tr -d ' ')
  [ "$size" -lt 5000000 ] || die "$(basename "$f") is $size bytes, over 5 MB"
done
mkdir -p "$dest"
cp "$out"/* "$dest"/
ls -l "$dest"
