#!/bin/sh
# Compare the two published 680 mm waist solves at matching construction phases.
set -eu
root=$(CDPATH='' cd -- "$(dirname -- "$0")/../public/media" && pwd)
media=${FTX_FILM_SOURCE_DIR:-$root}
dest=${1:?choose a fresh output directory}
mkdir -p "$dest"
for cut in column flare; do
  size=$(ffprobe -v error -select_streams v:0 -show_entries stream=width,height,avg_frame_rate,nb_frames \
    -of csv=p=0 "$media/construction-$cut.mp4" | sed 's/,$//')
  if [ "$size" != "3840,2160,60/1,360" ]; then
    echo "Render construction-$cut.mp4 at 3840×2160 and 60 fps first." >&2
    exit 1
  fi
done
ffmpeg -hide_banner -loglevel error -n -filter_complex_threads 2 \
  -threads 2 -i "$media/construction-column.mp4" -threads 2 -i "$media/construction-flare.mp4" \
  -filter_complex "[0:v]crop=1920:2160:960:0[l];[1:v]crop=1920:2160:960:0[r];[l][r]hstack,
drawbox=x=1918:y=0:w=4:h=ih:color=0xe8d8ac@0.65:t=fill,
drawtext=font='Liberation Serif':text='Column skirt':fontsize=76:fontcolor=0xf4eddb:x=88:y=73,
drawtext=font='Liberation Serif':text='Flared skirt':fontsize=76:fontcolor=0xf4eddb:x=2008:y=73,
drawtext=font='monospace':text='68 CM WAIST / 126 CM HEM':fontsize=30:fontcolor=0xe8d8ac:x=94:y=177,
drawtext=font='monospace':text='68 CM WAIST / 168 CM HEM':fontsize=30:fontcolor=0xe8d8ac:x=2014:y=177,
drawtext=font='monospace':text='FLAT PATTERNS':fontsize=39:fontcolor=0xf4eddb:x=92:y=h-104:enable='lt(t,3.3125)',
drawtext=font='monospace':text='SEAMS ALIGN':fontsize=39:fontcolor=0xf4eddb:x=92:y=h-104:enable='between(t,3.3125,3.8125)',
drawtext=font='monospace':text='SEWN ON THE SAME FITTED FORM':fontsize=39:fontcolor=0xf4eddb:x=92:y=h-104:enable='gt(t,3.8125)'[film]" \
  -map '[film]' -an -r 60 -c:v libx264 -threads 4 -preset veryfast -crf 17 -maxrate 9M -bufsize 9M \
  -profile:v high -level:v 5.2 -pix_fmt yuv420p -color_range tv -movflags +faststart "$dest/skirt-atelier.mp4"
ffmpeg -hide_banner -loglevel error -n -threads 2 -ss 5.5 -i "$dest/skirt-atelier.mp4" \
  -frames:v 1 -threads 1 -q:v 2 "$dest/skirt-atelier.jpg"
