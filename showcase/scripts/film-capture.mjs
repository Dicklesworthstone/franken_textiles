import sharp from "sharp";

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

// Camera controls already synchronize the scene before each exposure. Capture its
// backing surface directly, without waiting for Playwright's element stability checks.
export async function frameCapture(page) {
  const session = await page.context().newCDPSession(page);
  const { width, height } = page.viewportSize();
  const deviceScaleFactor = await page.evaluate(() => window.devicePixelRatio);
  // Emulation belongs to the DevTools session. Without it this session reads CSS-sized pixels.
  await session.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor, mobile: false });
  return async (quality = 94) => {
    const { cssVisualViewport: view } = await session.send("Page.getLayoutMetrics");
    const { data } = await session.send("Page.captureScreenshot", {
      format: "jpeg", quality, fromSurface: true, captureBeyondViewport: false, optimizeForSpeed: true,
      clip: { x: view.pageX, y: view.pageY, width: width / view.scale, height: height / view.scale, scale: view.scale },
    });
    const bytes = Buffer.from(data, "base64");
    const image = await sharp(bytes).metadata();
    if (image.width !== filmWidth || image.height !== filmHeight) {
      throw new Error(`Chromium captured ${image.width}×${image.height}; expected ${filmWidth}×${filmHeight}.`);
    }
    return bytes;
  };
}

export const fitUhd = `scale=${filmWidth}:${filmHeight}:force_original_aspect_ratio=decrease:force_divisible_by=2:out_range=tv,pad=${filmWidth}:${filmHeight}:(ow-iw)/2:(oh-ih)/2,setsar=1`;

export const encodeUhd = [
  "-an", "-c:v", "libx264", "-threads", "4", "-preset", "veryfast", "-crf", "17",
  "-maxrate", "9M", "-bufsize", "9M", "-profile:v", "high", "-level:v", "5.2",
  "-pix_fmt", "yuv420p", "-color_range", "tv", "-movflags", "+faststart",
];

export function filmArgs(output) {
  return ["-hide_banner", "-loglevel", "error", "-n", "-filter_threads", "2",
    "-f", "image2pipe", "-c:v", "mjpeg", "-probesize", "64k", "-analyzeduration", "0",
    "-framerate", String(fps), "-i", "pipe:0", "-vf", fitUhd, ...encodeUhd, output];
}
