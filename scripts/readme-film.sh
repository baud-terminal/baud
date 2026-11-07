#!/usr/bin/env bash
# Films the README's clip without a window: scripts/readme-demo-film.sh
# runs in a pty against baud's core, every frame is drawn by the CPU
# renderer through the Linux driver's timing -- the launch splash, the
# trail, the scrollbar, the photo -- by the film tool in
# baud-app/src/film.rs, a test that is run only from here, and the frames
# become one looping GIF. It opens no window, records no screen and drives
# no pointer of the desktop, so it runs the same on macOS and on Linux.
#
#   scripts/readme-film.sh [<out-dir>]
#
# What it makes, in <out-dir> (default target/readme-film):
#
#   readme.gif   the clip, about 18 s at the frames' size, looping back to
#                the launch
#   frames/      every frame, a PNG at twice the window's points
#   still.png    with ONLY=btop: btop alone at the size, the frame 4.5 s
#                in, and still-776.png, the same at the README's 776 px
#                width
#
# GRID and POINTS set the window -- default 110x33 at 12 pt: of 80x24 at
# 16, 100x30 at 13, 110x33 at 12 and 120x36 at 11, the largest type at
# which btop drew its cpu and mem boxes whole on an 18-core Mac, kept now
# that it draws the cpu box alone -- FPS the
# frames a second (25) and QUALITY gifski's: 80, where 90 made the clip
# 9.0 and 9.4 MB in two takes, too near the 10 MB it must stay under. The
# window is the macOS app's on a Retina display with the dark theme's
# opaque page: glass shows the desktop behind the window, which nothing
# can film without one (baud-app/src/film.rs says the rest).
#
# Everything outside the repository is pinned: kitten, btop and the
# encoders by nixpkgs revision, the photo by sha256 as well. The photo is an
# elementary OS wallpaper, an Unsplash photograph published before June 2017
# and so CC0, as elementary's debian/copyright records: "Canazei Granite
# Ridges" (unsplash.com/photos/yrwpJwDNSHE).
#
# It checks what it made rather than trusting it:
#   - the film tool fails a take whose demo wrote a name of this machine --
#     the user, the host, the computer's name, a home or temporary
#     directory -- and one whose second terminal answered the child
#     differently from the first;
#   - the last frame is the screen the launch settles on, so the clip
#     loops, and no two frames sampled 0.4 s apart are both blank;
#   - the GIF is under 10 MB.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
out=${1:-$root/target/readme-film}
GRID=${GRID:-110x33}
POINTS=${POINTS:-12}
FPS=${FPS:-25}
QUALITY=${QUALITY:-80}
NIXPKGS=github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a
# The revision of this repository whose log the clip shows.
REV=925fcf52
# The film tool's test, run in a profile of release's optimisation without
# LTO, so it builds in a fraction of the time.
FAST=(--config 'profile.fast.inherits="release"' --config 'profile.fast.lto=false'
  --config 'profile.fast.codegen-units=16' --config 'profile.fast.incremental=true'
  --profile fast)

die() {
  echo "readme-film: $*" >&2
  exit 1
}
git -C "$root" cat-file -e "$REV^{commit}" || die "this clone has no $REV"

tmp=$(mktemp -d)
cleanup() {
  # KEEP=1 leaves the photo and btop's config for a look.
  if [ -n "${KEEP-}" ]; then echo "kept $tmp" >&2; else rm -rf "$tmp"; fi
}
trap cleanup EXIT

nixout() { nix build --no-link --print-out-paths "$NIXPKGS#$1"; }
pinned() {
  [ "$(shasum -a 256 "$1" | cut -d' ' -f1)" = "$2" ] || die "$1 is not the file the clip was made from"
}
kitten=$(nixout kitty)/bin/kitten
btop=$(nixout btop)/bin/btop
ff=$(nixout ffmpeg-headless.bin)/bin/ffmpeg
gifski=$(nixout gifski)/bin/gifski
magick=$(nixout imagemagick)/bin/magick
photo="$(nixout pantheon.elementary-wallpapers.src)/backgrounds/Canazei Granite Ridges.jpg"
pinned "$photo" 0eff9cd7fe92aabb8937644ca154436ba926310808dd99a2902cdde335b0868c

