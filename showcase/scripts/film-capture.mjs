// Keep the existing CSS camera and typography at their design size. Chromium rasterizes it in UHD.
export const fps = 60;
export const filmWidth = 3840;
export const filmHeight = 2160;
export const filmUserAgent = "OpenAI File Downloader, XaiImageApiFetch/1.0";

export function captureOptions(viewport, deviceScaleFactor = filmWidth / viewport.width) {
  return { viewport, deviceScaleFactor, reducedMotion: "reduce", userAgent: filmUserAgent };
}

export async function enableFilmResolution(page) {
  await page.addInitScript(() => { window.__ftxFilmCapture = true; });
}

export const fitUhd = `scale=${filmWidth}:${filmHeight}:force_original_aspect_ratio=decrease:force_divisible_by=2,pad=${filmWidth}:${filmHeight}:(ow-iw)/2:(oh-ih)/2,setsar=1`;

export const encodeUhd = [
  "-an", "-c:v", "libx264", "-threads", "4", "-preset", "veryfast", "-crf", "17",
  "-maxrate", "9M", "-bufsize", "9M", "-profile:v", "high", "-level:v", "5.2",
  "-pix_fmt", "yuv420p", "-movflags", "+faststart",
];

export function filmArgs(output) {
  return ["-hide_banner", "-loglevel", "error", "-n", "-filter_threads", "2",
    "-f", "image2pipe", "-framerate", String(fps), "-i", "pipe:0", "-vf", fitUhd, ...encodeUhd, output];
}
