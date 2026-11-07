#!/usr/bin/env bash
# demo-sound.sh: the tour's sound, laid from its timeline and muxed into it.
#
#   scripts/demo-sound.sh [--bed BED.wav] TOUR.mp4 TOUR.timeline.tsv SOUNDS/
#
# TOUR.mp4 and TOUR.timeline.tsv are one take of the demo's `--record
# TOUR.mp4` (baud-app/examples/demo/record.rs, timeline.rs): the timeline's
# times are milliseconds from the film's first frame, the MP4's own clock,
# and its positions are the MP4's pixels. The tour has no music: with no
# `--bed`, the effects are laid over silence. `--bed BED.wav` lays them
# over that file instead. SOUNDS/ holds one file an effect, by these
# names; none of them, and no music, is in the repository, and nothing
# here makes them:
#
#   key.wav           a key typed, recorded from a real keyboard
#   enter.wav         Return, recorded from the same keyboard
#   trail-plasma.wav  an electric hum and swish, the plasma trail's jump
#   trail-liquid.wav  a wet glide, the liquid trail's
#   trail.wav         any other trail's
#   copy.wav          a two-note chime, a copy
#   paste.wav         one note, a paste
#   modem.wav         a modem's handshake, ending as CD lights
#   fax.wav           a fax's chirp, a picture shown
#   pop.wav           a firework's burst
#
# What it does, row by row of the timeline:
#   - key and enter: their sound at the cursor's x, quiet, each pitched
#     within 8% either way by a number drawn from a generator seeded with
#     the timeline's SHA-256, so the same timeline is the same pitches;
#   - trail: its kind's sound, panned from x to x2 over the sound's length;
#   - copy at the selection's x, paste at the cursor's, fax at the
#     picture's, quiet; pop at each burst's x, medium, at most three in any
#     second, the earliest kept;
#   - `light CD on`: the handshake, centred and loud, placed to end there;
#   - with a bed only, `video start` to `video end`: the bed at half for
#     their length.
# With a bed, every effect but a key's ducks it 6 dB for its own length; a
# key does not, or typing would pump the bed. With none, nothing is ducked.
# Pan is equal power, by x over the MP4's width. The bed, or the silence,
# is cut or padded with silence to the MP4's length.
# The mix is limited, encoded as AAC and muxed beside the MP4's video,
# which is copied, into a file in a directory of its own, then moved over
# TOUR.mp4: run again, it makes the same bytes, the video being the same.
#
# The ffmpeg is the demo's, from the nixpkgs revision in NIXPKGS below,
# which must be run.rs's (scripts/demo-sound-test.sh holds the two equal),
# and every encoder is told to be bit-exact: the same inputs give the
# same bytes. UFO rows are not sounded: the take writes none yet.
set -euo pipefail

NIXPKGS="github:NixOS/nixpkgs/319e4cebef10cbb636c6ea392137b52ad105336a"
FFMPEG_VERSION="ffmpeg version 9.0.1"
RATE=48000
SOUNDS="key enter trail-plasma trail-liquid trail copy paste modem fax pop"

usage() {
  echo "usage: scripts/demo-sound.sh [--bed BED.wav] TOUR.mp4 TOUR.timeline.tsv SOUNDS/" >&2
  echo "SOUNDS/ holds:$(for s in $SOUNDS; do printf ' %s.wav' "$s"; done)" >&2
  exit 2
}

bed=""
while [ "$#" -gt 0 ]; do
  case $1 in
  --bed)
    [ "$#" -ge 2 ] || usage
    bed=$2
    shift 2
    ;;
  --)
    shift
    break
    ;;
  -*) usage ;;
  *) break ;;
  esac
done
[ "$#" -eq 3 ] || usage
tour=$1
timeline=$2
sounds=${3%/}
for f in "$tour" "$timeline" ${bed:+"$bed"}; do
  [ -f "$f" ] || {
    echo "demo-sound: no file $f" >&2
    exit 2
  }
done
missing=""
for s in $SOUNDS; do
  [ -f "$sounds/$s.wav" ] || missing="$missing $s.wav"
done
if [ -n "$missing" ]; then
  echo "demo-sound: $sounds lacks:$missing" >&2
  usage
fi
head -n 1 -- "$timeline" | grep -qx $'t_ms\tevent\tdetail\tx_px\ty_px\tx2_px\ty2_px' || {
  echo "demo-sound: $timeline does not start with the timeline's header" >&2
  exit 2
}

