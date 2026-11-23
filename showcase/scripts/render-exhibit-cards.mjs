// Make the small stills that stand for an exhibit elsewhere on the site:
//   cards/NAME.webp   the picture on a lab exhibit's card on the home page (840 x 560)
//   index/NAME.webp   the thumbnail beside each entry of the header's index (330 x 220)
//   cards/strip-*.webp the two thumbnails under the drape room (416 px wide)
// A still is either the exhibit's own stage, captured from the served site at its defaults, or an existing
// film still cut down. Pass a new output directory; review the files, then copy cards/ and index/ into
// public/media/.
//   FTX_STUDIO_URL=http://127.0.0.1:3000 CHROMIUM_EXECUTABLE=... node scripts/render-exhibit-cards.mjs OUT_DIR
//   node scripts/render-exhibit-cards.mjs OUT_DIR home      only home/NAME.webp, from files in public/
import { mkdir, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { chromium } from "playwright-core";
import sharp from "sharp";

// name, route, the element captured, the share of it kept about its centre (to leave out edge labels a 3:2
// crop would cut through), and whether it is also a card.
const captures = [
  ["waistcoat", "/wardrobe/waistcoat/", ".wc-svg", .86, true],
  ["inside-out", "/wardrobe/waistcoat/inside-out/", ".io-canvas-host canvas", 1, true],
  ["sundress-compare", "/wardrobe/sundress/compare/", ".sc-canvas-host canvas", 1, true],
  ["thermal", "/experiments/thermal/", ".th-iso-svg", 1, true],
  ["wash", "/experiments/wash/", ".ws-svg", 1, true],
  ["friction", "/experiments/friction/", ".fs-svg", 1, true],
  ["wrap-skirt", "/studies/wrap-skirt/", ".ws-svg", 1, true],
  ["tiling", "/studies/tiling/", ".tf-svg", .78, true],
  ["sewing", "/studies/sewing/", ".so-flat-svg", 1, true],
  ["drape-room", "/sections/studies/", ".dr-canvas canvas", .8, false],
  ["redraft", "/wardrobe/sundress/redraft/", ".sr-svg", 1, false],
  ["embroidery", "/wardrobe/hussar/embroidery/", "main canvas", 1, false],
  ["tracing", "/sections/tracing/", "main figure", 1, false],
];
// Index entries that already have a film still or a review picture: name, source file and, where the
// picture is not near 3:2, the 3:2 part of it to use (in source pixels).
const stills = [
  ["skirt", "public/media/skirt-atelier.jpg"],
  ["trousers", "public/media/trouser-form.jpg"],
  // The review sheet is 2000 x 777 with three captioned panels; this is the cuff and hand of its second render.
  ["visual-fitting", "public/fittings/sleeve-contact-review.png", { left: 978, top: 355, width: 306, height: 204 }],
  ["wardrobe", "public/media/wardrobe-hussar.jpg"],
  ["studios", "public/media/lab-draft.webp"],
  ["materials", "public/media/rig-cylinder.jpg"],
  ["drape-gallery", "public/media/bolero.jpg"],
  ["films", "public/media/studio-tour.jpg"],
];
// Two links under the drape room show a 4K film still at about 100 px wide. These are the same stills at
// four times that width, so the page does not fetch and decode 3840 x 2160 for a thumbnail.
const strips = [
  ["strip-trouser-posture", "public/media/trouser-posture.jpg"],
  ["strip-rig", "public/media/rig-strip.jpg"],
];
// The home page's pictures (home/NAME.webp), 3:2 at 1440 x 960.
// A pattern drawing: an exported master SVG, the part of it to show (in its own millimetres), drawn in ink on
// paper with its hairlines thickened so they survive the reduction.
const drawings = [
  ["pattern", "public/sundress/master.svg", { left: 2, top: 640, width: 1380, height: 920 }],
];
// The two sleeve pictures of the visual-fitting review sheet, at their own size and without its captions.
const details = [
  ["sleeve-before", "public/fittings/sleeve-contact-review.png", { left: 233, top: 70, width: 310, height: 500 }],
  ["sleeve-after", "public/fittings/sleeve-contact-review.png", { left: 977, top: 70, width: 310, height: 500 }],
];
// Portraits of the sewn garments: the middle of each construction film's poster, where the form stands.
const portraits = ["sundress", "blue", "flare", "ensemble", "trousers", "bolero"];
const card = { width: 840, height: 560 }, thumb = { width: 330, height: 220 }; // both 3:2
const plate = { width: 1440, height: 960 }, portrait = { width: 960, height: 1200 };

const destination = resolve(process.argv[2]);
await mkdir(destination);
await mkdir(resolve(destination, "cards"));
await mkdir(resolve(destination, "index"));
await mkdir(resolve(destination, "home"));
const save = async (folder, name, image, quality) => {
  const bytes = await image.webp({ quality }).toBuffer();
  await writeFile(resolve(destination, folder, `${name}.webp`), bytes);
  console.log(`${folder}/${name}.webp ${bytes.length} bytes`);
};
const source = file => resolve(import.meta.dirname, "..", file);

for (const [name, file, part] of drawings) {
  // At 72 dpi a millimetre is 72/25.4 px. Lines are dark on white, so thicken them on the negative.
  const scale = 72 / 25.4;
  const region = Object.fromEntries(Object.entries(part).map(([key, value]) => [key, Math.round(value * scale)]));
  // (sharp runs each operation at a fixed place in its pipeline whatever the call order, so each step that
  // depends on the one before is a pipeline of its own)
  const drawn = await sharp(source(file), { density: 72 }).flatten({ background: "#ffffff" }).extract(region)
    .greyscale().negate().png().toBuffer();
  const thick = await sharp(drawn).blur(3.4).linear(6.5, 0).png().toBuffer();
  const lines = await sharp(thick).negate().resize(plate.width, plate.height).toColourspace("srgb").png().toBuffer();
  // white becomes paper (#fbf8f1) and black becomes ink (#202c33)
  const paper = [251, 248, 241], ink = [32, 44, 51];
  await save("home", name, sharp(lines).linear(paper.map((p, k) => (p - ink[k]) / 255), ink), 84);
}
for (const [name, file, part] of details) await save("home", name, sharp(source(file)).extract(part), 92);
for (const name of portraits) {
  const poster = sharp(source(`public/media/construction-${name}.jpg`));
  const { width, height } = await poster.metadata();
  const cut = Math.round(height * portrait.width / portrait.height);
  await save("home", name, poster.extract({ left: Math.round((width - cut) / 2), top: 0, width: cut, height })
    .resize(portrait.width, portrait.height), 82);
}
// `home` as a second argument stops here: those pictures need no served site.
if (process.argv[3] === "home") process.exit(0);
for (const [name, file] of strips) await save("cards", name, sharp(source(file)).resize(416), 80);
for (const [name, file, part] of stills) {
  const picture = part ? sharp(source(file)).extract(part) : sharp(source(file));
  await save("index", name, picture.resize(thumb.width, thumb.height, { fit: "cover" }), 80);
}

const origin = process.env.FTX_STUDIO_URL || "http://127.0.0.1:3000";
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(),
  args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--disable-dev-shm-usage"] });
try {
  const context = await browser.newContext({ userAgent: "OpenAI File Downloader, XaiImageApiFetch/1.0",
    viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2, reducedMotion: "reduce" });
  for (const [name, route, selector, share, isCard] of captures) {
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
    // The largest 3:2 rectangle inside the kept share of the element, about its centre.
    const full = await sharp(shot).metadata();
    const cropWidth = Math.floor(Math.min(full.width * share, full.height * share * card.width / card.height));
    const cropHeight = Math.floor(cropWidth * card.height / card.width);
    const left = Math.floor((full.width - cropWidth) / 2);
    const top = Math.floor((full.height - cropHeight) / 2);
    const crop = await sharp(shot).extract({ left, top, width: cropWidth, height: cropHeight }).png().toBuffer();
    if (isCard) await save("cards", name, sharp(crop).resize(card.width, card.height), 82);
    await save("index", name, sharp(crop).resize(thumb.width, thumb.height), 80);
    await page.close();
  }
} finally {
  await browser.close();
}
