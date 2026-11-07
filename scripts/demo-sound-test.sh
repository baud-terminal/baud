#!/usr/bin/env bash
# demo-sound-test.sh: scripts/demo-sound.sh checked on made-up inputs.
#
#   scripts/demo-sound-test.sh
#
# Everything it feeds the mixer is made here, in a directory of its own, by
# the pinned ffmpeg: a 4 s testsrc MP4 at 60 frames a second, a bed of
# pink noise, each effect a short sine or a seeded noise, and a timeline
# with every event the mixer sounds, a trail sweeping the width and five
# pops in one second. It then holds, for a mix with no bed, the default,
# and one with `--bed`:
#   - the mixer's pin is the demo's: NIXPKGS equals run.rs's;
#   - the bed is an option: the four positional arguments of a required
#     bed are refused;
#   - the MP4 the mixer leaves has one AAC stream as long as the video, to
#     within 50 ms, and the video stream's bytes are the ones it was given;
#   - the mixer run again on the same inputs leaves the same bytes;
#   - the second run is not vacuous: a timeline with one key moved 100 ms
#     leaves different bytes;
#   - with no bed, the last 0.4 s, where no effect sounds, is silent; with
#     the bed, the same span is not, so the silence is the bed's absence.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
root=$(cd -- "$here/.." && pwd)
mixer="$here/demo-sound.sh"

pin=$(sed -n 's/^NIXPKGS="\(.*\)"$/\1/p' "$mixer")
demo=$(sed -n 's/^pub const NIXPKGS: &str = "\(.*\)";$/\1/p' "$root/baud-app/examples/demo/run.rs")
[ -n "$pin" ] && [ "$pin" = "$demo" ] || {
  echo "FAIL: the mixer's nixpkgs ($pin) is not the demo's ($demo)" >&2
  exit 1
}

bin="$(nix build --no-link --print-out-paths "$pin#ffmpeg-headless.bin")/bin"
export DEMO_SOUND_FFMPEG_BIN="$bin"
ff() { "$bin/ffmpeg" -hide_banner -v error -nostdin -y "$@"; }

work=$(mktemp -d "${TMPDIR:-/tmp}/demo-sound-test.XXXXXX")
trap 'rm -rf -- "$work"' EXIT
mkdir -p -- "$work/sounds"

ff -f lavfi -i "testsrc=size=640x400:rate=60" -t 4 -c:v libx264 -preset ultrafast \
  -pix_fmt yuv420p -threads 1 -fflags +bitexact -flags:v +bitexact -map_metadata -1 \
  -- "$work/video.mp4"
ff -f lavfi -i "anoisesrc=color=pink:seed=7:sample_rate=48000:duration=6:amplitude=0.3" \
  -ac 2 -- "$work/bed.wav"
tone() { ff -f lavfi -i "sine=frequency=$2:sample_rate=48000:duration=$3" -- "$work/sounds/$1.wav"; }
noise() { ff -f lavfi -i "anoisesrc=seed=$2:sample_rate=48000:duration=$3:amplitude=0.5" -- "$work/sounds/$1.wav"; }
tone key 170 0.06
tone enter 130 0.08
noise trail-plasma 11 0.45
noise trail-liquid 12 0.45
noise trail 13 0.45
tone copy 880 0.6
tone paste 880 0.3
tone modem 1200 1.2
tone fax 2100 0.5
noise pop 14 0.2

timeline() {
  printf 't_ms\tevent\tdetail\tx_px\ty_px\tx2_px\ty2_px\n'
  printf '%s\tkey\t-\t100\t380\t\t\n' "$1"
  printf '300\tkey\t-\t120\t380\t\t\n'
  printf '500\tenter\t-\t140\t380\t\t\n'
  printf '800\ttrail\tplasma\t0\t380\t640\t20\n'
  printf '1200\tlight\tOH on\t\t\t\t\n'
  printf '1500\tlight\tCD on\t\t\t\t\n'
  printf '1600\tpicture\tshown\t100\t100\t500\t300\n'
  printf '2000\tfireworks\tburst\t300\t200\t\t\n'
  for t in 2100 2200 2300 2400; do printf '%s\tfireworks\tburst\t320\t200\t\t\n' "$t"; done
  printf '2600\tcopy\t-\t10\t20\t200\t60\n'
  printf '2800\tpaste\t-\t200\t380\t\t\n'
  printf '3000\tvideo\tstart\t\t\t\t\n'
  printf '3500\tvideo\tend\t\t\t\t\n'
}
timeline 200 >"$work/tour.timeline.tsv"
timeline 300 >"$work/moved.timeline.tsv"

