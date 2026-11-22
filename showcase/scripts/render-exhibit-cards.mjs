// Make the small stills that stand for an exhibit elsewhere on the site:
//   cards/NAME.webp   the picture on a lab exhibit's card on the home page (840 x 560)
//   index/NAME.webp   the thumbnail beside each entry of the header's index (330 x 220)
//   cards/strip-*.webp the two thumbnails under the drape room (416 px wide)
// A still is either the exhibit's own stage, captured from the served site at its defaults, or an existing
// film still cut down. Pass a new output directory; review the files, then copy cards/ and index/ into
// public/media/.
//   FTX_STUDIO_URL=http://127.0.0.1:3000 CHROMIUM_EXECUTABLE=... node scripts/render-exhibit-cards.mjs OUT_DIR
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright-core";
import sharp from "sharp";

// name, route, the element captured, the share of it kept (to leave out edge labels a 3:2 crop would cut
// through), whether the crop sits at the centre or the foot of that share, and whether it is also a card.
export const captures = [
  ["waistcoat", "/wardrobe/waistcoat/", ".wc-svg", .86, "centre", true],
  ["inside-out", "/wardrobe/waistcoat/inside-out/", ".io-canvas-host canvas", 1, "centre", true],
  ["sundress-compare", "/wardrobe/sundress/compare/", ".sc-canvas-host canvas", 1, "centre", true],
  ["thermal", "/experiments/thermal/", ".th-iso-svg", 1, "centre", true],
  ["wash", "/experiments/wash/", ".ws-svg", 1, "centre", true],
  ["friction", "/experiments/friction/", ".fs-svg", 1, "centre", true],
  ["wrap-skirt", "/studies/wrap-skirt/", ".ws-svg", 1, "centre", true],
  ["tiling", "/studies/tiling/", ".tf-svg", .78, "centre", true],
  ["sewing", "/studies/sewing/", ".so-flat-svg", 1, "centre", true],
  ["drape-room", "/sections/studies/", ".dr-canvas canvas", .8, "centre", false],
  ["redraft", "/wardrobe/sundress/redraft/", ".sr-svg", 1, "centre", false],
  ["embroidery", "/wardrobe/hussar/embroidery/", "main canvas", 1, "centre", false],
  ["tracing", "/sections/tracing/", "main figure", 1, "centre", false],
];
// Index entries that already have a film still or a review picture: name and source file.
export const stills = [
  ["skirt", "public/media/skirt-atelier.jpg"],
  ["trousers", "public/media/trouser-form.jpg"],
  ["visual-fitting", "public/fittings/sleeve-contact-review.png"],
  ["wardrobe", "public/media/wardrobe-hussar.jpg"],
  ["studios", "public/media/lab-draft.webp"],
  ["materials", "public/media/rig-cylinder.jpg"],
  ["drape-gallery", "public/media/bolero.jpg"],
  ["films", "public/media/studio-tour.jpg"],
];
// Two links under the drape room show a 4K film still at about 100 px wide. These are the same stills at
// four times that width, so the page does not fetch and decode 3840 x 2160 for a thumbnail.
export const strips = [
  ["strip-trouser-posture", "public/media/trouser-posture.jpg"],
  ["strip-rig", "public/media/rig-strip.jpg"],
];
const card = { width: 840, height: 560 }, thumb = { width: 330, height: 220 }; // both 3:2

const destination = resolve(process.argv[2]);
await mkdir(destination);
await mkdir(resolve(destination, "cards"));
await mkdir(resolve(destination, "index"));
const save = async (folder, name, image, quality) => {
  const bytes = await image.webp({ quality }).toBuffer();
  await writeFile(resolve(destination, folder, `${name}.webp`), bytes);
  console.log(`${folder}/${name}.webp ${bytes.length} bytes`);
};
const source = file => resolve(import.meta.dirname, "..", file);

for (const [name, file] of strips) await save("cards", name, sharp(source(file)).resize(416), 80);
for (const [name, file] of stills) await save("index", name, sharp(source(file)).resize(thumb.width, thumb.height, { fit: "cover" }), 80);

const origin = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
try {
  const context = await browser.newContext({ userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
    viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2, reducedMotion: "reduce" });
  for (const [name, route, selector, share, anchor, isCard] of captures) {
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
    const cropWidth = Math.floor(Math.min(keptWidth, keptHeight * card.width / card.height));
    const cropHeight = Math.floor(cropWidth * card.height / card.width);
    const left = Math.floor((full.width - cropWidth) / 2);
    const top = Math.floor(anchor === "foot" ? (full.height + keptHeight) / 2 - cropHeight : (full.height - cropHeight) / 2);
    const crop = await sharp(shot).extract({ left, top, width: cropWidth, height: cropHeight }).png().toBuffer();
    if (isCard) await save("cards", name, sharp(crop).resize(card.width, card.height), 82);
    await save("index", name, sharp(crop).resize(thumb.width, thumb.height), 80);
    await page.close();
  }
} finally {
  await browser.close();
}
