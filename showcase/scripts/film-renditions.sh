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
uhd=$(printf '%s\n' 3840 2160 60/1 60/1)
for film in ./*.mp4; do
  case $film in *-1080.mp4) continue ;; esac
  [ -f "$film" ] || continue
  # The CSV writer adds an empty column for browser captures with an ICC profile.
  [ "$(ffprobe -v error -select_streams v:0 -show_entries stream=width,height,r_frame_rate,avg_frame_rate \
    -of default=nw=1:nk=1 "$film")" = "$uhd" ] || continue
  copy=${film%.mp4}-1080.mp4
  source="FrankenTextiles 1080p copy of sha256 $(sha256sum "$film" | cut -d' ' -f1)"
  if [ -e "$copy" ] && [ "$(ffprobe -v error -show_entries format_tags=comment -of csv=p=0 "$copy")" = "$source" ]; then
    continue
  fi
  # Level 4.2 is the lowest that carries 1920x1080 at 60 fps, and what nearly every phone decodes in hardware.
  ffmpeg -hide_banner -loglevel error -y -i "$film" -an -vf scale=1920:1080:flags=lanczos:out_range=tv \
    -c:v libx264 -preset slow -crf 20 -maxrate 4500k -bufsize 9M -profile:v high -level:v 4.2 \
    -pix_fmt yuv420p -color_range tv -metadata comment="$source" -movflags +faststart "$copy"
  echo "$copy"
done
