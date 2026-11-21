/** Retain cheap interactive buffers. The offline film recorder supplies its own UHD scale. */
export function capturePixelRatio(interactiveLimit: number) {
  const browser = window as Window & { __ftxFilmCapture?: boolean };
  const density = browser.devicePixelRatio || 1;
  return browser.__ftxFilmCapture ? density : Math.min(interactiveLimit, density);
}
