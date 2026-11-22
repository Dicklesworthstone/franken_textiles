// Capture the still shown on each lab exhibit's card on the home page: the exhibit's own stage, as the page
// draws it at its defaults. Pass a new output directory; review the files, then copy them into
// public/media/cards/.
//   FTX_STUDIO_URL=http://127.0.0.1:3000 CHROMIUM_EXECUTABLE=... node scripts/render-exhibit-cards.mjs OUT_DIR
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright-core";
import sharp from "sharp";

// name, route, the element captured, the share of it kept (to leave out edge labels the 3:2 crop would cut
// through), and whether the crop sits at the centre or the foot of that share.
export const cards = [
  ["waistcoat", "/wardrobe/waistcoat/", ".wc-svg", .86, "centre"],
  ["inside-out", "/wardrobe/waistcoat/inside-out/", ".io-canvas-host canvas", 1, "centre"],
  ["sundress-compare", "/wardrobe/sundress/compare/", ".sc-canvas-host canvas", 1, "centre"],
  ["thermal", "/experiments/thermal/", ".th-iso-svg", 1, "centre"],
  ["wash", "/experiments/wash/", ".ws-svg", 1, "centre"],
  ["friction", "/experiments/friction/", ".fs-svg", 1, "centre"],
  ["wrap-skirt", "/studies/wrap-skirt/", ".ws-svg", 1, "centre"],
  ["tiling", "/studies/tiling/", ".tf-svg", .78, "centre"],
  ["sewing", "/studies/sewing/", ".so-flat-svg", 1, "centre"],
];
const width = 840, height = 560; // 3:2, twice the card's widest layout, for dense screens

// Two links under the drape room show a 4K film still at about 100 px wide. These are the same stills at
// four times that width, so the page does not fetch and decode 3840 x 2160 for a thumbnail.
export const strips = [
  ["strip-trouser-posture", "public/media/trouser-posture.jpg"],
  ["strip-rig", "public/media/rig-strip.jpg"],
];

const destination = resolve(process.argv[2]);
await mkdir(destination);
for (const [name, source] of strips) {
  const small = await sharp(resolve(import.meta.dirname, "..", source)).resize(416).webp({ quality: 80 }).toBuffer();
  await writeFile(resolve(destination, `${name}.webp`), small);
  console.log(`${name}.webp ${small.length} bytes from ${source}`);
}
const origin = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
try {
  const context = await browser.newContext({ userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
    viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2, reducedMotion: "reduce" });
  for (const [name, route, selector, share, anchor] of cards) {
    const page = await context.newPage();
    page.setDefaultTimeout(120000);
    await page.goto(new URL(route, origin).href, { waitUntil: "load" });
    const stage = page.locator(selector).first();
    await stage.waitFor();
    await page.evaluate(() => document.fonts.ready);
    // The stages fill in after their data and solver results arrive; a quiet network and a pause cover both.
    await page.waitForLoadState("networkidle").catch(() => {});
    await page.waitForTimeout(4000);
    await stage.scrollIntoViewIfNeeded();
    const shot = await stage.screenshot({ type: "png" });
    // The largest 3:2 rectangle inside the kept share of the element.
    const full = await sharp(shot).metadata();
    const keptWidth = full.width * share, keptHeight = full.height * share;
    const cropWidth = Math.floor(Math.min(keptWidth, keptHeight * width / height));
    const cropHeight = Math.floor(cropWidth * height / width);
    const left = Math.floor((full.width - cropWidth) / 2);
    const top = Math.floor(anchor === "foot" ? (full.height + keptHeight) / 2 - cropHeight : (full.height - cropHeight) / 2);
    const still = await sharp(shot).extract({ left, top, width: cropWidth, height: cropHeight })
      .resize(width, height).webp({ quality: 82 }).toBuffer();
    await writeFile(resolve(destination, `${name}.webp`), still);
    console.log(`${name}.webp ${still.length} bytes from ${selector}`);
    await page.close();
  }
} finally {
  await browser.close();
}