# btop naming nothing of the machine but its load: the cpu box alone,
# titled "CPU" rather than with the chip's name, with no GPU line (the
# GPU's memory), no uptime and no frequency -- the keys and why are in
# baud-app/examples/demo/scratch.rs, BTOP, and its test holds this copy
# to them. On the terminal's background rather than its own; its disks
# the root volume alone, should the mem box come back; drawn each 200 ms,
# so its graphs fill in the seconds it is on screen.
cat >"$tmp/btop.conf" <<'EOF'
color_theme = "Default"
theme_background = False
truecolor = True
shown_boxes = "cpu"
custom_cpu_name = "CPU"
show_gpu_info = "Off"
show_uptime = False
show_cpu_freq = False
show_disks = False
disks_filter = "/"
update_ms = 200
EOF
mkdir -p "$tmp/btop"
mv "$tmp/btop.conf" "$tmp/btop/btop.conf"
# The photo at about the size it is drawn at, under the name the demo
# types: kitten decoding a 20-megapixel JPEG would hold the picture back.
"$magick" "$photo" -resize 1400x -quality 92 "$tmp/canazei.jpg"
export README_KITTEN=$kitten README_BTOP=$btop README_PHOTO=$tmp/canazei.jpg
export README_REPO=$root README_REV=$REV README_TMP=$tmp
# git as it ships: no pager, and none of the machine's configuration.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1

# The machine's names the film tool cannot read itself, one a line.
forbid=$(
  id -un
  hostname -s
  if command -v scutil >/dev/null; then
    scutil --get ComputerName 2>/dev/null || true
    scutil --get LocalHostName 2>/dev/null || true
  fi
)

rm -rf "$out/frames"
mkdir -p "$out/frames"
demo="sh $root/scripts/readme-demo-film.sh ${ONLY-}"
(
  cd "$root"
  BAUD_FILM_CMD=$demo BAUD_FILM_OUT=$out/frames BAUD_FILM_FFMPEG=$ff \
    BAUD_FILM_GRID=$GRID BAUD_FILM_POINTS=$POINTS BAUD_FILM_FPS=$FPS BAUD_FILM_SECS=60 \
    BAUD_FILM_FORBID=$forbid \
    BAUD_FILM_ENV="README_KITTEN README_BTOP README_PHOTO README_REPO README_REV README_TMP GIT_CONFIG_GLOBAL GIT_CONFIG_NOSYSTEM" \
    cargo test -p baud "${FAST[@]}" film::film_the_readme_clip -- --ignored --exact --nocapture
)

fr=("$out"/frames/*.png)
n=${#fr[@]}
[ "$n" -gt "$FPS" ] || die "only $n frames"
echo "readme-film: $n frames, $GRID at $POINTS pt"
if [ "${ONLY-}" = btop ]; then
  cp "${fr[9 * FPS / 2]}" "$out/still.png"
  "$magick" "$out/still.png" -resize 776x "$out/still-776.png"
  "$magick" identify -format '%f: %wx%h, %b\n' "$out/still.png" "$out/still-776.png"
  exit 0
fi

# The launch settles on the prompt a second in, the splash gone and
# nothing typed yet; the clip ends on the same screen, so it loops back to
# the launch.
ae=$("$magick" compare -metric AE -fuzz 2% "${fr[FPS]}" "${fr[n - 1]}" null: 2>&1 | cut -d' ' -f1) || true
[ "${ae%.*}" -lt 100 ] || die "the last frame differs from the settled launch in $ae pixels, so it does not loop"
# From a second in, every tenth frame: the launch's first frame is the
# empty page, and btop clears the screen a fifth of a second before its
# first draw, so one blank sample is the program's; two in a row, 0.4 s of
# nothing, is a film that went wrong.
prev=1000
for ((i = FPS; i < n; i += 10)); do
  sd=$("$magick" "${fr[i]}" -colorspace gray -format '%[fx:int(standard_deviation*1000)]' info:)
  [ "$sd" -ge 2 ] || [ "$prev" -ge 2 ] || die "${fr[i - 10]} to ${fr[i]} are blank"
  prev=$sd
done
echo "readme-film: it loops ($ae pixels apart), and no two tenth frames running are blank"

# At the frames' own width, twice the window's points, so a Retina display
# shows it pixel for pixel: gifski would otherwise shrink it to about 800.
width=$("$magick" identify -format '%w' "${fr[0]}")
"$gifski" --quiet --fps "$FPS" --quality "$QUALITY" --width "$width" -o "$out/readme.gif" "${fr[@]}"
size=$(wc -c <"$out/readme.gif" | tr -d ' ')
"$magick" identify -format 'readme.gif: %wx%h, ' "$out/readme.gif[0]"
echo "$size bytes"
[ "$size" -lt 10000000 ] || die "readme.gif is $size bytes, over 10 MB: lower QUALITY or FPS"
