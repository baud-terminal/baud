#!/usr/bin/env bash
# demo-sound-test.sh: tools/scripts/demo-sound.sh checked on made-up inputs.
#
#   tools/scripts/demo-sound-test.sh
#
# Everything it feeds the mixer is made here, in a directory of its own, by
# the pinned ffmpeg: a 4 s testsrc MP4 at 60 frames a second; a pack of
# sine blips, a 48 kHz stereo WAV and a Mechvibes V2 config giving Enter,
# KeyA, KeyB and KeyC each a 20 ms down slice and a 10 ms up slice; and a
# timeline of five keys, four `key` and one `enter`, with a trail, a copy,
# a paste and a light between them, which lay nothing. It then holds:
#   - the mixer's pin is the demo's: NIXPKGS equals run.rs's;
#   - a pack that is not the pinned one is refused without `--unpinned`;
#   - TOUR.mp4 is left as it was, and each version has one AAC stream as
#     long as the video, to within 50 ms, beside the video's own bytes;
#   - the mixer run again leaves the same bytes in both versions, and a
#     timeline with one key moved 100 ms leaves the silent version's and
#     changes the clicks version's, so the comparison can fail;
#   - the silent version's audio decodes to zeros only, for at least the
#     video's length;
#   - the clicks version's audio has five sounds, each starting within
#     5 ms of its key's time (AAC is lossy, and the exact frames are
#     `sound.rs`'s tests'), and silence after the last.
set -euo pipefail

here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
root=$(cd -- "$here/../.." && pwd)
mixer="$here/demo-sound.sh"

pin=$(sed -n 's/^NIXPKGS="\(.*\)"$/\1/p' "$mixer")
demo=$(sed -n 's/^pub const NIXPKGS: &str = "\(.*\)";$/\1/p' "$root/crates/baud-app/examples/demo/run.rs")
[ -n "$pin" ] && [ "$pin" = "$demo" ] || {
  echo "FAIL: the mixer's nixpkgs ($pin) is not the demo's ($demo)" >&2
  exit 1
}

bin="$(nix build --no-link --print-out-paths "$pin#ffmpeg-headless.bin")/bin"
export DEMO_SOUND_FFMPEG_BIN="$bin"
ff() { "$bin/ffmpeg" -hide_banner -v error -nostdin -y "$@"; }

work=$(mktemp -d "${TMPDIR:-/tmp}/demo-sound-test.XXXXXX")
trap 'rm -rf -- "$work"' EXIT
mkdir -p -- "$work/pack"

ff -f lavfi -i "testsrc=size=640x400:rate=60" -t 4 -c:v libx264 -preset ultrafast \
  -pix_fmt yuv420p -threads 1 -fflags +bitexact -flags:v +bitexact -map_metadata -1 \
  -- "$work/video.mp4"
# Four 80 ms blocks: a 1 kHz blip from 0 to 20 ms and another from 40 to
# 50 ms of each, silence between.
blip="0.5*sin(2*PI*1000*t)*(lt(mod(t\,0.08)\,0.02)+between(mod(t\,0.08)\,0.04\,0.05))"
ff -f lavfi -i "aevalsrc=${blip}|${blip}:s=48000:d=0.32" -c:a pcm_f32le -- "$work/pack/blips.wav"
{
  printf '{"audio_file": "blips.wav", "config_version": "2", "definition_method": "single",\n'
  printf ' "author": "test", "name": "blips", "definitions": {\n'
  n=0
  for k in Enter KeyA KeyB KeyC; do
    b=$((n * 80))
    [ "$n" -eq 0 ] || printf ',\n'
    printf '  "%s": {"timing": [[%d, %d], [%d, %d]]}' "$k" "$b" $((b + 20)) $((b + 40)) $((b + 50))
    n=$((n + 1))
  done
  printf '\n}}\n'
} >"$work/pack/config.json"

timeline() {
  printf 't_ms\tevent\tdetail\tx_px\ty_px\tx2_px\ty2_px\n'
  printf '%s\tkey\t-\t100\t380\t\t\n' "$1"
  printf '500\ttrail\tplasma\t0\t380\t640\t20\n'
  printf '700\tkey\t-\t120\t380\t\t\n'
  printf '1000\tcopy\t-\t10\t20\t200\t60\n'
  printf '1300\tkey\t-\t140\t380\t\t\n'
  printf '1700\tlight\tCD on\t\t\t\t\n'
  printf '2100\tenter\t-\t160\t380\t\t\n'
  printf '2500\tpaste\t-\t200\t380\t\t\n'
  printf '2900\tkey\t-\t180\t380\t\t\n'
}
timeline 200 >"$work/tour.timeline.tsv"
timeline 300 >"$work/moved.timeline.tsv"