if [ -n "${DEMO_SOUND_FFMPEG_BIN:-}" ]; then
  bin=$DEMO_SOUND_FFMPEG_BIN
else
  bin="$(nix build --no-link --print-out-paths "$NIXPKGS#ffmpeg-headless.bin")/bin"
fi
ffmpeg="$bin/ffmpeg"
ffprobe="$bin/ffprobe"
"$ffmpeg" -hide_banner -version | head -n 1 | grep -q "^$FFMPEG_VERSION" || {
  echo "demo-sound: $ffmpeg is not $FFMPEG_VERSION, the pinned one" >&2
  exit 2
}

work=$(mktemp -d "${TMPDIR:-/tmp}/demo-sound.XXXXXX")
trap 'rm -rf -- "$work"' EXIT

probe() { "$ffprobe" -v error "$@" -of csv=p=0 -- "$tour"; }
width=$(probe -select_streams v:0 -show_entries stream=width)
length=$(probe -show_entries format=duration)
seed=$(shasum -a 256 -- "$timeline" | cut -c1-8)
seed=$((16#$seed))

# Each sound's length, for the sweeps, the ducks and the handshake's end.
lengths="$work/lengths"
: >"$lengths"
for s in $SOUNDS; do
  printf '%s\t%s\n' "$s" \
    "$("$ffprobe" -v error -show_entries format=duration -of csv=p=0 -- "$sounds/$s.wav")" \
    >>"$lengths"
done

# The placements, one a line: sound, start s, rate factor, pan from, pan
# to, gain, ducks (1 or 0); and the video spans, `video START END`.
plan="$work/plan"
awk -F '\t' -v seed="$seed" -v width="$width" -v lengths="$lengths" '
    BEGIN {
        while ((getline l < lengths) > 0) { split(l, f, "\t"); len[f[1]] = f[2] + 0 }
        state = seed % 4294967296
        pops = 0
    }
    function rnd() {
        state = (1664525 * state + 1013904223) % 4294967296
        return state / 4294967296
    }
    function pan(x) {
        if (x == "" || width <= 0) return 0.5
        p = x / width
        return p < 0 ? 0 : (p > 1 ? 1 : p)
    }
    function place(s, t, rate, p0, p1, g, duck) {
        if (t < 0) t = 0
        printf "%s\t%.6f\t%.6f\t%.6f\t%.6f\t%.3f\t%d\n", s, t, rate, p0, p1, g, duck
    }
    NR == 1 { next }
    {
        t = $1 / 1000; ev = $2; d = $3
        if (ev == "key" || ev == "enter") {
            place(ev, t, 1 + 0.08 * (2 * rnd() - 1), pan($4), pan($4), 0.35, 0)
        } else if (ev == "trail") {
            s = "trail-" d
            if (!(s in len)) s = "trail"
            place(s, t, 1, pan($4), pan($6 == "" ? $4 : $6), 0.6, 1)
        } else if (ev == "copy") {
            place("copy", t, 1, pan($4), pan($4), 0.3, 1)
        } else if (ev == "paste") {
            place("paste", t, 1, pan($4), pan($4), 0.3, 1)
        } else if (ev == "picture" && d == "shown") {
            place("fax", t, 1, pan($6 == "" ? $4 : ($4 + $6) / 2), pan($6 == "" ? $4 : ($4 + $6) / 2), 0.3, 1)
        } else if (ev == "fireworks") {
            n = 0
            for (k = 0; k < pops; k++) if (popt[k] > t - 1) n++
            if (n < 3) { popt[pops++] = t; place("pop", t, 1, pan($4), pan($4), 0.5, 1) }
        } else if (ev == "light" && d == "CD on") {
            place("modem", t - len["modem"], 1, 0.5, 0.5, 0.9, 1)
        } else if (ev == "video" && d == "start") {
            vstart = t
        } else if (ev == "video" && d == "end" && vstart != "") {
            printf "video\t%.6f\t%.6f\n", vstart, t
            vstart = ""
        }
    }
    END { if (vstart != "") printf "video\t%.6f\t%.6f\n", vstart, -1 }
' "$timeline" >"$plan"

# The graph: one input a sound file, split to its uses; the bed, if there
# is one, ducked and halved under the video; if not, silence, untouched.
graph="$work/graph"
inputs=()
i=0
for s in $SOUNDS; do
  uses=$(awk -F '\t' -v s="$s" '$1 == s' "$plan" | wc -l | tr -d ' ')
  [ "$uses" -gt 0 ] || continue
  inputs+=(-i "$sounds/$s.wav")
  i=$((i + 1))
done
awk -F '\t' -v rate="$RATE" -v sounds="$SOUNDS" -v lengths="$lengths" -v total="$length" \
  -v bedded="${bed:+1}" '
    BEGIN {
        while ((getline l < lengths) > 0) { split(l, f, "\t"); len[f[1]] = f[2] + 0 }
        n = split(sounds, list, " ")
        nr = 0
        nv = 0
    }
    $1 == "video" { vs[nv] = $2; ve[nv] = ($3 < 0 ? total : $3); nv++; next }
    { row[nr] = $0; uses[$1]++; nr++ }
    END {
        idx = 1
        for (k = 1; k <= n; k++) if (uses[list[k]] > 0) {
            s = list[k]
            printf "[%d:a]aformat=sample_fmts=fltp:sample_rates=%d:channel_layouts=mono,asplit=%d", idx, rate, uses[s]
            for (u = 0; u < uses[s]; u++) printf "[%s_%d]", s, u
            printf ";\n"
            idx++
        }
        duck = ""
        for (r = 0; r < nr; r++) {
            split(row[r], f, "\t")
            s = f[1]; t = f[2]; rt = f[3]; p0 = f[4]; p1 = f[5]; g = f[6]; d = f[7]
            u = seen[s]++
            dur = len[s] / rt
            ms = sprintf("%d", t * 1000 + 0.5)
            p = sprintf("(%s+(%s)*min(t/%.6f\\,1))", p0, p1 - p0, dur)
            printf "[%s_%d]asetrate=%d*%s,aresample=%d,asplit=2[l%d][r%d];\n", s, u, rate, rt, rate, r, r
            printf "[l%d]volume=%s*cos(%s*PI/2):eval=frame[L%d];\n", r, g, p, r
            printf "[r%d]volume=%s*sin(%s*PI/2):eval=frame[R%d];\n", r, g, p, r
            printf "[L%d][R%d]join=inputs=2:channel_layout=stereo,adelay=%s|%s[s%d];\n", r, r, ms, ms, r
            if (d == 1) duck = duck sprintf("+between(t\\,%.6f\\,%.6f)", t, t + dur)
        }
        vid = ""
        for (k = 0; k < nv; k++) vid = vid sprintf("+between(t\\,%.6f\\,%.6f)", vs[k], ve[k])
        printf "[0:a]aformat=sample_fmts=fltp:sample_rates=%d:channel_layouts=stereo,apad,atrim=0:%s", rate, total
        if (bedded == 1) printf ",volume=(1-0.49881*gt(0%s\\,0))*(1-0.5*gt(0%s\\,0)):eval=frame", duck, vid
        printf "[bed];\n"
        printf "[bed]"
        for (r = 0; r < nr; r++) printf "[s%d]", r
        printf "amix=inputs=%d:normalize=0:duration=first:dropout_transition=0,", nr + 1
        printf "alimiter=limit=0.89:level=disabled,atrim=0:%s[mix]\n", total
    }
' "$plan" >"$graph"

if [ -n "$bed" ]; then
  base=(-i "$bed")
else
  base=(-f lavfi -i "anullsrc=channel_layout=stereo:sample_rate=$RATE")
fi
out="$work/tour.mp4"
"$ffmpeg" -hide_banner -v error -nostdin -y \
  "${base[@]}" "${inputs[@]}" -i "$tour" \
  -filter_threads 1 -filter_complex_threads 1 \
  -/filter_complex "$graph" \
  -map "$((i + 1)):v:0" -map "[mix]" \
  -c:v copy -c:a aac -b:a 192k -ar "$RATE" -threads 1 \
  -fflags +bitexact -flags:a +bitexact -map_metadata -1 -map_chapters -1 \
  -movflags +faststart -f mp4 -- "$out"
mv -f -- "$out" "$tour"
echo "demo-sound: $(wc -l <"$plan" | tr -d ' ') sounds laid under $tour, $length s, ${bed:+over $bed}${bed:-no bed}"
