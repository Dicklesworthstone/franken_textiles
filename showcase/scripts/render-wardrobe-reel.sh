#!/bin/sh
# Assemble existing gallery renders into the home page loop. Outputs must be new.
set -eu
media="${FTX_FILM_SOURCE_DIR:-$(CDPATH='' cd -- "$(dirname -- "$0")/../public/media" && pwd)}"
dest="${1:-$media}"
mkdir -p "$dest"
for title in construction-sundress construction-blue rig-cylinder bolero rig-strip; do
  probe="$(ffprobe -v error -select_streams v:0 -show_entries stream=width,height,avg_frame_rate -of csv=p=0 "$media/$title.mp4")"
  if [ "$probe" != "3840,2160,60/1" ]; then
    echo "Render $title.mp4 in 4K60 before assembling the reel." >&2
    exit 1
  fi
done
ffmpeg -hide_banner -loglevel error -n \
  -ss 0.2 -t 3 -i "$media/construction-sundress.mp4" \
  -ss 3.0 -t 3 -i "$media/construction-blue.mp4" \
  -ss 1.2 -t 2.5 -i "$media/rig-cylinder.mp4" \
  -ss 2.8 -t 2.5 -i "$media/bolero.mp4" \
  -ss 1 -t 2.5 -i "$media/rig-strip.mp4" \
  -filter_complex_threads 2 -filter_complex "[0:v][1:v][2:v][3:v][4:v]concat=n=5:v=1:a=0[out]" \
  -map "[out]" -an -r 60 -c:v libx264 -threads 4 -preset veryfast -crf 17 -maxrate 9M -bufsize 9M \
  -profile:v high -level:v 5.2 -pix_fmt yuv420p -movflags +faststart "$dest/wardrobe-reel.mp4"
ffmpeg -hide_banner -loglevel error -n -ss 0.5 -i "$dest/wardrobe-reel.mp4" -frames:v 1 -q:v 2 "$dest/wardrobe-reel.jpg"