cp -- "$work/video.mp4" "$work/refused.mp4"
if "$mixer" "$work/refused.mp4" "$work/tour.timeline.tsv" "$work/pack" >/dev/null 2>&1; then
  echo "FAIL: the mixer took a pack that is not the pinned one, without --unpinned" >&2
  exit 1
fi

# mix NAME TIMELINE: NAME.mp4 from the testsrc, and its two versions.
mix() {
  cp -- "$work/video.mp4" "$work/$1.mp4"
  "$mixer" --unpinned "$work/$1.mp4" "$work/$2.timeline.tsv" "$work/pack" >/dev/null
}
mix one tour
mix two tour
mix three moved
sum() { shasum -a 256 <"$work/$1" | cut -d' ' -f1; }
[ "$(sum one.mp4)" = "$(sum video.mp4)" ] || {
  echo "FAIL: the mixer changed TOUR.mp4" >&2
  exit 1
}
for v in silent clicks; do
  [ "$(sum "one.$v.mp4")" = "$(sum "two.$v.mp4")" ] || {
    echo "FAIL ($v): the same inputs mixed twice made different bytes" >&2
    exit 1
  }
done
[ "$(sum one.silent.mp4)" = "$(sum three.silent.mp4)" ] || {
  echo "FAIL (silent): a key moved changed the silent version" >&2
  exit 1
}
[ "$(sum one.clicks.mp4)" != "$(sum three.clicks.mp4)" ] || {
  echo "FAIL (clicks): a key moved 100 ms made the same bytes, so the comparison cannot fail" >&2
  exit 1
}

said=""
vbytes() { ff -i "$1" -map 0:v:0 -c copy -f h264 -; }
for v in silent clicks; do
  out="$work/one.$v.mp4"
  probe() { "$bin/ffprobe" -v error "$@" -of csv=p=0 -- "$out"; }
  streams=$(probe -show_entries stream=codec_type,codec_name | sort | tr '\n' ' ')
  [ "$streams" = "aac,audio h264,video " ] || {
    echo "FAIL ($v): the MP4's streams are $streams" >&2
    exit 1
  }
  audio=$(probe -select_streams a:0 -show_entries stream=duration)
  video=$(probe -select_streams v:0 -show_entries stream=duration)
  awk -v a="$audio" -v v="$video" 'BEGIN { d = a - v; if (d < 0) d = -d; exit !(d <= 0.05) }' || {
    echo "FAIL ($v): the audio runs $audio s and the video $video s" >&2
    exit 1
  }
  [ "$(vbytes "$out" | shasum -a 256)" = "$(vbytes "$work/video.mp4" | shasum -a 256)" ] || {
    echo "FAIL ($v): the mixer changed the video's bytes" >&2
    exit 1
  }
  said="$said; $v: audio $audio s for video $video s, the same bytes twice"
done

# The silent version's samples, decoded: none is anything but zero bytes.
ff -i "$work/one.silent.mp4" -map 0:a:0 -f f32le -ac 2 -ar 48000 -- "$work/silent.f32"
bytes=$(wc -c <"$work/silent.f32" | tr -d ' ')
loud=$(tr -d '\000' <"$work/silent.f32" | wc -c | tr -d ' ')
video=$("$bin/ffprobe" -v error -select_streams v:0 -show_entries stream=duration -of csv=p=0 -- "$work/video.mp4")
awk -v b="$bytes" -v v="$video" 'BEGIN { exit !(b / 8 >= int(v * 48000)) }' || {
  echo "FAIL (silent): $bytes bytes decoded, shorter than the video's $video s" >&2
  exit 1
}
[ "$loud" -eq 0 ] || {
  echo "FAIL (silent): $loud of its $bytes bytes are not zero" >&2
  exit 1
}

# The clicks version's sounds: where each silence ends, in seconds, but the
# silence the stream ends in, which silencedetect ends at the stream's end.
onsets=$("$bin/ffmpeg" -hide_banner -nostdin -i "$work/one.clicks.mp4" -map 0:a:0 \
  -af silencedetect=noise=-60dB:d=0.1 -f null - 2>&1 |
  sed -n 's/.*silence_end: \([0-9.]*\).*/\1/p' |
  awk -v v="$video" '$1 < v - 0.01' | tr '\n' ' ')
awk -v got="$onsets" 'BEGIN {
    n = split(got, g, " "); split("0.200 0.700 1.300 2.100 2.900", w, " ")
    if (n != 5) exit 1
    for (k = 1; k <= 5; k++) { d = g[k] - w[k]; if (d < 0) d = -d; if (d > 0.005) exit 1 }
}' || {
  echo "FAIL (clicks): the sounds start at $onsets, not 0.200 0.700 1.300 2.100 2.900 s" >&2
  exit 1
}
echo "ok: TOUR.mp4 kept, a pack not pinned refused, aac beside h264${said}; the silent version decodes to $bytes zero bytes; the clicks start at ${onsets% }s"
