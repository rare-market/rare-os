#!/usr/bin/env bash
# Builds the Rare OS branding files that replace the base's (Bazzite's) artwork in the image:
#   steam-movies/  Steam's startup and suspend clips (game mode / Big Picture), with posters
#   icons/         the Rare mark as start-here / distributor-logo icons (PNG sizes + SVG)
#   wallpaper/     the desktop mode wallpaper (KDE wallpaper package)
# Source: launcher/public/brand/mark.png. Needs ffmpeg with libvpx and libvorbis.
#   tools/brand/make-os-assets.sh [out-dir]   (default image/system_files/usr/share/rare/brand)
set -euo pipefail
cd "$(dirname "$0")/../.."
MARK=launcher/public/brand/mark.png
OUT="${1:-image/system_files/usr/share/rare/brand}"
BG=0x000000        # Steam clips: flat black, like Steam's own fade
WALL_BG=0x100c18   # same colour as the boot splash
mkdir -p "$OUT/steam-movies" "$OUT/icons" "$OUT/wallpaper/contents/images"
ff() { ffmpeg -hide_banner -loglevel error -y "$@"; }

# Startup: 1 s at 60 fps, 1280x800 (Steam's own size), the mark fades in, holds, fades out.
# A silent audio track keeps the file shaped like Steam's clips; nothing is heard.
ff -f lavfi -i "color=c=${BG}:s=1280x800:r=60:d=1" -loop 1 -framerate 60 -t 1 -i "$MARK" \
   -f lavfi -t 1 -i anullsrc=r=48000:cl=stereo \
   -filter_complex "[1:v]scale=-1:176,format=rgba,fade=t=in:st=0:d=0.3:alpha=1,fade=t=out:st=0.72:d=0.28:alpha=1[m];[0:v][m]overlay=(W-w)/2:(H-h)/2:shortest=1,format=yuv420p[v]" \
   -map "[v]" -map 2:a -c:v libvpx -b:v 1M -crf 10 -auto-alt-ref 0 -c:a libvorbis -q:a 0 -shortest \
   "$OUT/steam-movies/rare-startup.webm"
ff -f lavfi -i "color=c=${BG}:s=1280x800" -i "$MARK" \
   -filter_complex "[1:v]scale=-1:176[m];[0:v][m]overlay=(W-w)/2:(H-h)/2" -frames:v 1 -q:v 3 \
   "$OUT/steam-movies/rare-startup.jpg"

# Suspend: 1 s at 60 fps, 450x450 (Steam's size), the mark fades to black.
ff -f lavfi -i "color=c=${BG}:s=450x450:r=60:d=1" -loop 1 -framerate 60 -t 1 -i "$MARK" \
   -f lavfi -t 1 -i anullsrc=r=48000:cl=stereo \
   -filter_complex "[1:v]scale=-1:150,format=rgba,fade=t=out:st=0.15:d=0.8:alpha=1[m];[0:v][m]overlay=(W-w)/2:(H-h)/2:shortest=1,format=yuv420p[v]" \
   -map "[v]" -map 2:a -c:v libvpx -b:v 500k -crf 10 -auto-alt-ref 0 -c:a libvorbis -q:a 0 -shortest \
   "$OUT/steam-movies/rare-suspend.webm"
ff -f lavfi -i "color=c=${BG}:s=450x450" -i "$MARK" \
   -filter_complex "[1:v]scale=-1:150[m];[0:v][m]overlay=(W-w)/2:(H-h)/2" -frames:v 1 -q:v 3 \
   "$OUT/steam-movies/rare-suspend.jpg"

# Icons: the mark centred on a transparent square, one PNG per hicolor size, plus an SVG
# (the 256 px PNG embedded) for the scalable directories and the KDE splash.
for s in 16 22 24 32 36 48 64 96 128 256; do
    inner=$(( s * 15 / 16 )); inner=$(( inner < 1 ? 1 : inner ))
    ff -i "$MARK" -vf "scale=-1:${inner}:flags=lanczos,format=rgba,pad=${s}:${s}:(ow-iw)/2:(oh-ih)/2:color=0x00000000" \
       -frames:v 1 "$OUT/icons/${s}.png"
done
{
    printf '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="256" height="256" viewBox="0 0 256 256">'
    printf '<image width="256" height="256" xlink:href="data:image/png;base64,'
    base64 < "$OUT/icons/256.png" | tr -d '\n'
    printf '"/></svg>\n'
} > "$OUT/icons/rare-logo.svg"

# Wallpaper: flat, the boot splash's colour, the mark small in the middle (2560x1600, 16:10 like
# the panel; KDE crops it for 16:9 screens).
ff -f lavfi -i "color=c=${WALL_BG}:s=2560x1600" -i "$MARK" \
   -filter_complex "[1:v]scale=-1:240:flags=lanczos[m];[0:v][m]overlay=(W-w)/2:(H-h)/2,format=rgb24" -frames:v 1 \
   "$OUT/wallpaper/contents/images/2560x1600.png"
ff -i "$OUT/wallpaper/contents/images/2560x1600.png" -vf scale=400:250 -frames:v 1 "$OUT/wallpaper/contents/screenshot.png"
cat > "$OUT/wallpaper/metadata.json" <<'EOF'
{
    "KPlugin": {
        "Authors": [{ "Name": "Rare" }],
        "Id": "Rare",
        "License": "CC-BY-SA-4.0",
        "Name": "Rare"
    }
}
EOF
ls -l "$OUT"/steam-movies "$OUT"/icons "$OUT"/wallpaper/contents/images
