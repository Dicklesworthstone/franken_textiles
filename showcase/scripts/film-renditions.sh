#!/bin/sh
# Write the 1080p60 copy of every 4K film in public/media, for devices that cannot decode 3840x2160 at 60 fps.
# The players in components/InlineFilm.tsx choose between the two.
#
#   sh scripts/film-renditions.sh [media-directory]
#
# Each copy records the SHA-256 of the film it was made from, so a re-rendered film gets a new copy and an
# unchanged one is left alone. Needs ffmpeg with libx264, ffprobe and sha256sum.
set -eu
cd "${1:-$(dirname "$0")/../public/media}"
video_format() {
  # The CSV writer adds an empty column for browser captures with an ICC profile.
  ffprobe -v error -select_streams v:0 -show_entries "stream=width,height,r_frame_rate,avg_frame_rate,nb_frames${2:-}" \
    -of default=nw=1:nk=1 "$1"
}
decoded_frames() {
  # A fast-start MP4 can keep its entire sample table and source comment after truncation.
  progress=$(ffmpeg -nostdin -hide_banner -loglevel error -xerror -threads 2 -i "$1" \
    -map 0:v:0 -an -fps_mode passthrough -nostats -progress pipe:1 -f null -) || return
  printf '%s\n' "$progress" | awk -F= '$1 == "frame" { frames = $2 + 0 } $0 == "progress=end" { print frames }'
}
for film in ./*.mp4; do
  case $film in *-1080.mp4) continue ;; esac
  [ -f "$film" ] || continue
  format=$(video_format "$film")
  frames=$(printf '%s\n' "$format" | tail -n 1)
  [ "$format" = "$(printf '%s\n' 3840 2160 60/1 60/1 "$frames")" ] || continue
  case $frames in
    ''|0|*[!0-9]*) printf 'Missing a valid frame count in %s.\n' "$film" >&2; exit 1 ;;
  esac
  # A cached file must match the format the player uses on devices with limited decoding support.
  rendition_fields=,codec_name,profile,pix_fmt,level
  expected=$(printf '%s\n' h264 High 1920 1080 yuv420p 42 60/1 60/1 "$frames")
  copy=${film%.mp4}-1080.mp4
  digest=$(sha256sum "$film" | cut -d' ' -f1)
  source="FrankenTextiles 1080p copy of sha256 $digest"
  if [ -e "$copy" ] && [ "$(video_format "$copy" "$rendition_fields")" = "$expected" ] &&
    [ "$(ffprobe -v error -show_entries format_tags=comment -of csv=p=0 "$copy")" = "$source" ] &&
    [ "$(decoded_frames "$copy")" = "$frames" ]; then
    continue
  fi
  pending=$(mktemp "./.${copy##*/}.XXXXXX")
  # Level 4.2 is the lowest that carries 1920x1080 at 60 fps, and what nearly every phone decodes in hardware.
  if ! ffmpeg -nostdin -hide_banner -loglevel error -xerror -y -i "$film" -map 0:v:0 -an \
    -vf scale=1920:1080:flags=lanczos:out_range=tv \
    -c:v libx264 -preset slow -crf 20 -maxrate 4500k -bufsize 9M -profile:v high -level:v 4.2 \
    -pix_fmt yuv420p -color_range tv -metadata comment="$source" -movflags +faststart -f mp4 "$pending"; then
    printf 'Failed to encode %s. Unpublished output is in %s.\n' "$copy" "$pending" >&2
    exit 1
  fi
  if [ "$(video_format "$pending" "$rendition_fields")" != "$expected" ]; then
    printf 'Unexpected format or frame count in %s. Unpublished output is in %s.\n' "$copy" "$pending" >&2
    exit 1
  fi
  if [ "$(sha256sum "$film" | cut -d' ' -f1)" != "$digest" ]; then
    printf 'The source changed during encoding. Unpublished output is in %s.\n' "$pending" >&2
    exit 1
  fi
  chmod 644 "$pending"
  mv -fT "$pending" "$copy"
  echo "$copy"
done