cp -- "$work/video.mp4" "$work/old.mp4"
if "$mixer" "$work/old.mp4" "$work/tour.timeline.tsv" "$work/bed.wav" "$work/sounds" \
  >/dev/null 2>&1; then
  echo "FAIL: the mixer took a bed as a positional argument" >&2
  exit 1
fi

# mix NAME TIMELINE [--bed FILE]
mix() {
  cp -- "$work/video.mp4" "$work/$1.mp4"
  "$mixer" "${@:3}" "$work/$1.mp4" "$work/$2.timeline.tsv" "$work/sounds" >/dev/null
}
# The loudest sample, in dB, of the last 0.4 s of NAME.mp4's audio.
peak() {
  "$bin/ffmpeg" -hide_banner -nostdin -ss 3.6 -i "$work/$1.mp4" -map 0:a:0 \
    -af volumedetect -f null - 2>&1 | sed -n 's/.*max_volume: \(.*\) dB$/\1/p'
}

said=""
for arm in quiet bed; do
  case $arm in
  quiet) with=() ;;
  bed) with=(--bed "$work/bed.wav") ;;
  esac
  mix "$arm-one" tour ${with[@]+"${with[@]}"}
  mix "$arm-two" tour ${with[@]+"${with[@]}"}
  mix "$arm-three" moved ${with[@]+"${with[@]}"}

  probe() { "$bin/ffprobe" -v error "$@" -of csv=p=0 -- "$work/$arm-one.mp4"; }
  streams=$(probe -show_entries stream=codec_type,codec_name | sort | tr '\n' ' ')
  [ "$streams" = "aac,audio h264,video " ] || {
    echo "FAIL ($arm): the mixed MP4's streams are $streams" >&2
    exit 1
  }
  audio=$(probe -select_streams a:0 -show_entries stream=duration)
  video=$(probe -select_streams v:0 -show_entries stream=duration)
  awk -v a="$audio" -v v="$video" 'BEGIN { d = a - v; if (d < 0) d = -d; exit !(d <= 0.05) }' || {
    echo "FAIL ($arm): the audio runs $audio s and the video $video s" >&2
    exit 1
  }
  vbytes() { ff -i "$1" -map 0:v:0 -c copy -f h264 -; }
  [ "$(vbytes "$work/$arm-one.mp4" | shasum -a 256)" = "$(vbytes "$work/video.mp4" | shasum -a 256)" ] || {
    echo "FAIL ($arm): the mixer changed the video's bytes" >&2
    exit 1
  }
  one=$(shasum -a 256 <"$work/$arm-one.mp4")
  two=$(shasum -a 256 <"$work/$arm-two.mp4")
  three=$(shasum -a 256 <"$work/$arm-three.mp4")
  [ "$one" = "$two" ] || {
    echo "FAIL ($arm): the same inputs mixed twice made different bytes" >&2
    exit 1
  }
  [ "$one" != "$three" ] || {
    echo "FAIL ($arm): a key moved 100 ms made the same bytes, so the comparison cannot fail" >&2
    exit 1
  }
  tail=$(peak "$arm-one")
  case $arm in
  quiet) awk -v p="$tail" 'BEGIN { exit !(p == "-inf" || p + 0 < -60) }' || {
    echo "FAIL (quiet): with no bed the end peaks at $tail dB, not silence" >&2
    exit 1
  } ;;
  bed) awk -v p="$tail" 'BEGIN { exit !(p != "-inf" && p + 0 > -30) }' || {
    echo "FAIL (bed): with the bed the end peaks at $tail dB, so the bed is missing" >&2
    exit 1
  } ;;
  esac
  said="$said; $arm: audio $audio s for video $video s, the same bytes twice (${one%% *}), a moved key changes them, the end peaks at $tail dB"
done
echo "ok: aac beside h264, a positional bed refused${said}"
