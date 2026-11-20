#!/bin/sh
# Assemble existing gallery renders into the home page loop. Outputs must be new.
set -eu
media="$(CDPATH='' cd -- "$(dirname -- "$0")/../public/media" && pwd)"
dest="${1:-$media}"
mkdir -p "$dest"
ffmpeg -hide_banner -loglevel error -n \
  -ss 0.2 -t 3 -i "$media/construction-sundress.mp4" \
  -ss 3.0 -t 3 -i "$media/construction-blue.mp4" \
  -ss 1.2 -t 2.5 -i "$media/rig-cylinder.mp4" \
  -ss 2.8 -t 2.5 -i "$media/bolero.mp4" \
  -ss 1 -t 2.5 -i "$media/rig-strip.mp4" \
  -filter_complex "[0:v]fps=24,scale=960:540:force_original_aspect_ratio=increase,crop=960:540,setsar=1[a];[1:v]fps=24,scale=960:540:force_original_aspect_ratio=increase,crop=960:540,setsar=1[b];[2:v]fps=24,crop=iw:ih*0.85:0:ih*0.10,scale=960:540:force_original_aspect_ratio=increase,crop=960:540,setsar=1[c];[3:v]fps=24,scale=960:540,setsar=1[d];[4:v]fps=24,crop=iw:ih*0.85:0:ih*0.10,scale=960:540:force_original_aspect_ratio=increase,crop=960:540,setsar=1[e];[a][b][c][d][e]concat=n=5:v=1:a=0[out]" \
  -map "[out]" -an -c:v libx264 -preset medium -crf 20 -pix_fmt yuv420p -movflags +faststart "$dest/wardrobe-reel.mp4"
ffmpeg -hide_banner -loglevel error -n -ss 0.5 -i "$dest/wardrobe-reel.mp4" -frames:v 1 -q:v 2 "$dest/wardrobe-reel.jpg"
